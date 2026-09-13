//! Shared child-process discipline: every long-running child UZE spawns runs
//! in its own process group, is polled against a deadline, and — on timeout —
//! has its whole group killed, never just the direct child. Hook handlers,
//! Git acquisition, installer runners, and vendor-CLI capture all share these
//! helpers so no caller silently regresses to a bare `child.kill()`.
//!
//! Not part of the public contract; workspace-internal process plumbing.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How much of a shell command's output is kept, **per stream**: stdout and
/// stderr are read by separate threads and capped independently, so a
/// command that fills both is reported with up to twice this much. What is
/// kept is the tail of each; whatever fell off the front is counted and
/// announced rather than silently lost.
const MAX_SHELL_OUTPUT_BYTES: usize = 64 * 1024;

/// How long a finished command's readers are given to hand over what they
/// captured. They are already at EOF in every ordinary case; this bounds the
/// one case that is not — a descendant still holding a pipe open.
const READER_GRACE: Duration = Duration::from_secs(2);

/// Runs `command` in its own process group so a timeout can kill the whole
/// tree (the process AND its descendants), never just the direct child.
/// No-op on platforms without process groups.
pub fn with_process_group(mut command: Command) -> Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// Polls `child` to completion but never longer than `timeout`.
///
/// Returning `(status, false)` means the process exited on its own.
/// Returning `(status, true)` means the deadline was reached: the whole
/// process group was killed (sweeping `/proc` for stragglers both before and
/// after reaping, because a descendant forked between the two can otherwise
/// survive the group signal) and the direct child was reaped so it cannot
/// stay a zombie.
pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> io::Result<(ExitStatus, bool)> {
    let pid = child.id();
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status, false)),
            Ok(None) if Instant::now() >= deadline => {
                kill_process_group(pid);
                let status = child.wait()?;
                // A descendant forked after the first sweep can still be
                // alive here; the PGID survives the direct child's death, so
                // sweep again after reaping.
                kill_process_group(pid);
                return Ok((status, true));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(source) => {
                kill_process_group(pid);
                return Err(source);
            }
        }
    }
}

/// Kills a whole process group — the process plus any descendant it started.
///
/// This issues the `kill(2)` syscalls directly and must never shell out to
/// `/bin/kill`. procps-ng's `kill` parses a negative pid by its *first digit
/// only* (`case '?'`: `pid = '0' - optopt`, then exits), so `kill -KILL -1234`
/// or `-KILL -10000` becomes `kill(-1, SIGKILL)` — every process the user
/// owns, including the login shell, `systemd --user`, and the agent that ran
/// the tests — while `-KILL -2345` becomes a silent no-op against group 2.
/// The latter is the "group signal misses a member under WSL2" quirk this
/// helper used to work around with a `/proc` sweep; the former is what took
/// the whole WSL session down whenever a timed-out child's pid started with
/// `1`. The sweep is kept as a belt-and-braces pass for a descendant that
/// left the group (`setsid`) between the two signals.
#[cfg(unix)]
pub fn kill_process_group(pid: u32) {
    // `kill(0)` / `kill(-1)` would target our own group / every process we
    // own. No child ever has such a pid; refuse rather than risk it.
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return;
    };
    if pgid <= 1 {
        return;
    }
    // SAFETY: plain `kill(2)` calls on a pid we spawned; no memory involved.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
        // Also signal the direct child by pid, in case it changed its own
        // process group before the group signal landed.
        libc::kill(pgid, libc::SIGKILL);
    }
    for member in process_group_members(pid) {
        if let Ok(member) = libc::pid_t::try_from(member)
            && member > 1
        {
            // SAFETY: as above.
            unsafe {
                libc::kill(member, libc::SIGKILL);
            }
        }
    }
}

#[cfg(not(unix))]
pub fn kill_process_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .status();
}

/// Reads `/proc` directly (rather than shelling out to `ps --pgid`) to list
/// every PID currently reporting `pgid` as its process group.
///
/// Empty on a platform without `/proc` — macOS included — and that is
/// correct rather than merely tolerable: this sweep is the belt-and-braces
/// pass for a descendant that left the group via `setsid`, added for a WSL2
/// quirk. The two `kill(2)` calls above are what actually kill the group,
/// and they are portable. A platform where the sweep finds nothing loses a
/// backstop, not the kill.
#[cfg(unix)]
fn process_group_members(pgid: u32) -> Vec<u32> {
    let mut members = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return members;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // Fields are space-separated, but the second field (comm) is
        // parenthesized and may itself contain spaces or parens, so split
        // on the last ')' rather than naively splitting on whitespace.
        let Some((_, after_comm)) = stat.rsplit_once(')') else {
            continue;
        };
        // After comm: state(0) ppid(1) pgrp(2) ...
        let pgrp = after_comm
            .split_whitespace()
            .nth(2)
            .and_then(|field| field.parse::<u32>().ok());
        if pgrp == Some(pgid) {
            members.push(pid);
        }
    }
    members
}

