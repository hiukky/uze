use std::{path::PathBuf, process::Command};

fn package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("skill-plugin")
}

fn contains_fixture_skill_wrapper(entries: &[PathBuf], uze_home: &std::path::Path) -> bool {
    entries.iter().any(|entry| {
        let Ok(target) = std::fs::read_link(entry) else {
            return false;
        };
        target.starts_with(uze_home.join("state/attachments"))
            && std::fs::read_to_string(target.join("SKILL.md")).is_ok_and(|skill| {
                skill.starts_with("---\nname: uze-agent-skill-conformance:uze-e2e\n")
            })
    })
}

/// `uze plugin install` through a staged test marketplace — the product
/// rejects direct-path installs, so the test exercises the real user flow:
/// `market add` first, then `plugin install <name>@<market>`.
fn install_via_marketplace_json(
    home: &std::path::Path,
    uze_home: &std::path::Path,
    package: &std::path::Path,
    path: &str,
) -> std::process::Output {
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(home, package);
    let base = || {
        Command::new(env!("CARGO_BIN_EXE_uze"))
            .env("UZE_HOME", uze_home)
            .env("HOME", home)
            .env("PATH", path)
            .args(&market_args)
            .output()
    };
    let market_add = base().unwrap();
    assert!(
        market_add.status.success(),
        "market add failed: {}",
        String::from_utf8_lossy(&market_add.stderr)
    );
    let mut with_json = install_args.clone();
    with_json.push("--format".to_owned());
    with_json.push("json".to_owned());
    Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", uze_home)
        .env("HOME", home)
        .env("PATH", path)
        .args(&with_json)
        .output()
        .unwrap()
}

fn install_via_marketplace(
    home: &std::path::Path,
    uze_home: &std::path::Path,
    package: &std::path::Path,
    path: &str,
) -> std::process::Output {
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(home, package);
    let base = || {
        Command::new(env!("CARGO_BIN_EXE_uze"))
            .env("UZE_HOME", uze_home)
            .env("HOME", home)
            .env("PATH", path)
            .args(&market_args)
            .output()
    };
    let market_add = base().unwrap();
    assert!(
        market_add.status.success(),
        "market add failed: {}",
        String::from_utf8_lossy(&market_add.stderr)
    );
    Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", uze_home)
        .env("HOME", home)
        .env("PATH", path)
        .args(&install_args)
        .output()
        .unwrap()
}

/// Copies the MCP fixture package into `dest_dir` with its `mcp.json`
/// placeholder command rewritten to the real, test-build-resolved path of
/// the fixture MCP server binary — see
/// `tests/_fixtures/canonical/mcp-plugin/README.md`.
fn mcp_package_fixture_with_resolved_binary(dest_dir: &std::path::Path) -> PathBuf {
    let source = uze_testkit::fixtures::canonical("mcp-plugin");
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
    uze_testkit::temp::scratch(label)
}

#[test]
fn no_subcommand_stays_headless_when_stdout_is_not_a_terminal() {
    // `UZE_PANE` is set for every process the terminal runtime spawns, and
    // a bare `uze` seeing it opens a space in the running client instead of
    // printing help. Inherited, this test failed for anyone running the
    // suite from inside uze itself — which is how the project dogfoods.
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env_remove("UZE_PANE")
        .output()
        .unwrap();
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
    let command_log = dir.join("commands.log");
    fs::create_dir_all(&mcp_state_dir).unwrap();
    for (name, version_line) in [
        ("claude", "9.9.9 (Fake Claude)"),
        ("codex", "codex-cli 9.9.9"),
        ("opencode", "opencode v9.9.9"),
        ("opencode2", "opencode2 v9.9.9"),
        ("agy", "agy 9.9.9"),
    ] {
        let path = dir.join(name);
        let script = format!(
            r#"#!/bin/sh
echo "$0|$*" >> "{command_log}"
if [ "$1" = "plugin" ]; then
  case "$2" in
    list) echo '{{"imports":[]}}'; exit 0 ;;
    install)
      mkdir -p "$HOME/.gemini/config/plugins"
      staged="$HOME/.gemini/config/plugins/$(basename "$3")"
      cp -R "$3/." "$staged/" 2>/dev/null || true
      # Real `agy` stages the copy under the plugin's own declared
      # manifest name (plugin.json's "name" field), not the source
      # directory's basename (verified against real agy 1.1.22 — see
      # antigravity/plugin.rs's attach_generated_plugin) — a generated
      # envelope's source dir is named after the qualified package id,
      # which differs from the plugin's bare declared name.
      declared=$(sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$staged/plugin.json" 2>/dev/null | head -n 1)
      if [ -n "$declared" ] && [ "$declared" != "$(basename "$3")" ]; then
        rm -rf "$HOME/.gemini/config/plugins/$declared"
        mv "$staged" "$HOME/.gemini/config/plugins/$declared"
      fi
      exit 0
      ;;
    uninstall) exit 0 ;;
  esac
fi
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
            command_log = command_log.display(),
        );
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    dir
}

