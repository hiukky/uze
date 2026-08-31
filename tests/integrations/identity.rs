//! Vendor-neutral integration contract (L1): every real integration
//! reports a stable id/capability declaration, and `uze-core` never
//! names a specific harness.
//!
//! Migrated verbatim from the former `tests/integration_conformance.rs`
//! (identity + core-neutrality); peer-exposure contracts live in
//! `tests/integrations/contract.rs`.

//! Integration Conformance Test Suite.
//!
//! Formalizes behavioral invariants that Claude, Codex, Antigravity, and
//! OpenCode already share — proven independently, per-integration, before
//! this suite existed — as a single, reusable set of assertions taken
//! against `&dyn IntegrationPort`. This is deliberately **not** a new
//! trait or framework: every helper below is a plain function; the only
//! "framework" concession is a couple of small, local fixture structs
//! (`CoverageFixture`, `SkillFixture`) that exist purely to avoid four-way
//! tuple returns, not to impose a shape on future integrations.
//!
//! Produced by, and should be read alongside, the Integration Capability
//! Contracts Audit: `IntegrationPort` stays unchanged, the public API is
//! unchanged, and no vendor module was refactored except two helpers
//! proven byte-for-byte (`crate::shared::provision`) or found NOT
//! byte-for-byte and deliberately left alone (the `..`/absolute-path
//! normalization each coverage function does — see that audit's
//! Duplication Analysis for the concrete divergence found).
//!
//! **What this suite deliberately does NOT assert** (per its own brief):
//! a vendor's manifest shape is never checked against another's;
//! OpenCode is never asked for package-level delivery (it has none, by
//! design); no publication/catalogue model is assumed identical across
//! vendors (Antigravity and OpenCode publish nothing at all, and that's
//! correct). Every assertion below is phrased as an *outcome* invariant
//! (route, coverage set, lifecycle state) — never as "the JSON must look
//! like X."

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

// `PATH` is process-global; every test below that mutates it must not
// interleave with another one doing the same under the default parallel
// test runner — same discipline, same reason, as
// `uze_core::harness_runtime`'s own `PATH_ENV_GUARD`.

use uze::{
    acquisition::{PackageSource, Provenance, ResolvedSource},
    capability::{Capability, CapabilityKind, Representation},
    home::UzeHome,
    integration::IntegrationPort,
    project::Resource,
    state,
    store::{PackageId, StoredPackage},
};

use uze::integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};

// ============================================================================
// Fixture plumbing — plain functions, not a framework.
// ============================================================================

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-conformance-{label}-{}-{nonce}",
        std::process::id()
    ))
}

/// Writes a canonical `plugin.json` plus every `(relative_path, content)`
/// pair, then builds the `StoredPackage` those bytes describe. Deliberately
/// mirrors a real `Store::ingest` result closely enough for
/// `package_exposure_plan`/`exposure_plan` to behave identically, without
/// pulling acquisition/Store machinery into this file — every fixture
/// still lives entirely under a throwaway temp root, never a real
/// `$UZE_HOME/store`.
fn build_package(
    label: &str,
    name: &str,
    extra_files: &[(&str, &str)],
) -> (PathBuf, StoredPackage) {
    let root = temp(label);
    let pkg_root = root.join("pkg");
    fs::create_dir_all(&pkg_root).unwrap();
    fs::write(
        pkg_root.join("plugin.json"),
        format!(r#"{{"name":"{name}","version":"1.0.0","description":"Conformance fixture"}}"#),
    )
    .unwrap();
    for (relative, content) in extra_files {
        let path = pkg_root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
    }
    let id = PackageId::from_plugin_name(name, &pkg_root.join("plugin.json")).unwrap();
    let package = StoredPackage {
        active_name: id.plugin_name().to_owned(),
        id,
        root: pkg_root.clone(),
        manifest: pkg_root.join("plugin.json"),
        provenance: Provenance {
            requested: PackageSource::Local {
                path: PathBuf::from("/tmp/fake"),
            },
            resolved: ResolvedSource::Local {
                path: PathBuf::from("/tmp/fake"),
            },
        },
    };
    (root, package)
}

fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
    state::record(
        home,
        state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: None,
            strategy: "conformance-fixture".to_owned(),
            installed: true,
        },
    )
    .unwrap();
}
// ============================================================================
// 1. Basic identity/capabilities contract — CORE_INTEGRATION.
// ============================================================================
//
// Every integration must report a non-empty stable id, a non-empty display
// name, and a capability declaration that says *something* about what it
// can deliver. This is intentionally the shallowest invariant in the
// suite: it exists to catch a fifth, future integration shipped with a
// blank `id()` or an empty `capabilities()`, not to compare vendors
// against each other.