/// Reads a child handle to EOF, keeping the **last** `cap` bytes and
/// reporting how many were dropped off the front.
///
/// The tail, not the head: every runner a gate or a `setup` step wraps
/// writes what went wrong last and its progress noise first, so a head-kept
/// cap hands the operator 64 KiB of "Compiling …" and none of the failure
/// it was reported for.
///
/// Past the cap it keeps reading and throws the oldest bytes away rather
/// than returning. A reader that stopped early would leave the pipe
/// undrained, and the child then blocks on its next `write()` — or takes
/// `SIGPIPE` once the handle is dropped. Either way a merely chatty child
/// becomes a hang until its deadline, or a spurious failure, which is the
/// opposite of what a cap is for: the cap bounds *memory*, not how much the
/// child is allowed to say.
pub fn read_bounded<R: Read>(mut handle: R, cap: usize) -> (Vec<u8>, usize) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut dropped = 0usize;
    // Trimming only once the buffer has grown to twice the cap keeps the
    // cost of holding a tail linear in what the child said: each compaction
    // moves at most `cap` bytes and buys `cap` bytes of headroom.
    let slack = cap.saturating_mul(2);
    let mut trim = |bytes: &mut Vec<u8>, limit: usize| {
        if bytes.len() > limit {
            let excess = bytes.len() - cap;
            bytes.drain(..excess);
            dropped += excess;
        }
    };
    loop {
        match handle.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&buffer[..read]);
                trim(&mut bytes, slack);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    trim(&mut bytes, cap);
    (bytes, dropped)
}

/// Whether an executable of this name is reachable through `PATH`. A
/// generated artifact may depend on a small system program (the hook
/// wrapper's `jq`); this is how a diagnostic checks for one without
/// running it.
pub fn program_on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(program);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

/// Runs a shell command in `cwd`, bounded in time and output. Returns
/// whether it exited zero, and its combined stdout and stderr — for the
/// project-declared commands (a checkout's setup, a delivery's gate, a
/// forge CLI) whose output is what the operator or the agent is told.
pub fn run_shell_bounded(cwd: &Path, command: &str, timeout: Duration) -> (bool, String) {
    let shell = if Path::new("/bin/sh").exists() {
        "/bin/sh"
    } else {
        "sh"
    };
    let mut invocation = Command::new(shell);
    invocation.arg("-c").arg(command);
    let child = with_process_group(invocation)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(cwd)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return (false, format!("could not run `{command}`: {error}")),
    };
    // One reader per stream, never one reading them in turn: a pipe holds
    // about 64 KiB, and a gate command is stderr-heavy (every compiler and
    // test runner is). Reading stdout to EOF first would let stderr fill and
    // block the child, so stdout never reaches EOF either and the whole
    // command hangs until its deadline — reported as a timeout, with nothing
    // captured to say otherwise.
    let pid = child.id();
    let stdout = drain_on_thread(child.stdout.take().expect("piped"));
    let stderr = drain_on_thread(child.stderr.take().expect("piped"));
    let (status, timed_out) = match wait_with_timeout(&mut child, timeout) {
        Ok(outcome) => outcome,
        Err(error) => return (false, format!("`{command}` failed: {error}")),
    };
    // The whole group was killed if it timed out, so both pipes close and
    // the readers finish; a descendant that left the group could still hold
    // one open, so the wait is bounded rather than unconditional.
    let mut swept = false;
    let stdout = stdout.collect(pid, &mut swept);
    let stderr = stderr.collect(pid, &mut swept);
    let captured = combine_streams(&stdout, &stderr);
    if timed_out {
        // What the command managed to say before the deadline is usually the
        // only evidence of *why* it never finished, so it is reported rather
        // than discarded.
        let mut report = format!("`{command}` timed out after {}s", timeout.as_secs());
        if !captured.trim().is_empty() {
            report.push('\n');
            report.push_str(captured.trim());
        }
        return (false, report);
    }
    (status.success(), captured.trim().to_owned())
}

/// One stream's reader: the thread draining it, and the channel it answers
/// through so the caller can bound how long it waits.
struct Drain {
    reader: thread::JoinHandle<()>,
    answer: mpsc::Receiver<(Vec<u8>, usize)>,
}

/// What a stream had to say, or why nothing could be said for it.
enum Stream {
    Read {
        bytes: Vec<u8>,
        dropped: usize,
    },
    /// The reader never reached EOF: something still holds the pipe open.
    Unread,
}

