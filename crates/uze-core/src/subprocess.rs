//! Shared child-process discipline: every long-running child UZE spawns runs
//! in its own process group, is polled against a deadline, and — on timeout —
//! has its whole group killed, never just the direct child. Hook handlers,
//! Git acquisition, installer runners, and vendor-CLI capture all share these
//! helpers so no caller silently regresses to a bare `child.kill()`.
//!
//! Not part of the public contract; workspace-internal process plumbing.

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

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
/// The `/bin/kill` coreutils binary is resolved absolutely so this never
/// depends on `PATH` inside a sanitized or harness environment.
///
/// The negative-PID group signal below is necessary but not sufficient: on
/// some kernels (observed under WSL2) a descendant that shares the exact
/// same PGID as the shell can still survive a `kill(2)` targeted at that
/// process group, even though a direct single-PID SIGKILL to that same
/// descendant reliably kills it. So this also enumerates `/proc` for every
/// process still reporting the handler's PGID and kills each one directly,
/// which is what actually reaches a straggler the group signal missed.
#[cfg(unix)]
pub fn kill_process_group(pid: u32) {
    let _ = Command::new("/bin/kill")
        .arg("-KILL")
        .arg(format!("-{pid}"))
        .status();
    // Fall back to killing just the direct child if the group kill failed
    // (e.g. the process already exited between the poll and the kill).
    let _ = Command::new("/bin/kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
    for member in process_group_members(pid) {
        let _ = Command::new("/bin/kill")
            .arg("-KILL")
            .arg(member.to_string())
            .status();
    }
}

#[cfg(not(unix))]
pub fn kill_process_group(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .status();
}

/// Reads `/proc` directly (rather than shelling out to `ps --pgid`, whose
/// group filter has been unreliable in the same environments where the
/// group-wide signal misses a member) to list every PID currently reporting
/// `pgid` as its process group.
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

/// Reads a child handle to the end, stopping (and overflowing) past `cap`
/// bytes so a chatty child cannot exhaust memory.
pub fn read_bounded<R: Read>(mut handle: R, cap: usize) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match handle.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if bytes.len() + read > cap {
                    bytes.extend_from_slice(&buffer[..cap.saturating_sub(bytes.len())]);
                    return (bytes, true);
                }
                bytes.extend_from_slice(&buffer[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (bytes, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bounded_caps_and_signals_overflow() {
        let payload = vec![b'x'; 10_000];
        let (bytes, overflowed) = read_bounded(&payload[..], 4096);
        assert_eq!(bytes.len(), 4096);
        assert!(overflowed);
        let (bytes, overflowed) = read_bounded(&payload[..], 100);
        assert_eq!(bytes.len(), 100);
        assert!(overflowed);
    }

    #[test]
    fn read_bounded_reads_through_without_overflow() {
        let payload = vec![b'a'; 100];
        let (bytes, overflowed) = read_bounded(&payload[..], 4096);
        assert_eq!(bytes, payload);
        assert!(!overflowed);
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
        let mut child = Command::new("/bin/true").spawn().unwrap();
        let (status, timed_out) = wait_with_timeout(&mut child, Duration::from_secs(5)).unwrap();
        assert!(!timed_out);
        assert!(status.success());
    }
}
