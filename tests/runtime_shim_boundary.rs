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
//! Deliberately source-level, not behavioral: a behavioral test would need
//! a real or faked shim on `PATH` to prove recursion, which is exactly the
//! kind of environment-dependent setup this invariant must hold regardless
//! of. Scanning for the literal call shape is a direct, cheap proxy for "no
//! internal call site can possibly re-enter the shim," and it fails loudly
//! and specifically (file + line) the moment someone reintroduces one.

use std::{fs, path::Path};

const VENDOR_NAMES: [&str; 4] = ["claude", "codex", "opencode", "agy"];

fn integrations_src_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-integrations/src")
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
