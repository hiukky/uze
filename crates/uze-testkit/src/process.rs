//! Executables a test runs without ever having held a descriptor to them.

use std::path::Path;
use std::process::{Command, Stdio};

/// Writes `bytes` to `path` as an executable, through a process that has
/// exited before this returns.
///
/// A file cannot be `exec`ed while any descriptor to it is open for
/// writing, and a test binary spawns from several threads at once: a
/// descriptor this process opens is copied into every child a sibling test
/// forks, until that child's own `exec` closes it. That instant is the
/// kernel's `ETXTBSY`, and it belongs to the harness, not to the test.
/// Writing through a child of our own leaves the descriptor in a process
/// that is gone — waited on — by the time anything runs the file.
pub fn install_executable(path: &Path, bytes: &[u8]) {
    let mut writer = Command::new("sh")
        .args(["-c", r#"cat > "$1" && chmod 0755 "$1""#, "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("a POSIX shell is on PATH");
    let mut stdin = writer.stdin.take().expect("stdin is piped");
    std::io::Write::write_all(&mut stdin, bytes).expect("the writer reads its whole input");
    drop(stdin);
    let status = writer.wait().expect("the writer is waited on");
    assert!(status.success(), "installing {}: {status}", path.display());
}
