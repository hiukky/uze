use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projects/portable-project")
}

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
}

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

/// Writes a fake `claude`/`codex` executable that only understands
/// `--version`. Used to exercise `uze setup`'s detection and attachment
/// lifecycle deterministically, independent of whether real harness CLIs
/// are installed on the machine running this test.
#[cfg(unix)]
fn fake_harness_bin_dir(label: &str) -> PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};

    let dir = temporary_home(label);
    fs::create_dir_all(&dir).unwrap();
    for (name, version_line) in [
        ("claude", "9.9.9 (Fake Claude)"),
        ("codex", "codex-cli 9.9.9"),
    ] {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\necho '{version_line}'\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    dir
}

#[test]
fn inspect_reports_project_resources_and_does_not_write_vendor_state() {
    let root = fixture_project();
    let hook = root.join(".claude/hooks/pre-commit.sh");
    let before = std::fs::read(&hook).unwrap();

    let home = temporary_home("cli-inspect");
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .args(["inspect", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["effective_resources"].as_array().unwrap().len(), 3);
    assert!(report["integrations"]["claude-code"].is_object());
    assert!(report["integrations"]["codex"].is_object());
    assert!(report["integrations"]["opencode"].is_object());
    assert_eq!(std::fs::read(&hook).unwrap(), before);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn add_and_inspect_use_the_same_injected_uze_home() {
    let home = temporary_home("cli-store");
    let add = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .args([
            "add",
            package_fixture().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(add.status.success());
    let installed: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    assert_eq!(installed["package_id"], "uze-agent-skill-conformance");
    assert!(PathBuf::from(installed["store_path"].as_str().unwrap()).starts_with(&home));

    let inspect = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .args([
            "inspect",
            fixture_project().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let report: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(report["effective_resources"].as_array().unwrap().len(), 4);
    assert!(
        report["effective_resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|resource| resource["path"]
                .as_str()
                .unwrap()
                .contains("store/packages"))
    );
    let _ = std::fs::remove_dir_all(home);
}

/// Deterministic: `PATH` is cleared so no real `claude`/`codex` binary can
/// be resolved, regardless of what is installed on the machine running this
/// test. `uze setup` must report both harnesses as not detected and must
/// not write anything under the isolated `HOME`.
#[test]
fn setup_reports_absent_harnesses_without_failing_or_writing_state() {
    let home = temporary_home("cli-setup-absent");
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "")
        .arg("setup")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("claude-code: not detected, skipping setup"));
    assert!(stdout.contains("codex: not detected, skipping setup"));
    assert!(!home.join(".claude/skills").exists());
    assert!(!home.join(".agents/skills").exists());
    let _ = std::fs::remove_dir_all(home);
}

/// Deterministic: no setup has run, so `uze doctor` must report both
/// harnesses as not configured without printing any credential material.
#[test]
fn doctor_reports_not_configured_before_any_setup() {
    let home = temporary_home("cli-doctor-before-setup");
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .arg("doctor")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("claude-code"));
    assert!(stdout.contains("codex"));
    assert!(stdout.matches("not configured").count() >= 2);
    let _ = std::fs::remove_dir_all(home);
}

/// Deterministic end-to-end: `uze setup` against fake, PATH-resolvable
/// `claude`/`codex` executables (so no real harness install is required to
/// run this test), then `uze add` alone attaching the shared fixture skill
/// for both — matching the target `uze setup` / `uze add` / plain harness
/// invocation experience, minus the real invocation itself. Setup running
/// twice must not duplicate recorded state or managed artifacts.
#[test]
fn setup_then_add_attaches_transparently_without_a_separate_sync_step() {
    let home = temporary_home("cli-setup-then-add-home");
    let uze_home = temporary_home("cli-setup-then-add-uze-home");
    let fake_bin = fake_harness_bin_dir("cli-setup-then-add-bin");
    let path = format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap());

    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_uze"))
            .env("UZE_HOME", &uze_home)
            .env("HOME", &home)
            .env("PATH", &path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "uze {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };

    let setup_once = run(&["setup"]);
    assert!(setup_once.contains("claude-code: ready (version 9.9.9"));
    assert!(setup_once.contains("codex: ready (version"));
    assert!(home.join(".claude/skills").is_dir());
    assert!(home.join(".agents/skills").is_dir());

    // Idempotent: a second `uze setup` does not fail or duplicate state.
    run(&["setup"]);
    let doctor = run(&["doctor"]);
    assert_eq!(doctor.matches("installed / unverified").count(), 2);

    // `uze add` alone attaches both, without any separate sync command.
    let add = run(&["add", package_fixture().to_str().unwrap()]);
    assert!(add.contains("Attached to claude-code:"));
    assert!(add.contains("Attached to codex:"));

    let claude_entries: Vec<_> = std::fs::read_dir(home.join(".claude/skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(claude_entries.len(), 1);
    assert!(claude_entries[0].is_symlink());

    let codex_entries: Vec<_> = std::fs::read_dir(home.join(".agents/skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(codex_entries.len(), 1);
    assert!(codex_entries[0].is_symlink());
    assert_eq!(
        std::fs::read_link(&codex_entries[0]).unwrap(),
        uze_home.join("store/packages/uze-agent-skill-conformance/skills/uze-e2e")
    );

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
}
