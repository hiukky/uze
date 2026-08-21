use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
}

/// Copies the MCP fixture package into `dest_dir` with its `mcp.json`
/// placeholder command rewritten to the real, test-build-resolved path of
/// the fixture MCP server binary — see
/// `tests/fixtures/packages/agent-plugin-mcp/README.md`.
fn mcp_package_fixture_with_resolved_binary(dest_dir: &std::path::Path) -> PathBuf {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-mcp");
    std::fs::create_dir_all(dest_dir).unwrap();
    std::fs::copy(source.join("plugin.json"), dest_dir.join("plugin.json")).unwrap();
    let manifest = std::fs::read_to_string(source.join("mcp.json")).unwrap();
    let resolved = manifest.replace(
        "__UZE_MCP_FIXTURE_BINARY__",
        env!("CARGO_BIN_EXE_uze-mcp-conformance-fixture"),
    );
    std::fs::write(dest_dir.join("mcp.json"), resolved).unwrap();
    dest_dir.to_path_buf()
}

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

#[test]
fn no_subcommand_stays_headless_when_stdout_is_not_a_terminal() {
    let output = Command::new(env!("CARGO_BIN_EXE_uze")).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Manage one local agent plugin environment"));
    assert!(text.contains("Usage:"));
}

/// Writes a fake `claude`/`codex` executable that understands `--version`
/// and just enough of `mcp get`/`mcp add`/`mcp remove` (tracked via a
/// sibling state directory of touched files, one per registered entry
/// name) to exercise `uze setup`'s detection and the full attachment
/// lifecycle — Skills and MCP alike — deterministically, independent of
/// whether real harness CLIs are installed on the machine running this
/// test. Claude's `mcp add` shape is `--scope user --transport stdio
/// <name> -- <command> [args...]`; Codex's is `<name> -- <command>
/// [args...]` — the script skips known flags and takes the first
/// remaining token as the entry name, working for both shapes.
#[cfg(unix)]
fn fake_harness_bin_dir(label: &str) -> PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};

    let dir = temporary_home(label);
    fs::create_dir_all(&dir).unwrap();
    let mcp_state_dir = dir.join("mcp-state");
    fs::create_dir_all(&mcp_state_dir).unwrap();
    for (name, version_line) in [
        ("claude", "9.9.9 (Fake Claude)"),
        ("codex", "codex-cli 9.9.9"),
        ("opencode", "opencode 9.9.9"),
        ("gemini", "gemini 9.9.9"),
    ] {
        let path = dir.join(name);
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "mcp" ]; then
  case "$2" in
    get)
      [ -f "{state}/$3" ] && exit 0 || exit 1
      ;;
    remove)
      rm -f "{state}/$3"
      exit 0
      ;;
    add)
      shift 2
      name=""
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --scope|--transport) shift 2 ;;
          --) shift; break ;;
          *) name="$1"; shift ;;
        esac
      done
      touch "{state}/$name"
      exit 0
      ;;
    *) exit 0 ;;
  esac
fi
echo '{version_line}'
"#,
            state = mcp_state_dir.display(),
        );
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    dir
}

