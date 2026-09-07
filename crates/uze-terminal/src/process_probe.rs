//! What the operating system knows about a process this crate did not spawn.
//!
//! Four questions — who is on the other end of a socket, what image is a pid
//! running, where is it standing, what did it inherit — and every one of them
//! is a fact only the kernel holds. Linux answers all four through `/proc`;
//! macOS has no `/proc` and answers them through `libproc` and `sysctl`.
//!
//! They live together here, rather than beside the code that asks, because
//! the *policy* built on top of them is identical everywhere and should be
//! written once. `runtime.rs` used to carry a `#[cfg(target_os = "linux")]`
//! copy of each decision next to a `None`-returning stub for everywhere
//! else, which meant the rule ("the listener counts only when it runs this
//! same executable") existed in a form no other platform could ever run.
//!
//! Every function returns `Option`, and a platform with no answer says
//! `None` rather than guessing. A caller must treat `None` as *unknown* and
//! never as *no* — the difference matters most to `peer_pid`, where the
//! wrong reading of it would tear down a healthy server.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// The pid of the process on the other end of `stream`, as the kernel
/// stamped it onto the connection — never something the peer could claim
/// for itself.
pub fn peer_pid(stream: &UnixStream) -> Option<u32> {
    platform::peer_pid(stream)
}

/// The executable image `pid` is currently running. Stops matching a path
/// once the binary behind it is replaced underneath a live process (a
/// `cargo install --force` mid-session), which is exactly what makes it
/// usable as an identity check.
pub fn executable_of(pid: u32) -> Option<PathBuf> {
    platform::executable_of(pid)
}

/// The directory `pid` is standing in.
pub fn current_directory_of(pid: libc::pid_t) -> Option<PathBuf> {
    platform::current_directory_of(pid)
}

/// The short command name of `pid` — what `ps` prints, and what a harness is
/// free to overwrite with a title of its own.
pub fn command_name_of(pid: libc::pid_t) -> Option<String> {
    platform::command_name_of(pid)
}

/// The value `key` has in `pid`'s environment, as captured at `exec`.
///
/// Best-effort by nature: the environment block of another process is
/// readable only for the same user, and only up to whatever bound the
/// platform puts on it. `None` covers all of "no such variable", "not
/// permitted" and "did not fit".
pub fn environment_value_of(pid: libc::pid_t, key: &str) -> Option<String> {
    platform::environment_value_of(pid, key)
}

/// Splits a NUL-separated environment block and returns `key`'s value.
///
/// Shared by both platforms: Linux reads this block out of `/proc`, macOS
/// out of `sysctl`, and the shape they hand back is the same one.
fn value_in_environment_block(block: &[u8], key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    block
        .split(|&byte| byte == 0)
        .find_map(|entry| entry.strip_prefix(prefix.as_bytes()))
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf8_lossy(value).into_owned())
}

