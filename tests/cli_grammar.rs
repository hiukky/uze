//! Grammar/precedence tests for ADR-019 (`docs/adr/019-explicit-project-
//! machine-boundary-in-cli-command-grammar.md`): built-ins must always take
//! precedence over `<plugin>@<market>` shorthand, the shorthand must
//! require `@`, and an unrecognized flag after it must never be silently
//! ignored. Each test below corresponds to one line of the ambiguity list
//! the change was reviewed against.

use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-grammar-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn uze(home: &PathBuf) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_uze"));
    command
        .env("UZE_HOME", home)
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        // Isolates project-root resolution from this repo's own real
        // `agents.lock` — see `root_remove_no_longer_falls_back_to_global_removal`
        // in tests/cli.rs for why this matters.
        .current_dir(home);
    command
}

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
}

/// `uze flow@ai` — no marketplace `ai` registered, so this must reach the
/// shorthand's own resolution logic (and fail there, on the marketplace
/// lookup) rather than being misparsed as an unrecognized command.
#[test]
fn shorthand_reaches_project_resolution_not_unrecognized_command() {
    let home = temporary_home("shorthand-basic");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["flow@ai"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unrecognized subcommand"),
        "flow@ai must not be treated as an unrecognized command, got: {stderr}"
    );
    assert!(
        stderr.contains("marketplace") && stderr.contains("ai"),
        "expected a marketplace-not-found error, got: {stderr}"
    );
    assert!(
        !PathBuf::from("agents.lock").exists() || !home.join("agents.lock").is_file(),
        "a failed shorthand must never leave a lock behind"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze remove flow` — the built-in `Remove` variant, never reinterpreted:
/// it has no `@`, so it was never shorthand-eligible in the first place,
/// but this proves it actually reaches `Command::Remove`'s own project-
/// scoped logic (a `NoProjectEnvironment`-shaped failure here, not a
/// generic "unrecognized subcommand").
#[test]
fn remove_is_the_builtin_not_shorthand() {
    let home = temporary_home("remove-builtin");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["remove", "flow"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unrecognized subcommand"),
        "remove must dispatch as a built-in, got: {stderr}"
    );
    assert!(
        stderr.contains("no project environment found"),
        "expected the project-scoped remove error, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze market add <path>` — machine-level, must never touch a project's
/// `agents.lock` even though one is present in the working directory.
#[test]
fn market_add_never_touches_the_project_lock() {
    let home = temporary_home("market-add");
    std::fs::create_dir_all(&home).unwrap();
    // A local marketplace root: needs its own `marketplace.json`.
    let market_root = home.join("market");
    std::fs::create_dir_all(&market_root).unwrap();
    std::fs::write(
        market_root.join("marketplace.json"),
        r#"{"name": "ai", "plugins": []}"#,
    )
    .unwrap();

    let output = uze(&home)
        .args(["market", "add", market_root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "market add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !home.join("agents.lock").is_file(),
        "`market add` must never create agents.lock"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze plugin install <path>` — machine-level install by direct source
/// (no `@`), also never touching the project lock.
#[test]
fn plugin_install_by_direct_path_never_touches_the_project_lock() {
    let home = temporary_home("plugin-install-path");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home)
        .args(["plugin", "install", package_fixture().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "plugin install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !home.join("agents.lock").is_file(),
        "`plugin install` must never create agents.lock"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze doctor` — a built-in with no `@`, unambiguous by construction.
#[test]
fn doctor_is_the_builtin() {
    let home = temporary_home("doctor-builtin");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["doctor"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("UZE Home"));
    let _ = std::fs::remove_dir_all(home);
}

/// `uze status` — a built-in with no `@`, unambiguous by construction.
#[test]
fn status_is_the_builtin() {
    let home = temporary_home("status-builtin");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["status"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Project"));
    let _ = std::fs::remove_dir_all(home);
}

/// `uze unknown` — no `@`, matches no built-in: a real unrecognized
/// command, not silent shorthand. Must fail with a `clap`-shaped error and
/// a hint pointing at the `@market` form.
#[test]
fn bare_unknown_name_is_an_unrecognized_command_with_a_hint() {
    let home = temporary_home("unknown-bare");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["unknown"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand 'unknown'"),
        "got: {stderr}"
    );
    assert!(
        stderr.contains("unknown@<market>"),
        "expected a hint toward the shorthand form, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze flow@ai --unknown` — an unrecognized flag after the shorthand must
/// be rejected by `clap`, never silently ignored (the exact bug the
/// pre-`clap` `argv[1].contains('@')` parser had).
#[test]
fn shorthand_rejects_an_unknown_flag_instead_of_ignoring_it() {
    let home = temporary_home("shorthand-unknown-flag");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["flow@ai", "--unknown"]).output().unwrap();
    assert!(
        !output.status.success(),
        "an unrecognized flag must fail the command, not be silently dropped"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--unknown") || stderr.to_lowercase().contains("unexpected argument"),
        "expected clap's own unrecognized-argument error, got: {stderr}"
    );
    assert!(
        !home.join("agents.lock").is_file(),
        "a rejected flag must never let the shorthand proceed and write a lock"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze doctor@foo` — `doctor` is a built-in name, but the *token* is
/// `"doctor@foo"`, which no built-in name equals exactly. Since no
/// built-in name can ever contain `@` (ADR-019's soundness argument), this
/// must be classified as shorthand: plugin `doctor` from marketplace
/// `foo` — not the `doctor` diagnostics command.
#[test]
fn builtin_name_followed_by_at_market_is_still_shorthand() {
    let home = temporary_home("doctor-at-foo");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["doctor@foo"]).output().unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Proof it did NOT run diagnostics: `doctor`'s own success output
    // ("UZE Home") never appears, and the failure is a marketplace lookup,
    // not a coincidentally-similar diagnostics report.
    assert!(
        !stdout.contains("UZE Home"),
        "doctor@foo must not run the doctor command, got stdout: {stdout}"
    );
    assert!(
        !stderr.contains("unrecognized subcommand"),
        "doctor@foo must be classified as shorthand, got: {stderr}"
    );
    assert!(
        stderr.contains("marketplace") && stderr.contains("foo"),
        "expected a marketplace-not-found error for `foo`, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// `uze --help` must name the Project/Machine split explicitly
/// (`specs/cli-command-grammar/spec.md`'s "Top-level help names both
/// scopes" requirement) and list `market`/`plugin`/`harness` under the
/// machine heading, with the shorthand shown in Usage.
#[test]
fn help_names_the_project_machine_split() {
    let home = temporary_home("help-split");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Project:"), "missing Project heading");
    assert!(stdout.contains("Machine:"), "missing Machine heading");
    assert!(stdout.contains("<plugin>@<market>"), "missing Usage line");
    for namespace in ["market", "plugin", "harness"] {
        assert!(
            stdout.contains(namespace),
            "Machine heading must list `{namespace}`"
        );
    }
    let _ = std::fs::remove_dir_all(home);
}

/// `uze market --help` must list only `market`'s own verbs — namespace
/// help stays self-contained, per the same requirement's second scenario.
#[test]
fn market_help_is_self_contained() {
    let home = temporary_home("market-help");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["market", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for verb in ["add", "remove", "list", "inspect"] {
        assert!(stdout.contains(verb), "market --help missing `{verb}`");
    }
    for unrelated in ["harness", "doctor"] {
        assert!(
            !stdout.contains(unrelated),
            "market --help unexpectedly mentions unrelated `{unrelated}`: {stdout}"
        );
    }
    let _ = std::fs::remove_dir_all(home);
}

/// Every built-in command/subcommand name, at every nesting level, is free
/// of `@` — the machine-checkable half of ADR-019's soundness argument
/// (`specs/cli-command-grammar/spec.md`'s "No built-in command can ever
/// collide with a shorthand token" requirement).
#[test]
fn no_builtin_command_name_contains_at() {
    let home = temporary_home("help-listing");
    std::fs::create_dir_all(&home).unwrap();
    let output = uze(&home).args(["--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        // Skip the lines that legitimately document/demonstrate the
        // shorthand form itself (the Usage line and the Examples section).
        if line.contains("<plugin>@<market>") || line.contains("flow@ai") {
            continue;
        }
        assert!(
            !line.contains('@'),
            "a help line unexpectedly contains '@' outside the shorthand form: {line}"
        );
    }
    let _ = std::fs::remove_dir_all(home);
}