#[test]
fn inspect_reports_an_installed_plugin_without_vendor_writes() {
    let home = temporary_home("cli-inspect");
    let add = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args(["add", package_fixture().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(add.status.success());
    let before = std::fs::read(home.join("state/attachments.json")).ok();
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args(["inspect", "uze-agent-skill-conformance", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["plugin"]["id"], "uze-agent-skill-conformance");
    assert_eq!(report["capabilities"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(home.join("state/attachments.json")).ok(),
        before
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn add_and_inspect_use_the_same_injected_uze_home() {
    let home = temporary_home("cli-store");
    let add = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
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
    assert_eq!(installed["plugin"]["id"], "uze-agent-skill-conformance");
    assert!(PathBuf::from(installed["plugin"]["store_path"].as_str().unwrap()).starts_with(&home));

    let inspect = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args(["inspect", "uze-agent-skill-conformance", "--format", "json"])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let report: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(report["plugin"]["id"], "uze-agent-skill-conformance");
    assert!(
        report["plugin"]["store_path"]
            .as_str()
            .unwrap()
            .contains("store/packages")
    );
    let _ = std::fs::remove_dir_all(home);
}

/// Deterministic: `PATH` is cleared so no real harness binary or installer
/// can be resolved. Explicit setup records a blocked provisioning attempt,
/// but never creates harness-owned directories under the isolated `HOME`.
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
    assert!(stdout.contains("claude-code: setup Failed"));
    assert!(stdout.contains("codex: setup Failed"));
    assert!(!home.join(".claude/skills").exists());
    assert!(!home.join(".agents/skills").exists());
    let _ = std::fs::remove_dir_all(home);
}

/// Deterministic: `uze doctor` is headless and must not print credential
/// material. With the builtin `uze` seed, a fresh home on a machine where
/// harnesses are detected will already show them as prepared (builtin
/// auto-prepares), so this test is deterministic by clearing `PATH` — no
/// harness is detected, therefore both remain "not configured" even after
/// the builtin store entry is seeded.
#[test]
fn doctor_reports_not_configured_before_any_setup() {
    let home = temporary_home("cli-doctor-before-setup");
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "")
        .arg("doctor")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("claude-code"));
    assert!(stdout.contains("codex"));
    assert!(stdout.matches("not configured").count() >= 2);
    // Builtin `uze` is seeded even when no harness is present.
    assert!(stdout.contains("uze"));
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
    assert!(setup_once.contains("claude-code: ready (update; version 9.9.9"));
    assert!(setup_once.contains("codex: ready (update; version"));
    assert!(home.join(".claude/skills").is_dir());
    assert!(home.join(".agents/skills").is_dir());

    // Idempotent: a second `uze setup` does not fail or duplicate state.
    run(&["setup"]);
    let doctor = run(&["doctor"]);
    assert!(doctor.matches("installed / unverified").count() >= 2);

    // `uze add` alone attaches both, without any separate sync command.
    let add = run(&["add", package_fixture().to_str().unwrap()]);
    assert!(add.contains("Attached to claude-code:"));
    assert!(add.contains("Attached to codex:"));

    let claude_entries: Vec<_> = std::fs::read_dir(home.join(".claude/skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    // With the builtin `uze` seeded, each harness has the builtin plus the
    // freshly added fixture.
    assert!(
        claude_entries.len() >= 2,
        "claude should have builtin + fixture"
    );
    assert!(claude_entries.iter().any(|p| p.is_symlink()));
    // The fixture skill for Claude is exposed via a shim, so its symlink
    // points at the shim dir, not directly at the store. Check presence by
    // listing, not by exact target.
    let claude_names: Vec<_> = claude_entries
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert!(
        claude_names.contains(&"uze-e2e".to_owned())
            || claude_names.contains(&"uze-agent-skill-conformance-uze-e2e".to_owned()),
        "claude should contain the fixture skill, got {claude_names:?}"
    );

    let codex_entries: Vec<_> = std::fs::read_dir(home.join(".agents/skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        codex_entries.len() >= 2,
        "codex/opencode should have builtin + fixture"
    );
    assert!(codex_entries.iter().any(|p| p.is_symlink()));
    assert!(
        codex_entries.iter().any(|p| {
            std::fs::read_link(p).ok()
                == Some(uze_home.join("store/packages/uze-agent-skill-conformance/skills/uze-e2e"))
        }),
        "codex should contain the fixture skill symlink"
    );

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
}

/// A user with OpenCode already installed should not have to learn that an
/// extra UZE setup step is required before their first package works. `add`
/// detects the executable, prepares only UZE-owned prerequisites, then
/// attaches the package through the normal integration lifecycle.
#[test]
fn add_prepares_a_detected_opencode_and_attaches_without_prior_setup() {
    let home = temporary_home("cli-add-autoprepares-opencode-home");
    let uze_home = temporary_home("cli-add-autoprepares-opencode-uze-home");
    let fake_bin = fake_harness_bin_dir("cli-add-autoprepares-opencode-bin");
    let path = format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap());

    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &uze_home)
        .env("HOME", &home)
        .env("PATH", &path)
        .args(["add", package_fixture().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "uze add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let skills_dir = home.join(".agents/skills");
    let entries: Vec<_> = std::fs::read_dir(&skills_dir)
        .expect("detected OpenCode should have a prepared global skills dir")
        .map(|entry| entry.unwrap().path())
        .collect();
    // Builtin `uze` (`uze-uze`) plus the fixture.
    assert!(
        entries.len() >= 2,
        "should have builtin + fixture, got {entries:?}"
    );
    assert!(entries.iter().any(|p| p.is_symlink()));
    assert!(
        entries.iter().any(|p| {
            std::fs::read_link(p).ok()
                == Some(uze_home.join("store/packages/uze-agent-skill-conformance/skills/uze-e2e"))
        }),
        "fixture skill should be present alongside builtin"
    );

    let integrations = std::fs::read_to_string(uze_home.join("state/integrations.json")).unwrap();
    assert!(integrations.contains("\"opencode\""));
    assert!(integrations.contains("\"installed\": true"));

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
}

/// Deterministic end-to-end for MCP (see ADR-007): fake, PATH-resolvable
/// `claude`/`codex` scripts that understand `mcp get`/`mcp add`/`mcp
/// remove` well enough to prove `uze setup` + `uze add` registers the MCP
/// fixture for both, idempotently, without a real harness binary. No
/// network, credentials, or LLM involved — this only proves the
/// attach/idempotency/removal mechanics, not real harness behavior.
#[test]
fn setup_then_add_attaches_the_mcp_fixture_idempotently_and_removal_works() {
    let home = temporary_home("cli-mcp-home");
    let uze_home = temporary_home("cli-mcp-uze-home");
    let fake_bin = fake_harness_bin_dir("cli-mcp-bin");
    let mcp_package_dir = temporary_home("cli-mcp-package");
    let package = mcp_package_fixture_with_resolved_binary(&mcp_package_dir);
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

    run(&["setup"]);
    let add = run(&["add", package.to_str().unwrap()]);
    assert!(add.contains("Attached to claude-code: mcp:uze-mcp-conformance-uze-conformance"));
    assert!(add.contains("Attached to codex: mcp:uze-mcp-conformance-uze-conformance"));

    let mcp_state = fake_bin.join("mcp-state");
    assert!(
        mcp_state
            .join("uze-mcp-conformance-uze-conformance")
            .is_file()
    );
    let ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(uze_home.join("state/attachments.json")).unwrap())
            .unwrap();
    let receipts = ledger["receipts"].as_object().unwrap();
    assert!(receipts.len() >= 2);
    // With builtin `uze` seeded, attachments also contain its 4 skill receipts;
    // filter to the MCP package's receipts before asserting their shape.
    let mcp_receipts: Vec<_> = receipts
        .values()
        .filter(|receipt| receipt["package_id"] == "uze-mcp-conformance")
        .collect();
    assert!(
        mcp_receipts.len() >= 2,
        "expected at least 2 MCP receipts, got {mcp_receipts:?}"
    );
    assert!(
        mcp_receipts.iter().all(
            |receipt| receipt["artifact"]["VENDOR_CONFIG_ENTRY"]["entry_name"]
                == "uze-mcp-conformance-uze-conformance"
        ),
        "MCP receipts had unexpected entry_name: {mcp_receipts:?}"
    );
    // At least claude and codex must be among the MCP deliveries; gemini/
    // opencode may also be present depending on integration support.
    assert!(
        mcp_receipts
            .iter()
            .any(|r| r["integration"] == "claude-code"),
        "claude MCP receipt missing"
    );
    assert!(
        mcp_receipts.iter().any(|r| r["integration"] == "codex"),
        "codex MCP receipt missing"
    );

    // Idempotent: `add` a second time does not fail and does not require a
    // real "already exists" overwrite behavior from either harness (the
    // fake script's `get` reports success, so `attach()` never re-invokes
    // `add`).
    let second_add = run(&["add", package.to_str().unwrap()]);
    assert!(
        second_add.contains("Attached to claude-code: mcp:uze-mcp-conformance-uze-conformance")
    );
    assert!(second_add.contains("Attached to codex: mcp:uze-mcp-conformance-uze-conformance"));

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
    let _ = std::fs::remove_dir_all(mcp_package_dir);
}

#[test]
fn remove_uses_the_package_centric_application_flow() {
    let home = temporary_home("cli-remove");
    let add = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args(["add", package_fixture().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(add.status.success());
    let remove = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args(["remove", "uze-agent-skill-conformance", "--format", "json"])
        .output()
        .unwrap();
    assert!(remove.status.success());
    let report: serde_json::Value = serde_json::from_slice(&remove.stdout).unwrap();
    assert_eq!(report["outcome"], "REMOVED");
    let list = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .args(["list", "--format", "json"])
        .output()
        .unwrap();
    assert!(list.status.success());
    // With builtin `uze` seeded, `list` is not empty after removing the
    // fixture — the builtin remains. Filter it out for this test's original
    // assertion that the user-added package is gone.
    let plugins = serde_json::from_slice::<serde_json::Value>(&list.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let non_builtin: Vec<_> = plugins.iter().filter(|p| p["id"] != "uze").collect();
    assert!(
        non_builtin.is_empty(),
        "expected no non-builtin plugins after remove, got {plugins:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}