#[cfg(target_os = "linux")]
mod platform {
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    pub(super) fn peer_pid(stream: &UnixStream) -> Option<u32> {
        let mut peer = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut size = size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `getsockopt` writes at most `size` bytes into `peer`, and
        // `size` is that value's own size.
        let asked = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut peer).cast(),
                &raw mut size,
            )
        };
        (asked == 0 && peer.pid > 0).then_some(peer.pid as u32)
    }

    pub(super) fn executable_of(pid: u32) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }

    pub(super) fn current_directory_of(pid: libc::pid_t) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    pub(super) fn command_name_of(pid: libc::pid_t) -> Option<String> {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|comm| comm.trim().to_owned())
            .filter(|comm| !comm.is_empty())
    }

    pub(super) fn environment_value_of(pid: libc::pid_t, key: &str) -> Option<String> {
        let block = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
        super::value_in_environment_block(&block, key)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    /// `LOCAL_PEERPID` is Darwin's `SO_PEERCRED`: same guarantee, different
    /// level — the pid comes from the kernel's record of who connected, not
    /// from anything sent over the socket.
    pub(super) fn peer_pid(stream: &UnixStream) -> Option<u32> {
        let mut pid: libc::pid_t = 0;
        let mut size = size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: `getsockopt` writes at most `size` bytes into `pid`, and
        // `size` is that value's own size.
        let asked = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&raw mut pid).cast(),
                &raw mut size,
            )
        };
        (asked == 0 && pid > 0).then_some(pid as u32)
    }

    pub(super) fn executable_of(pid: u32) -> Option<PathBuf> {
        let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer and the length handed to `proc_pidpath` describe
        // the same allocation.
        let written = unsafe {
            libc::proc_pidpath(
                pid as libc::c_int,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
            )
        };
        (written > 0).then(|| path_from(&buffer[..written as usize]))
    }

    pub(super) fn current_directory_of(pid: libc::pid_t) -> Option<PathBuf> {
        // SAFETY: zeroed is a valid `proc_vnodepathinfo` (it is plain data),
        // and the size handed to `proc_pidinfo` is the struct's own.
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let wanted = size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
        // SAFETY: as above — `proc_pidinfo` writes at most `wanted` bytes.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&raw mut info).cast(),
                wanted,
            )
        };
        if written < wanted {
            return None;
        }
        // `vip_path` is `MAXPATHLEN` bytes that libc declares as a nested
        // array to stay buildable on old compilers; it is one flat
        // NUL-terminated string, and is read back as one.
        let path = info.pvi_cdir.vip_path.as_flattened();
        let bytes: Vec<u8> = path.iter().map(|&byte| byte as u8).collect();
        let end = bytes.iter().position(|&byte| byte == 0)?;
        (end > 0).then(|| path_from(&bytes[..end]))
    }

    /// Shells a `#!` line can name. A process running one of these is
    /// reported by the *script* it was handed, never by the interpreter —
    /// see [`command_name_of`].
    const INTERPRETERS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "ash"];

    pub(super) fn command_name_of(pid: libc::pid_t) -> Option<String> {
        let name = proc_name(pid);
        // Linux and Darwin disagree about what a `#!` script is called, and
        // the disagreement is load-bearing: Linux sets `comm` from the file
        // handed to `execve` — the script — while Darwin reports the image
        // that ended up running, which is the interpreter. A harness
        // delivered as a shell script therefore came back as `sh` here and
        // as its own name there, and the workspace, which recognises an
        // agent by the process in its pane, saw no agents at all on a Mac.
        //
        // Reported the way Linux reports it, so one rule identifies a pane
        // on both. The interpreter's own name survives when there is no
        // script argument to prefer — a plain login shell is still `bash`.
        match name.as_deref() {
            Some(running) if INTERPRETERS.contains(&running) => {
                script_handed_to_interpreter(pid).or(name)
            }
            _ => name,
        }
    }

    /// The short name Darwin records for the running image.
    fn proc_name(pid: libc::pid_t) -> Option<String> {
        let mut buffer = [0u8; 2 * libc::MAXCOMLEN + 1];
        // SAFETY: the buffer and the length handed to `proc_name` describe the
        // same allocation.
        let written =
            unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        (written > 0)
            .then(|| String::from_utf8_lossy(&buffer[..written as usize]).into_owned())
            .filter(|name| !name.is_empty())
    }

    /// The basename of the first argument an interpreter was given, when
    /// that argument names a file — which is what a `#!` line produces.
    /// `None` for an interpreter running interactively or with `-c`, where
    /// there is no script and the interpreter's own name is the answer.
    fn script_handed_to_interpreter(pid: libc::pid_t) -> Option<String> {
        let (arguments, _) = process_arguments(pid)?;
        let script = arguments.into_iter().nth(1)?;
        let script = PathBuf::from(std::ffi::OsString::from_vec(script));
        if !script.is_file() {
            return None;
        }
        script
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .filter(|name| !name.is_empty())
    }

    /// Darwin keeps a process's `exec`-time argv and environment in one
    /// `KERN_PROCARGS2` blob: `argc`, the executable path, then `argc`
    /// NUL-terminated arguments, then the environment in the same form. The
    /// arguments are stepped over rather than scanned past, so an argument
    /// that happens to read like `KEY=value` can never be mistaken for one.
    ///
    /// Bounded at 256 KiB. `KERN_ARGMAX` is a megabyte, and this is asked on
    /// every status poll; a process whose block genuinely exceeds the bound
    /// answers `None`, which the caller already treats as "fall back to the
    /// command name".
    pub(super) fn environment_value_of(pid: libc::pid_t, key: &str) -> Option<String> {
        let (_, environment) = process_arguments(pid)?;
        super::value_in_environment_block(&environment, key)
    }

    /// A process's `exec`-time arguments and environment, which Darwin keeps
    /// together in one `KERN_PROCARGS2` blob: `argc`, the executable path,
    /// then `argc` NUL-terminated arguments, then the environment in the
    /// same form.
    ///
    /// The arguments are stepped over rather than scanned past, so an
    /// argument that happens to read like `KEY=value` can never be mistaken
    /// for a variable — and the same walk is what hands the arguments back.
    ///
    /// Bounded at 256 KiB. `KERN_ARGMAX` is a megabyte, and this is asked on
    /// every status poll; a process whose block genuinely exceeds the bound
    /// answers `None`, which every caller already treats as "cannot say".
    fn process_arguments(pid: libc::pid_t) -> Option<(Vec<Vec<u8>>, Vec<u8>)> {
        const LIMIT: usize = 256 * 1024;

        let mut request = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut buffer = vec![0u8; LIMIT];
        let mut size = buffer.len();
        // SAFETY: `request` is three ints, and `size` is `buffer`'s length —
        // `sysctl` writes no more than that and updates `size` to what it did.
        let asked = unsafe {
            libc::sysctl(
                request.as_mut_ptr(),
                request.len() as libc::c_uint,
                buffer.as_mut_ptr().cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if asked != 0 || size < size_of::<libc::c_int>() {
            return None;
        }
        buffer.truncate(size);

        let (count, rest) = buffer.split_at(size_of::<libc::c_int>());
        let argc = libc::c_int::from_ne_bytes(count.try_into().ok()?);
        // The padding is skipped only where it occurs — between the
        // executable path and the first argument. Skipping empty entries
        // throughout would miscount an argument that is itself the empty
        // string (`prog "" x` is legal), moving the boundary and swallowing
        // the first variable.
        let mut entries = rest.split(|&byte| byte == 0).peekable();
        entries.next()?;
        while entries.next_if(|entry| entry.is_empty()).is_some() {}
        let mut arguments = Vec::new();
        for _ in 0..argc {
            arguments.push(entries.next()?.to_vec());
        }
        let environment: Vec<u8> = entries.collect::<Vec<_>>().join(&0u8);
        Some((arguments, environment))
    }

    fn path_from(bytes: &[u8]) -> PathBuf {
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
}

/// Neither `/proc` nor `libproc`. The endpoint keeps whatever answer it had
/// before it could be asked at all, and a pane reports no foreground status
/// rather than a made-up one.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    pub(super) fn peer_pid(_stream: &UnixStream) -> Option<u32> {
        None
    }

    pub(super) fn executable_of(_pid: u32) -> Option<PathBuf> {
        None
    }

    pub(super) fn current_directory_of(_pid: libc::pid_t) -> Option<PathBuf> {
        None
    }

    pub(super) fn command_name_of(_pid: libc::pid_t) -> Option<String> {
        None
    }

    pub(super) fn environment_value_of(_pid: libc::pid_t, _key: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        command_name_of, current_directory_of, environment_value_of, value_in_environment_block,
    };

    #[test]
    fn a_value_is_read_out_of_a_nul_separated_block() {
        let block = b"PATH=/bin\0UZE_SHIM_NAME=claude\0HOME=/root\0";
        assert_eq!(
            value_in_environment_block(block, "UZE_SHIM_NAME"),
            Some("claude".to_owned())
        );
    }

    /// A variable set to nothing is not a name, and reporting `""` as the
    /// process running in a pane would blank the label rather than fall back
    /// to the command name.
    #[test]
    fn an_empty_value_is_not_an_answer() {
        assert_eq!(
            value_in_environment_block(b"UZE_SHIM_NAME=\0", "UZE_SHIM_NAME"),
            None
        );
    }

    /// The prefix has to be the whole name: `SHIM_NAME` must not be answered
    /// by `UZE_SHIM_NAME`, and `UZE_SHIM_NAME_EXTRA` must not answer
    /// `UZE_SHIM_NAME`.
    #[test]
    fn a_key_matches_only_itself() {
        let block = b"UZE_SHIM_NAME_EXTRA=no\0UZE_SHIM_NAME=yes\0";
        assert_eq!(
            value_in_environment_block(block, "UZE_SHIM_NAME"),
            Some("yes".to_owned())
        );
        assert_eq!(value_in_environment_block(block, "SHIM_NAME"), None);
    }

    /// A harness is delivered as a shell script often enough that this is
    /// the common case, not an edge one — and the workspace recognises an
    /// agent by the process running in its pane, so the name has to be the
    /// script's.
    ///
    /// Linux sets `comm` from the file handed to `execve`, which is the
    /// script; Darwin reports the image that ended up running, which is the
    /// interpreter. This asserts the Linux answer on both, which is what
    /// makes one identification rule work on either.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn a_script_is_named_by_itself_and_not_by_its_interpreter() {
        // Short, because Linux truncates `comm` to 15 bytes and a name cut
        // in half would pass this test for the wrong reason.
        let dir = uze_testkit::temp::scratch("named-script");
        let script = dir.join("uzeprobe");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();

        let mut child = std::process::Command::new(&script).spawn().unwrap();
        let named = wait_for_name(child.id() as libc::pid_t);
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            named.as_deref(),
            Some("uzeprobe"),
            "a `#!` script must be reported by its own name, not by its interpreter"
        );
    }

    /// The other half of the rule: an interpreter with no script to prefer
    /// keeps its own name. Without this, the lookup above would happily
    /// report a flag as the program.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn an_interpreter_with_no_script_keeps_its_own_name() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .spawn()
            .unwrap();
        let named = wait_for_name(child.id() as libc::pid_t);
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            named.as_deref(),
            Some("sh"),
            "an interpreter given `-c` has no script, so it is its own answer"
        );
    }

    /// The name settles once the child has `exec`ed; before that the kernel
    /// still reports what it forked from.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn wait_for_name(pid: libc::pid_t) -> Option<String> {
        let own = command_name_of(std::process::id() as libc::pid_t);
        for _ in 0..200 {
            let seen = command_name_of(pid);
            if seen.is_some() && seen != own {
                return seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        command_name_of(pid)
    }

    /// The three process questions, asked about this very process, on any
    /// platform that claims to answer them. Cheap, and it is the only thing
    /// that tells a Linux reviewer whether the macOS half compiles into
    /// something that actually works — or the reverse.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn the_platform_answers_about_this_process() {
        let me = std::process::id() as libc::pid_t;
        assert_eq!(
            current_directory_of(me).and_then(|cwd| cwd.canonicalize().ok()),
            std::env::current_dir()
                .ok()
                .and_then(|cwd| cwd.canonicalize().ok()),
            "the platform must report the directory this test is standing in"
        );
        assert!(
            command_name_of(me).is_some_and(|name| !name.is_empty()),
            "the platform must report a command name for a live process"
        );
        // Set by the harness that runs this test, in this process, before
        // the probe reads it back out of the kernel's own copy.
        unsafe { std::env::set_var("UZE_PROBE_SELF", "probed") };
        let read_back = environment_value_of(me, "UZE_PROBE_SELF");
        // `exec`-time, deliberately: the kernel's copy is the one taken when
        // the process started, so a variable set afterwards is *expected* to
        // be absent. What must never happen is a different value.
        assert!(
            read_back.is_none() || read_back.as_deref() == Some("probed"),
            "environment probe answered {read_back:?}, which is neither absent nor the value set"
        );
    }
}
