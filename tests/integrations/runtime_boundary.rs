//! Structural contract test for the runtime-shim-boundary invariant (spec
//! §13, ADR-014): "UZE runtime shims are USER ENTRY boundaries. Internal
//! integration code must invoke upstream harness binaries directly."
//!
//! A bare `Command::new("claude"|"codex"|"opencode"|"agy")` PATH lookup
//! from inside `uze-integrations` risks recursing into UZE's own
//! `~/.uze/shims/<vendor>` once `uze setup <harness>` has ever run (it can
//! sit ahead of the real binary on `$PATH`) — the exact bug this milestone
//! generalized the fix for, from Claude alone to Codex too, and
//! caught a real regression of by re-checking here. This test scans every
//! `.rs` source file under `crates/uze-integrations/src/` and fails if any
//! non-comment line spawns a vendor binary by its bare literal name instead
//! of through the resolved `provisioning_executable()`/
//! `resolve_real_executable` pattern every integration in this crate uses.
//!
//! The same scan is applied to `tests/` itself
//! (`no_deterministic_test_spawns_a_bare_vendor_executable`): a test that
//! spawns a vendor by bare name measures the developer's `PATH`, not the
//! vendor.
//!
//! Deliberately source-level, not behavioral: a behavioral test would need
//! a real or faked shim on `PATH` to prove recursion, which is exactly the
//! kind of environment-dependent setup this invariant must hold regardless
//! of. Scanning for the literal call shape is a direct, cheap proxy for "no
//! internal call site can possibly re-enter the shim," and it fails loudly
//! and specifically (file + line) the moment someone reintroduces one.

use std::{fs, path::Path};

use uze_core::UzeHome;
use uze_core::integration::IntegrationPort;
use uze_integrations::antigravity::AntigravityIntegration;
use uze_integrations::claude::ClaudeIntegration;
use uze_integrations::codex::CodexIntegration;
use uze_integrations::opencode::OpenCodeIntegration;

const VENDOR_NAMES: [&str; 4] = ["claude", "codex", "opencode", "agy"];

fn integrations_src_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-integrations/src")
}