/// A fake legacy V2-only installation. The isolated PATH deliberately has no
/// stable `opencode`: this proves `uze setup opencode` handles `opencode2`
/// without passing it the stable CLI's incompatible `upgrade` subcommand.
#[cfg(unix)]
fn fake_legacy_opencode_bin_dir(label: &str) -> PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};

    let dir = temporary_home(label);
    fs::create_dir_all(&dir).unwrap();
    let command_log = dir.join("commands.log");
    for (name, script) in [
        (
            "opencode2",
            format!(
                "#!/bin/sh\necho \"$0|$*\" >> \"{}\"\necho 'opencode2 v9.9.9'\n",
                command_log.display()
            ),
        ),
        (
            "sh",
            format!(
                "#!/bin/sh\necho \"$0|$*\" >> \"{}\"\nexit 0\n",
                command_log.display()
            ),
        ),
    ] {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
    dir
}

#[test]
fn inspect_reports_an_installed_plugin_without_vendor_writes() {
    let home = temporary_home("cli-inspect");
    let add = install_via_marketplace(&home, &home, &package_fixture(), "/usr/bin:/bin");
    assert!(add.status.success());
    let before = std::fs::read(home.join("state/attachments.json")).ok();
    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args([
            "plugin",
            "inspect",
            "uze-agent-skill-conformance",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["plugin"]["id"], "uze-agent-skill-conformance@test");
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
    let add = install_via_marketplace_json(&home, &home, &package_fixture(), "/usr/bin:/bin");
    assert!(add.status.success());
    let installed: serde_json::Value = serde_json::from_slice(&add.stdout).unwrap();
    assert_eq!(
        installed["plugin"]["id"],
        "uze-agent-skill-conformance@test"
    );
    assert!(PathBuf::from(installed["plugin"]["store_path"].as_str().unwrap()).starts_with(&home));

    let inspect = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args([
            "plugin",
            "inspect",
            "uze-agent-skill-conformance",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(inspect.status.success());
    let report: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(report["plugin"]["id"], "uze-agent-skill-conformance@test");
    assert!(
        report["plugin"]["store_path"]
            .as_str()
            .unwrap()
            .contains("store/plugins"),
        "store_path should live under the Store's plugins dir, got {}",
        report["plugin"]["store_path"]
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
/// material. With the default `uze` seed, a fresh home on a machine where
/// harnesses are detected will already show them as prepared (the default
/// plugin auto-prepares), so this test is deterministic by clearing `PATH` — no
/// harness is detected, therefore both remain "not configured" even after
/// the default plugin's store entry is seeded.
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
    assert!(stdout.contains("Claude Code"));
    assert!(stdout.contains("Codex"));
    assert!(stdout.matches("not configured").count() >= 2);
    // Default `uze` is seeded even when no harness is present.
    assert!(stdout.contains("uze"));
    let _ = std::fs::remove_dir_all(home);
}

/// L2 setup conformance: this list is intentionally compared to the product
/// registry below. Registering another harness therefore requires an explicit
/// setup scenario here rather than silently inheriting partial coverage.
#[cfg(unix)]
const SETUP_CONFORMANCE_HARNESSES: [(&str, &str, &str); 4] = [
    ("claude-code", "claude", "update"),
    ("codex", "codex", "update"),
    ("opencode", "opencode", "upgrade"),
    ("antigravity", "agy", "update"),
];

/// Every registered integration gets a deterministic `uze setup <harness>`
/// command-level contract. The fake executables record their own invocations,
/// proving the update reaches a real vendor binary on PATH rather than a UZE
/// shim, while keeping installer/network behavior out of `cargo test`.
#[test]
#[cfg(unix)]
fn setup_conformance_matrix_covers_every_registered_harness() {
    use uze_core::home::UzeHome;
    use uze_integrations::registry::IntegrationRegistry;

    let registry_root = temporary_home("cli-setup-conformance-registry");
    let registry_home = UzeHome::at(registry_root.join("uze"));
    let registry = IntegrationRegistry::isolated(&registry_root, &registry_home);
    let mut registered = registry.ids();
    registered.sort_unstable();
    let mut covered = SETUP_CONFORMANCE_HARNESSES
        .iter()
        .map(|(id, _, _)| *id)
        .collect::<Vec<_>>();
    covered.sort_unstable();
    assert_eq!(
        covered, registered,
        "every registered harness needs setup conformance"
    );

    let home = temporary_home("cli-setup-conformance-home");
    let uze_home = temporary_home("cli-setup-conformance-uze-home");
    let fake_bin = fake_harness_bin_dir("cli-setup-conformance-bin");
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    for (harness, executable, update_command) in SETUP_CONFORMANCE_HARNESSES {
        let output = Command::new(env!("CARGO_BIN_EXE_uze"))
            .env("UZE_HOME", &uze_home)
            .env("HOME", &home)
            .env("PATH", &path)
            .args(["setup", harness])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "uze setup {harness} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("{harness}: ready (update;")),
            "unexpected setup output for {harness}: {stdout}"
        );
        assert!(
            uze_home.join("shims").join(executable).is_symlink(),
            "uze setup {harness} must create its default {executable} shim"
        );
        let commands = std::fs::read_to_string(fake_bin.join("commands.log")).unwrap();
        assert!(
            commands
                .lines()
                .any(|line| line.ends_with(&format!("/{executable}|{update_command}"))),
            "{harness} did not use the documented {update_command} route through the resolved real {executable} binary: {commands}"
        );
    }

    let _ = std::fs::remove_dir_all(registry_root);
    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
}

#[test]
#[cfg(unix)]
fn setup_opencode_legacy_binary_uses_installer_not_stable_upgrade() {
    let home = temporary_home("cli-setup-opencode2-home");
    let uze_home = temporary_home("cli-setup-opencode2-uze-home");
    let fake_bin = fake_legacy_opencode_bin_dir("cli-setup-opencode2-bin");
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    let output = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &uze_home)
        .env("HOME", &home)
        .env("PATH", &path)
        .args(["setup", "opencode"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "uze setup opencode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = std::fs::read_to_string(fake_bin.join("commands.log")).unwrap();
    assert!(commands.contains("opencode.ai/v2/install"), "{commands}");
    assert!(commands.contains("opencode2|--version"), "{commands}");
    assert!(
        !commands.contains("opencode2|upgrade"),
        "legacy OpenCode must not receive stable-only `upgrade`: {commands}"
    );

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
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
    // Both fake harnesses' provisioning reported `Verified` above ("ready
    // (update; version ...)"), and `status()` reflects that once recorded —
    // see `IntegrationPort::status`'s doc comment.
    assert!(doctor.matches("installed / verified").count() >= 2);

    // `uze plugin install` alone attaches both, without any separate sync
    // command.
    //
    // Both the default `uze` package and this single-skill fixture qualify
    // for Generated Native Package (ADR-013: no explicit `.claude-plugin/
    // plugin.json`, but a conventional `skills/` directory UZE can safely
    // represent) — so Claude receives package-level delivery, not a
    // per-resource `.claude/skills` symlink. Codex has no envelope for
    // either package and still decomposes at the capability level, so its
    // resource-level attachment output is unchanged.
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(&home, &package_fixture());
    run(&market_args.iter().map(String::as_str).collect::<Vec<_>>());
    let add = run(&install_args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(add.contains("Claude Code: native"));
    assert!(add.contains("Codex: native"));

    // `.claude/skills` is prepared (by `install`) but stays empty: neither
    // package decomposes into it anymore. The generated envelope directory
    // is where delivery now lives, one subdirectory per generatable
    // package, each independently rebuildable from the Store.
    let claude_skills_entries: Vec<_> = std::fs::read_dir(home.join(".claude/skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        claude_skills_entries.is_empty(),
        "no package should decompose into .claude/skills once generatable, got {claude_skills_entries:?}"
    );
    let generated_root = uze_home.join("state/attachments/claude/generated");
    assert!(
        generated_root
            .join("uze-agent-skill-conformance@test/.claude-plugin/plugin.json")
            .is_file(),
        "the fixture's generated Claude envelope should exist"
    );
    // Default-policy skills stay byte-preserving per-skill symlinks inside
    // the envelope's own `skills/` directory (ADR-030).
    assert!(
        generated_root
            .join("uze-agent-skill-conformance@test/skills/uze-e2e")
            .is_symlink(),
        "the generated envelope should reference the Store's skills/ by symlink, not a copy"
    );
    assert!(
        generated_root
            .join("uze@uze-official/.claude-plugin/plugin.json")
            .is_file(),
        "the default uze package's generated Claude envelope should exist too"
    );

    let codex_entries: Vec<_> = std::fs::read_dir(home.join(".agents/skills"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        codex_entries.len() >= 2,
        "codex/opencode should have default + fixture"
    );
    assert!(codex_entries.iter().any(|p| p.is_symlink()));
    assert!(
        contains_fixture_skill_wrapper(&codex_entries, &uze_home),
        "codex should contain the qualified fixture skill wrapper"
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

    let output = install_via_marketplace(&home, &uze_home, &package_fixture(), &path);
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
    // Default `uze` (`uze-uze`) plus the fixture.
    assert!(
        entries.len() >= 2,
        "should have default + fixture, got {entries:?}"
    );
    assert!(entries.iter().any(|p| p.is_symlink()));
    assert!(
        contains_fixture_skill_wrapper(&entries, &uze_home),
        "the qualified fixture skill wrapper should be present alongside the default plugin"
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
    // This fixture (`agent-plugin-mcp`) has only `mcp.json`, no `skills/`
    // and no `.claude-plugin/plugin.json`/`.codex-plugin/plugin.json` —
    // under Generated Native Package (ADR-013) an MCP-only package
    // is just as eligible as a Skill-only one, so BOTH Claude and Codex now
    // receive package-level delivery covering the one MCP resource — no
    // resource-level `mcp add` for either. Opencode has no package envelope
    // and stays resource-level via native `opencode mcp add` (now 🟢 Native).
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(&home, &package);
    run(&market_args.iter().map(String::as_str).collect::<Vec<_>>());
    let add = run(&install_args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(add.contains("Claude Code: native"));
    assert!(add.contains("Codex: native"));

    // Opencode is resource-level native (no package envelope) via
    // `opencode mcp add`; when the fake opencode binary is on PATH it
    // creates a mcp-state marker, when running under llvm-cov with direct
    // file fallback no marker is created — receipt is the source of truth.
    let _mcp_state = fake_bin.join("mcp-state");
    let ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(uze_home.join("state/attachments.json")).unwrap())
            .unwrap();
    let receipts = ledger["receipts"].as_object().unwrap();
    assert!(receipts.len() >= 2);
    // With the default `uze` seeded, attachments also contain its own
    // package-level receipts (the default package is generatable too, see
    // the skill-fixture CLI test); filter to this MCP package's receipts
    // before asserting their shape.
    let mcp_receipts: Vec<_> = receipts
        .values()
        .filter(|receipt| receipt["package_id"] == "uze-mcp-conformance@test")
        .collect();
    assert!(
        mcp_receipts.len() >= 3,
        "expected at least 3 receipts for the MCP package (claude + codex package-level + opencode resource-level), got {mcp_receipts:?}"
    );
    let opencode_receipt = mcp_receipts
        .iter()
        .find(|r| r["integration"] == "opencode")
        .expect("opencode receipt missing");
    assert!(
        opencode_receipt["artifact"]
            .get("VENDOR_CONFIG_ENTRY")
            .is_some()
            || opencode_receipt["artifact"]
                .get("VendorConfigEntry")
                .is_some(),
        "opencode's receipt for the MCP package must be resource-level VendorConfigEntry (no package), got {opencode_receipt:?}"
    );
    let claude_receipt = mcp_receipts
        .iter()
        .find(|r| r["integration"] == "claude-code")
        .expect("claude receipt missing");
    assert_eq!(
        claude_receipt["artifact"]["INTEGRATION_OWNED"]["kind"], "claude-plugin-generated",
        "claude's receipt for an envelope-less MCP-only package must be the generated package kind, not a resource-level VendorConfigEntry: {claude_receipt:?}"
    );
    assert_eq!(
        claude_receipt["artifact"]["INTEGRATION_OWNED"]["origin"],
        "generated"
    );
    let codex_receipt = mcp_receipts
        .iter()
        .find(|r| r["integration"] == "codex")
        .expect("codex receipt missing");
    assert_eq!(
        codex_receipt["artifact"]["INTEGRATION_OWNED"]["kind"], "marketplace-plugin-generated",
        "codex's receipt for an envelope-less MCP-only package must be the generated package kind, not a resource-level VendorConfigEntry: {codex_receipt:?}"
    );
    assert_eq!(
        codex_receipt["artifact"]["INTEGRATION_OWNED"]["origin"],
        "generated"
    );

    // Idempotent: `plugin install` a second time does not fail. Both
    // integrations' package delivery re-resolves to the same
    // already-installed selector — no reinstall, no resource-level replay.
    let second_add = run(&install_args.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(second_add.contains("Claude Code: native"));
    assert!(second_add.contains("Codex: native"));

    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(uze_home);
    let _ = std::fs::remove_dir_all(fake_bin);
    let _ = std::fs::remove_dir_all(mcp_package_dir);
}

/// `uze plugin remove` — the machine-level verb (renamed from the old,
/// unnamespaced root `remove`, which used to reach this exact flow via an
/// implicit fallback — see `plugin_remove_never_confused_with_project_remove`
/// / ADR-019 for why that fallback is gone).
#[test]
fn plugin_remove_uses_the_package_centric_application_flow() {
    let home = temporary_home("cli-remove");
    let add = install_via_marketplace(&home, &home, &package_fixture(), "/usr/bin:/bin");
    assert!(add.status.success());
    let remove = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .args([
            "plugin",
            "remove",
            "uze-agent-skill-conformance",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(remove.status.success());
    let report: serde_json::Value = serde_json::from_slice(&remove.stdout).unwrap();
    assert_eq!(report["outcome"], "REMOVED");
    let list = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .args(["plugin", "list", "--format", "json"])
        .output()
        .unwrap();
    assert!(list.status.success());
    // With the default `uze` seeded, `list` is not empty after removing the
    // fixture — the default plugin remains. Filter it out for this test's
    // original assertion that the user-added package is gone.
    let plugins = serde_json::from_slice::<serde_json::Value>(&list.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let non_default: Vec<_> = plugins
        .iter()
        .filter(|p| p["id"] != "uze@uze-official")
        .collect();
    assert!(
        non_default.is_empty(),
        "expected no non-default plugins after remove, got {plugins:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// ADR-019's central breaking change: root `uze remove` is strictly
/// project-scoped. Run with no `agents.lock` anywhere in `home`'s ancestry
/// (a plain temp dir, not a project), this must now fail loudly — never
/// silently fall through to removing the machine-installed package, the
/// pre-ADR-019 behavior `plugin_remove_uses_the_package_centric_application_flow`
/// exercises deliberately instead.
#[test]
fn root_remove_no_longer_falls_back_to_global_removal() {
    let home = temporary_home("cli-remove-no-fallback");
    let add = install_via_marketplace(&home, &home, &package_fixture(), "/usr/bin:/bin");
    assert!(add.status.success());

    // `current_dir(&home)` matters here: this repo's own root (the ambient
    // cwd `cargo test` runs from) has a real `agents.lock` of its own
    // (UZE dogfoods itself) — running from there would resolve a *real*
    // project root and hit `NotInLock` instead of the `NoLock` case this
    // test targets. `home` is a fresh temp dir with no `agents.lock`,
    // `AGENTS.md`, or `.git` anywhere in its ancestry.
    let remove = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(&home)
        .args(["remove", "uze-agent-skill-conformance"])
        .output()
        .unwrap();
    assert!(
        !remove.status.success(),
        "root `remove` outside a project must fail, not silently succeed"
    );
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(
        stderr.contains("no project environment found"),
        "expected a no-project-environment error, got: {stderr}"
    );
    assert!(
        stderr.contains("uze plugin remove"),
        "error should point at the machine-level equivalent, got: {stderr}"
    );

    // The whole point: the machine-installed package must survive untouched.
    let list = Command::new(env!("CARGO_BIN_EXE_uze"))
        .env("UZE_HOME", &home)
        .args(["plugin", "list", "--format", "json"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let plugins = serde_json::from_slice::<serde_json::Value>(&list.stdout)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    assert!(
        plugins
            .iter()
            .any(|p| p["id"] == "uze-agent-skill-conformance@test"),
        "package must survive a failed project-scoped remove, got {plugins:?}"
    );
    let _ = std::fs::remove_dir_all(home);
}
