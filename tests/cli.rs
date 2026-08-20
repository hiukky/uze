use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/portable-project")
}

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-plugin-package")
}

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
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