fn deterministic_tests_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_internal_integration_call_site_spawns_a_bare_vendor_executable() {
    let mut files = Vec::new();
    rust_files(&integrations_src_dir(), &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .rs files under crates/uze-integrations/src"
    );

    let mut violations = Vec::new();
    for path in &files {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for (line_number, line) in contents.lines().enumerate() {
            let trimmed = line.trim_start();
            // Doc comments and regular comments are prose describing this
            // exact invariant (e.g. "rather than a bare Command::new(...)")
            // — only live code can actually spawn a process.
            if trimmed.starts_with("//") {
                continue;
            }
            for vendor in VENDOR_NAMES {
                let needle = format!("Command::new(\"{vendor}\")");
                if line.contains(&needle) {
                    violations.push(format!("{}:{}: {needle}", path.display(), line_number + 1));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "found bare Command::new(<vendor>) call site(s) that bypass the resolved-executable \
         pattern (provisioning_executable()/resolve_real_executable) and could recurse into \
         UZE's own runtime shim:\n{}",
        violations.join("\n")
    );
}

// ============================================================================
// Behavioral complement
// ============================================================================
//
// The scan above proves no *internal call site* can re-enter the shim. This
// is the behavioral half (former `integration_conformance.rs` §10): with
// UZE's own shims dir first on PATH — the real-world shape once
// `uze setup <harness>` has run — every harness `detect()` must resolve
// the real executable, never its own shim.
//
// All four run inside one test function deliberately: each mutates the
// process-global PATH, and Rust test functions in one binary run in
// parallel; the crate-wide process-env lock in `uze-testkit` also
// serializes this against every other PATH-mutating test in this binary.

#[cfg(unix)]
fn write_fake_executable(dir: &Path, name: &str, version_line: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho '{version_line}'\n")).unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn upstream_executable_resolution_never_recurses_through_the_runtime_shim() {
    let root = uze_testkit::temp::scratch("shim-boundary-behavioral");
    let uze_home = UzeHome::at(root.join("uze"));
    let shims_dir = uze_home.shims_dir();
    let real_dir = root.join("real-bin");

    // A poisoned shim for every vendor name, plus the real thing in a
    // separate directory — shims_dir listed FIRST on PATH, exactly the
    // hazard shape after `uze setup <harness>` (`~/.uze/shims` ahead of
    // the real binary).
    for (name, poison, real) in [
        (
            "claude",
            "POISON (should never be read) 0.0.1",
            "9.9.9 (Real Claude)",
        ),
        ("codex", "codex-cli POISON", "codex-cli 9.9.9"),
        ("opencode2", "opencode2 vPOISON", "opencode2 v9.9.9"),
        ("agy", "1.0.0-POISON", "1.1.19"),
    ] {
        write_fake_executable(&shims_dir, name, poison);
        write_fake_executable(&real_dir, name, real);
    }

    let mut scope = uze_testkit::env::scope();
    scope.set(
        "PATH",
        uze_testkit::temp::join_paths(&[&shims_dir, &real_dir]),
    );

    let claude = ClaudeIntegration::new(root.join("claude"), uze_home.clone());
    let codex = CodexIntegration::new(root.join("agents"), uze_home.clone());
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("opencode-config.json"),
        uze_home.clone(),
    );
    let antigravity = AntigravityIntegration::new(root.join("agents"), uze_home.clone());

    let claude_detection = claude.detect();
    let codex_detection = codex.detect();
    let opencode_detection = opencode.detect();
    let antigravity_detection = antigravity.detect();

    assert_eq!(
        claude_detection.version.as_deref(),
        Some("9.9.9"),
        "Claude detect() must resolve the real binary, never its own shim"
    );
    assert_eq!(
        codex_detection.version.as_deref(),
        Some("9.9.9"),
        "Codex detect() must resolve the real binary, never its own shim"
    );
    assert_eq!(
        opencode_detection.version.as_deref(),
        Some("v9.9.9"),
        "OpenCode detect() must resolve the real binary, never its own shim"
    );
    assert_eq!(
        antigravity_detection.version.as_deref(),
        Some("1.1.19"),
        "Antigravity detect() must resolve the real binary, never its own shim"
    );

    let _ = fs::remove_dir_all(root);
}

/// The same literal-call-shape scan, applied to the deterministic suite
/// itself.
///
/// A test that spawns `codex`/`claude`/… by bare name does not test the
/// vendor: on any machine where `uze setup` has run, that name resolves to
/// UZE's own shim, so the test measures whatever the developer's `PATH`
/// happens to hold. That is exactly how the real-Codex dogfood pair in
/// `harness/codex.rs` came to run `uze` instead of Codex and fail — while
/// passing in CI, where no shim exists. Skipping when the binary is absent
/// does not save it either: the guard reads "present on PATH" as "the real
/// harness", which is the false premise.
///
/// Everything under `tests/` therefore builds its world through
/// `uze-testkit` and asserts through UZE's own ports. Real-harness evidence
/// belongs to the Harness Conformance Lab (`conformance/`), where the
/// binary, HOME, and network are the container's — see
/// `docs/architecture/invariants.md` and `tests/README.md`.
#[test]
fn no_deterministic_test_spawns_a_bare_vendor_executable() {
    let mut files = Vec::new();
    rust_files(&deterministic_tests_dir(), &mut files);
    assert!(!files.is_empty(), "expected to find .rs files under tests/");

    let mut violations = Vec::new();
    for path in &files {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for (line_number, line) in contents.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            for vendor in VENDOR_NAMES {
                let needle = format!("Command::new(\"{vendor}\")");
                if line.contains(&needle) {
                    violations.push(format!("{}:{}: {needle}", path.display(), line_number + 1));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "found deterministic test(s) spawning a vendor binary from the ambient PATH, which is \
         UZE's own shim on any machine that has run `uze setup`. Build the world with \
         `uze-testkit` and assert through UZE's ports; put real-harness evidence in \
         `conformance/`:\n{}",
        violations.join("\n")
    );
}