/// Starts draining `handle` immediately, keeping the join handle so a reader
/// that cannot finish is a bounded wait rather than a thread leaked for the
/// life of the process.
fn drain_on_thread<R: Read + Send + 'static>(handle: R) -> Drain {
    let (sender, answer) = mpsc::channel();
    let reader = thread::spawn(move || {
        let _ = sender.send(read_bounded(handle, MAX_SHELL_OUTPUT_BYTES));
    });
    Drain { reader, answer }
}

impl Drain {
    /// Waits out the reader, sweeping the process group once if it cannot
    /// finish.
    ///
    /// A descendant that left the group — a `setsid`'d daemon, a dev server,
    /// a language server a suite started — still holds the pipe, so the
    /// reader never sees EOF. Killing the group is what closes it; `swept`
    /// keeps the two streams from each paying for their own kill.
    fn collect(self, pid: u32, swept: &mut bool) -> Stream {
        if let Ok((bytes, dropped)) = self.answer.recv_timeout(READER_GRACE) {
            // Sending is the reader's last act, so this joins a thread that
            // is already on its way out rather than waiting on one.
            let _ = self.reader.join();
            return Stream::Read { bytes, dropped };
        }
        if !*swept {
            kill_process_group(pid);
            *swept = true;
        }
        match self.answer.recv_timeout(READER_GRACE) {
            Ok((bytes, dropped)) => {
                let _ = self.reader.join();
                Stream::Read { bytes, dropped }
            }
            // Joining here would block on a `read` nothing can end. The
            // thread is left to finish on its own, and the caller is told
            // the stream is missing rather than handed an empty one.
            Err(_) => Stream::Unread,
        }
    }
}

impl Stream {
    fn bytes(&self) -> &[u8] {
        match self {
            Stream::Read { bytes, .. } => bytes,
            Stream::Unread => &[],
        }
    }

    fn dropped(&self) -> usize {
        match self {
            Stream::Read { dropped, .. } => *dropped,
            Stream::Unread => 0,
        }
    }
}

