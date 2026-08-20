use std::{path::PathBuf, process::Command};

fn fixture_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/portable-project")
}

#[test]
fn inspect_reports_project_resources_and_does_not_write_vendor_state() {
    let root = fixture_project();
    let hook = root.join(".claude/hooks/pre-commit.sh");
    let before = std::fs::read(&hook).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .args(["inspect", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["project_resources"].as_array().unwrap().len(), 3);
    assert!(report["integrations"]["claude-code"].is_object());
    assert!(report["integrations"]["codex"].is_object());
    assert!(report["integrations"]["opencode"].is_object());
    assert_eq!(std::fs::read(&hook).unwrap(), before);
}