fn assert_basic_identity_contract(integration: &dyn IntegrationPort) {
    assert!(!integration.id().is_empty(), "id() must not be empty");
    assert!(
        !integration.display_name().is_empty(),
        "display_name() must not be empty"
    );
    // The id is the stable logic key (receipts, state, matching); the label
    // is what every human-facing surface (TUI, README, CLI text) shows.
    // Equal strings mean the integration forgot its label and the UI would
    // regress to ids like `claude-code` next to labels like `Antigravity
    // CLI`.
    assert!(
        integration.display_name() != integration.id(),
        "{}: display_name() must be a human label distinct from the stable id — the id is for logic, the label for display",
        integration.id()
    );
    let capabilities = integration.capabilities();
    assert!(
        !(capabilities.direct_standard.is_empty()
            && capabilities.native.is_empty()
            && capabilities.adaptable.is_empty()
            && capabilities.degraded.is_empty()),
        "{}: capabilities() must declare at least one representable capability kind",
        integration.id()
    );
    assert!(
        !capabilities.evidence.is_empty(),
        "{}: capabilities() must state its evidence, never a silent claim",
        integration.id()
    );
}

/// Claude, Codex, and Antigravity deliver Skills and MCP as their own
/// native package/plugin — explicit or generated envelope, ADR-013 §2 /
/// ADR-020 / ADR-021 — so both kinds must be declared `native`, never
/// `adaptable`.
/// The capability-level shims (skills-dir reference, `mcp add`) are the
/// fallback for resources outside the envelope's coverage, not the primary
/// route; declaring them primary is exactly the "UI says Adapted while
/// delivery is Native" drift this assertion exists to catch.
fn assert_native_skill_and_mcp(integration: &dyn IntegrationPort) {
    let capabilities = integration.capabilities();
    for kind in [CapabilityKind::AgentSkill, CapabilityKind::Mcp] {
        assert!(
            capabilities.native.contains(&kind),
            "{}: {kind:?} must be declared native (package/extension delivery)",
            integration.id()
        );
        assert!(
            !capabilities.adaptable.contains(&kind),
            "{}: {kind:?} must not be declared adaptable — capability-level shims are the \
             fallback, not the primary route",
            integration.id()
        );
    }
}

/// OpenCode V2 is standard (`opencode`, legacy `opencode2` alias kept):
/// Skills are consumed natively from the shared `~/.agents/skills` root
/// (direct standard), MCP is now native via `opencode mcp add <name> --`
/// into `mcp.servers` (no `remove` verb, so detach stays file rewrite).
fn assert_opencode_native_skill_adapted_mcp(integration: &dyn IntegrationPort) {
    let capabilities = integration.capabilities();
    assert!(
        capabilities
            .direct_standard
            .contains(&CapabilityKind::AgentSkill),
        "opencode: AgentSkill must be declared direct_standard (native shared-root discovery)"
    );
    assert!(
        capabilities.native.contains(&CapabilityKind::Mcp),
        "opencode: MCP must be declared native (`opencode mcp add` CLI)"
    );
    assert!(
        !capabilities.adaptable.contains(&CapabilityKind::Mcp),
        "opencode: MCP must not be adaptable after native CLI migration"
    );
}

#[test]
fn claude_reports_stable_identity_and_capabilities() {
    let root = temp("identity-claude");
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    assert_basic_identity_contract(&integration);
    assert_native_skill_and_mcp(&integration);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_reports_stable_identity_and_capabilities() {
    let root = temp("identity-codex");
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_basic_identity_contract(&integration);
    assert_native_skill_and_mcp(&integration);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn opencode_reports_stable_identity_and_capabilities() {
    let root = temp("identity-opencode");
    let integration = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("opencode-config/opencode.json"),
        UzeHome::at(root.join("uze")),
    );
    assert_basic_identity_contract(&integration);
    assert_opencode_native_skill_adapted_mcp(&integration);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn antigravity_reports_stable_identity_and_capabilities() {
    let root = temp("identity-antigravity");
    let home = UzeHome::at(root.join("uze"));
    let integration = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_basic_identity_contract(&integration);
    assert_native_skill_and_mcp(&integration);
    // Invocation policy is the semantic dimension (ADR-030): Antigravity
    // has NO way to hide a Skill from the model or the user's slash
    // surface, so a canonical user-only Skill must route Adaptable with
    // the degradation stated — never Native, never silently model-visible
    // while claiming coverage. There is no canonical `Command` kind to
    // declare; a vendor Command may only ever be a projection detail.
    let (_root2, package) = build_package(
        "antigravity-policy",
        "flow",
        &[(
            "skills/review/SKILL.md",
            "---\nname: review\n---\n\nBody.\n",
        )],
    );
    let user_only = Resource::from_package(
        package.id.clone(),
        package.root.clone(),
        Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path: package.root.join("skills/review/SKILL.md"),
            payload: b"---\nname: review\ninvoke:\n  model: false\n  user: true\n---\n\nBody.\n"
                .to_vec(),
        },
    );
    let plan = integration.exposure_plan(&user_only);
    assert_eq!(
        plan.route,
        uze_core::router::CompatibilityRoute::Adaptable,
        "antigravity: a user-only Skill degrades honestly (no explicit-only mechanism)"
    );
    assert!(
        plan.evidence
            .contains("invoke.model=false cannot be enforced"),
        "the degradation must be stated in the evidence, never hidden"
    );
    let _ = fs::remove_dir_all(_root2);
    let _ = fs::remove_dir_all(root);
}
const VENDOR_NAMES: [&str; 4] = ["claude", "codex", "opencode", "antigravity"];

fn core_src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            // `tests.rs` is only ever reachable via `#[cfg(test)] mod
            // tests;` — an out-of-line test module split into its own file
            // is unconditionally test code, the same as an inline `#[cfg(test)]
            // mod tests { .. }` block that `strip_test_modules` already
            // skips; without this a file this large moving out of line
            // (as `src/ui/tests.rs` did) reads as thousands of new lines of
            // "production" fixture strings.
            if path.file_name().is_some_and(|name| name == "tests.rs") {
                continue;
            }
            out.push(path);
        }
    }
}

