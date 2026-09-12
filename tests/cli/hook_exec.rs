//! `uze hook-exec` as a harness actually invokes it (ADR-033): the payload
//! on stdin, the group's declaration on the command line, and an answer the
//! harness reads as a decision.
//!
//! Both claims here are about what happens when things go wrong, because
//! that is where a hook contract is either kept or quietly abandoned: a
//! `deny` group must still deny, and a handler must still be bounded by the
//! timeout its author wrote.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// The block signal Claude, Codex and Antigravity document for a tool hook.
/// Exit 1 is *not* it: those harnesses log a non-zero-but-not-2 exit and
/// run the tool anyway.
const BLOCKING_EXIT: i32 = 2;

struct Answer {
    exit: i32,
    stdout: String,
    stderr: String,
    elapsed: Duration,
}

fn world(label: &str) -> (PathBuf, PathBuf) {
    let root = uze_testkit::temp::scratch(label);
    let home = root.join("home");
    let uze_home = root.join("uze");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&uze_home).unwrap();
    (home, uze_home)
}

fn hook_exec(home: &Path, uze_home: &Path, arguments: &[&str], payload: &str) -> Answer {
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("HOME", home)
        .env("UZE_HOME", uze_home)
        .arg("hook-exec")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the uze binary starts");
    // A run that answers before reading stdin has closed the pipe already;
    // that is an answer, not a test failure.
    let _ = child.stdin.take().unwrap().write_all(payload.as_bytes());
    let output = child.wait_with_output().unwrap();
    Answer {
        exit: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        elapsed: started.elapsed(),
    }
}

fn claude_payload(command: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "cwd": "/repo",
    })
    .to_string()
}

/// A payload the adapter cannot read is the failure mode a vendor reaches
/// by changing its own shape — and every safety hook in the world is a
/// `deny` group. It must block and say why, never exit 1 (which the
/// harness reads as "logged and ignored, execution continues").
#[test]
fn a_deny_group_blocks_when_the_payload_cannot_be_read() {
    let (home, uze_home) = world("hook-exec-deny");
    let answer = hook_exec(
        &home,
        &uze_home,
        &[
            "--adapter",
            "claude-code",
            "--event",
            "pre_tool_use",
            "--effect",
            "deny",
            "--plugin-root",
            home.to_str().unwrap(),
            "--command",
            "true",
        ],
        "this is not JSON",
    );

    assert_eq!(
        answer.exit, BLOCKING_EXIT,
        "a deny group that cannot be evaluated blocks: {}",
        answer.stderr
    );
    assert!(
        answer.stderr.contains("not JSON"),
        "the reason reaches the channel the harness feeds back: {}",
        answer.stderr
    );
    let document: serde_json::Value = serde_json::from_str(answer.stdout.trim())
        .expect("a blocking harness reads the decision as a document");
    assert_eq!(
        document["hookSpecificOutput"]["permissionDecision"], "deny",
        "{document}"
    );
}

/// The same failure on an observational group is diagnostic, not a block:
/// its purpose is to watch, so it reports and lets the tool run.
#[test]
fn an_observe_group_proceeds_when_the_payload_cannot_be_read() {
    let (home, uze_home) = world("hook-exec-observe");
    let answer = hook_exec(
        &home,
        &uze_home,
        &[
            "--adapter",
            "claude-code",
            "--event",
            "pre_tool_use",
            "--effect",
            "observe",
            "--plugin-root",
            home.to_str().unwrap(),
            "--command",
            "true",
        ],
        "this is not JSON",
    );

    assert_eq!(answer.exit, 0, "an observational group fails open");
    assert!(answer.stderr.contains("not JSON"), "{}", answer.stderr);
}

/// The timeout the author declared is the one the handler gets. It used to
/// be validated, shown at trust time, and then dropped on the way to the
/// runner, which gave every handler 30 seconds regardless.
#[test]
fn a_handler_is_bounded_by_the_timeout_its_author_declared() {
    let (home, uze_home) = world("hook-exec-timeout");
    let answer = hook_exec(
        &home,
        &uze_home,
        &[
            "--adapter",
            "claude-code",
            "--event",
            "pre_tool_use",
            "--effect",
            "deny",
            "--plugin-root",
            home.to_str().unwrap(),
            "--command",
            "sleep 30",
            "--timeout",
            "2",
        ],
        &claude_payload("ls"),
    );

    assert!(
        answer.elapsed < Duration::from_secs(15),
        "the declared 2s bound decided when to stop waiting, not the 30s default: {:?}",
        answer.elapsed
    );
    assert_eq!(
        answer.exit, BLOCKING_EXIT,
        "a deny group whose handler timed out blocks: {}",
        answer.stderr
    );
    assert!(
        answer.stderr.contains("timed out after 2s"),
        "the reason names the author's own bound: {}",
        answer.stderr
    );
}