/// Both streams as the operator reads them, with the two things that would
/// otherwise be a silent lie stated out loud: output that fell off the front
/// of the cap, and a stream nothing could be read from.
fn combine_streams(stdout: &Stream, stderr: &Stream) -> String {
    let mut combined = String::new();
    let dropped = stdout.dropped() + stderr.dropped();
    if dropped > 0 {
        combined.push_str(&format!(
            "... [output truncated, {dropped} bytes dropped]\n"
        ));
    }
    for (name, stream) in [("stdout", stdout), ("stderr", stderr)] {
        if matches!(stream, Stream::Unread) {
            combined.push_str(&format!(
                "... [{name} could not be read: a process is still holding it open]\n"
            ));
        }
    }
    combined.push_str(&String::from_utf8_lossy(stdout.bytes()));
    let stderr = String::from_utf8_lossy(stderr.bytes());
    if !stderr.trim().is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    combined
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_program_is_found_only_where_path_actually_holds_an_executable() {
        assert!(super::program_on_path("sh"), "the system shell is on PATH");
        assert!(!super::program_on_path("a-program-nobody-installed"));
    }

    use super::*;

    #[test]
    fn read_bounded_caps_and_counts_what_it_dropped() {
        let payload = vec![b'x'; 10_000];
        let (bytes, dropped) = read_bounded(&payload[..], 4096);
        assert_eq!(bytes.len(), 4096);
        assert_eq!(dropped, 10_000 - 4096);
        let (bytes, dropped) = read_bounded(&payload[..], 100);
        assert_eq!(bytes.len(), 100);
        assert_eq!(dropped, 10_000 - 100);
    }

    #[test]
    fn read_bounded_reads_through_without_overflow() {
        let payload = vec![b'a'; 100];
        let (bytes, dropped) = read_bounded(&payload[..], 4096);
        assert_eq!(bytes, payload);
        assert_eq!(dropped, 0);
    }

    /// What a runner says last is what it failed on; what it says first is
    /// progress noise. A head-kept cap handed the operator 64 KiB of
    /// "Compiling …" and none of the failure the gate was reported for.
    #[test]
    fn read_bounded_keeps_the_end_of_a_long_stream() {
        let mut payload = b"first\n".repeat(20_000);
        payload.extend_from_slice(b"FAILURES: test_auth_redirect FAILED\n");
        let (bytes, dropped) = read_bounded(&payload[..], 4096);

        assert!(dropped > 0);
        assert!(
            String::from_utf8_lossy(&bytes).contains("test_auth_redirect FAILED"),
            "the failure line was dropped in favour of the progress noise"
        );
    }

    /// A pipe holds ~64 KiB. A command writing past that on stderr while
    /// stdout is still open blocks unless both streams are drained at once,
    /// and blocks for the command's *whole* timeout — 30 minutes for a
    /// delivery gate. Finishing at all is the assertion.
    #[test]
    fn a_stderr_heavy_command_is_not_starved_by_an_open_stdout() {
        let started = Instant::now();
        let (succeeded, output) = run_shell_bounded(
            Path::new("."),
            "head -c 1048576 /dev/zero | tr '\\0' 'e' 1>&2; echo done",
            Duration::from_secs(30),
        );
        assert!(succeeded, "the command did not exit zero: {output}");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the command was starved rather than drained"
        );
        assert!(output.contains("done"), "stdout was lost: {output}");
    }

    /// The mirror case: output past the cap must be discarded, not left in
    /// the pipe. A reader that stopped would block the child, or `SIGPIPE`
    /// it once the handle dropped — a verbose command reported as failed.
    #[test]
    fn a_command_writing_past_the_cap_still_exits_zero_with_truncated_output() {
        let (succeeded, output) = run_shell_bounded(
            Path::new("."),
            "head -c 1048576 /dev/zero | tr '\\0' 'o'",
            Duration::from_secs(30),
        );
        assert!(succeeded, "a verbose command was reported as failed");
        assert!(
            output.len() <= MAX_SHELL_OUTPUT_BYTES + 64,
            "{}",
            output.len()
        );
        assert!(!output.is_empty(), "nothing was captured");
        assert!(
            output.contains("[output truncated, "),
            "truncation was not announced: {}",
            &output[..80.min(output.len())]
        );
    }

    /// The gate the reviewer reproduced: thousands of progress lines, then
    /// the one line naming what failed. A report that keeps the head is a
    /// report with no evidence in it.
    #[test]
    fn a_failing_gate_reports_the_failure_and_not_the_progress_noise() {
        let (succeeded, output) = run_shell_bounded(
            Path::new("."),
            "i=0; while [ $i -lt 4000 ]; do echo \"   Compiling crate-$i v0.1.0\"; \
             i=$((i+1)); done; echo 'FAILURES: test_auth_redirect FAILED'; exit 1",
            Duration::from_secs(60),
        );
        assert!(!succeeded);
        assert!(
            output.contains("FAILURES: test_auth_redirect FAILED"),
            "the failure line never reached the report"
        );
        assert!(
            output.contains("[output truncated, "),
            "the dropped progress output was not announced"
        );
    }

    /// A command that never finishes still had something to say about why.
    #[test]
    fn a_timed_out_command_reports_what_it_managed_to_say() {
        let (succeeded, output) = run_shell_bounded(
            Path::new("."),
            "echo preface; sleep 30",
            Duration::from_millis(300),
        );
        assert!(!succeeded);
        assert!(output.contains("timed out"), "{output}");
        assert!(
            output.contains("preface"),
            "the captured tail was lost: {output}"
        );
    }

    /// A gate or `setup` step that leaves a descendant on the pipe — a dev
    /// server, a language server a suite starts — used to have its output
    /// replaced by `""` after the grace expired, so a step that *failed* was
    /// reported with no evidence at all. Sweeping the group closes the pipe.
    #[test]
    fn a_step_whose_pipe_a_survivor_holds_open_is_still_reported() {
        let started = Instant::now();
        let (succeeded, output) = run_shell_bounded(
            Path::new("."),
            "sleep 60 & echo 'the step said this'",
            Duration::from_secs(30),
        );
        assert!(succeeded);
        assert!(
            output.contains("the step said this"),
            "the step's output was lost to a survivor on the pipe: {output:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the survivor was never swept"
        );
    }

    /// A stream nothing could be read from is a fact about the report, not
    /// an empty stream: saying so is the difference between "the step was
    /// silent" and "we could not hear it".
    #[test]
    fn a_stream_that_could_not_be_read_says_so() {
        let combined = combine_streams(
            &Stream::Read {
                bytes: b"partial".to_vec(),
                dropped: 12,
            },
            &Stream::Unread,
        );
        assert!(combined.contains("[output truncated, 12 bytes dropped]"));
        assert!(combined.contains("[stderr could not be read"));
        assert!(combined.contains("partial"));
    }

    #[test]
    fn wait_with_timeout_kills_a_hung_child() {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let (status, timed_out) =
            wait_with_timeout(&mut child, Duration::from_millis(300)).unwrap();
        assert!(timed_out);
        assert!(!status.success());
    }

    #[test]
    fn wait_with_timeout_returns_promptly_for_a_fast_child() {
        // `/bin/sh -c 'exit 0'`, not `/bin/true`: macOS keeps `true` under
        // `/usr/bin` and ships no `/bin/true`. `/bin/sh` is the one path
        // POSIX promises, and any process that exits at once will do.
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let (status, timed_out) = wait_with_timeout(&mut child, Duration::from_secs(5)).unwrap();
        assert!(!timed_out);
        assert!(status.success());
    }
}