/// Strips every `#[cfg(test)] mod ... { ... }` body via brace-depth
/// tracking, so illustrative fixture strings inside a test module never
/// count as production vendor knowledge. Deliberately line-based, not a
/// real parser: this is a heuristic regression guard, not a compiler, and
/// the codebase's own convention (`#[cfg(test)]` immediately followed by
/// `mod name {`) is consistent enough for a line scan to track reliably.
fn strip_test_modules(contents: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut lines = contents.lines().enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        if line.trim() == "#[cfg(test)]"
            && let Some((_, next_line)) = lines.peek()
            && next_line.trim_start().starts_with("mod ")
        {
            let (_, mod_line) = lines.next().unwrap();
            let mut depth =
                mod_line.matches('{').count() as i32 - mod_line.matches('}').count() as i32;
            while depth > 0
                && let Some((_, body_line)) = lines.next()
            {
                depth +=
                    body_line.matches('{').count() as i32 - body_line.matches('}').count() as i32;
            }
            continue;
        }
        out.push((index, line.to_owned()));
    }
    out
}

#[test]
fn core_never_names_a_vendor_harness() {
    let mut files = Vec::new();
    rust_files(&core_src_dir(), &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .rs files under crates/uze-core/src"
    );

    let mut violations = Vec::new();
    for path in &files {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for (line_number, line) in strip_test_modules(&contents) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let lower = line.to_lowercase();
            for vendor in VENDOR_NAMES {
                // Word-boundary-ish match: the vendor name surrounded by
                // quote/paren/whitespace/underscore-adjacent characters,
                // not merely a substring — avoids flagging, say, an
                // unrelated English word that happens to contain "codex".
                if lower
                    .match_indices(vendor)
                    .any(|(index, _)| is_word_boundary_match(&lower, index, vendor.len()))
                {
                    violations.push(format!(
                        "{}:{}: contains vendor name `{vendor}`",
                        path.display(),
                        line_number + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "uze-core production logic must never name a specific harness (test fixtures are \
         deliberately excluded; see this test's own doc comment):\n{}",
        violations.join("\n")
    );
}
fn is_word_boundary_match(haystack: &str, start: usize, len: usize) -> bool {
    let bytes = haystack.as_bytes();
    let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
    let end = start + len;
    let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
    before_ok && after_ok
}

/// Shared neutrality scanner for the application and CLI/TUI guards below.
/// `dir` is walked recursively; comments are stripped (they are prose
/// allowed to explain vendor behavior) and `#[cfg(test)]` modules are
/// stripped (they are fixtures allowed to name harnesses).
fn vendor_names_in_production(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(dir, &mut files);
    let mut violations = Vec::new();
    for path in &files {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for (line_number, line) in strip_test_modules(&contents) {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let lower = line.to_lowercase();
            for vendor in VENDOR_NAMES {
                if lower
                    .match_indices(vendor)
                    .any(|(index, _)| is_word_boundary_match(&lower, index, vendor.len()))
                {
                    violations.push(format!(
                        "{}:{}: contains vendor name `{vendor}`",
                        path.display(),
                        line_number + 1
                    ));
                }
            }
        }
    }
    violations
}

/// The Application orchestrates integrations; it must not know which
/// harnesses exist. Composition is `uze-integrations`' registry's job.
#[test]
fn application_never_names_a_vendor_harness() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-application/src");
    let violations = vendor_names_in_production(&dir);
    assert!(
        violations.is_empty(),
        "uze-application production logic must never name a specific harness (composition \
         lives in uze-integrations' registry):\n{}",
        violations.join("\n")
    );
}

/// CLI/TUI presentation consumes registry descriptors and application read
/// models; a hard-coded vendor list here would drift the moment a harness
/// joins the registry. `uze-terminal` and `uze-extensions` are presentation
/// crates feeding the same UI, so they are held to the identical rule.
#[test]
fn cli_and_tui_never_name_a_vendor_harness() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = vendor_names_in_production(&root.join("src"));
    violations.extend(vendor_names_in_production(
        &root.join("crates/uze-terminal/src"),
    ));
    violations.extend(vendor_names_in_production(
        &root.join("crates/uze-extensions/src"),
    ));
    assert!(
        violations.is_empty(),
        "CLI/TUI production logic (including uze-terminal and uze-extensions) must never name \
         a specific harness (descriptors and read models only):\n{}",
        violations.join("\n")
    );
}
