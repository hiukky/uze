//! Capability conformance (L1/L2): per-harness `package_exposure_plan`
//! exact-coverage semantics — generated native projection, explicit
//! envelope precedence, unsafe-declaration rejection and fallback.
//!
//! Migrated verbatim from the former `tests/integration_conformance.rs`
//! (the PACKAGE_DELIVERY half); the lifecycle half now lives in
//! `tests/integrations/lifecycle_conformance.rs`.

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
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

// `PATH` is process-global; every test below that mutates it must not
// interleave with another one doing the same under the default parallel
// test runner — same discipline, same reason, as
// `uze_core::harness_runtime`'s own `PATH_ENV_GUARD`.

use uze_core::{
    acquisition::{PackageSource, Provenance, ResolvedSource},
    capability::{Capability, CapabilityKind, Representation},
    exposure::ExposureMechanism,
    home::UzeHome,
    integration::IntegrationPort,
    project::Resource,
    router::CompatibilityRoute,
    state,
    store::{PackageId, StoredPackage},
};

use uze_integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
};

// ============================================================================
// Fixture plumbing — plain functions, not a framework.
// ============================================================================

fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
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

/// Writes `<dir>/<name>/SKILL.md` under the package root and returns the
/// discovered `Resource` for it — the same shape `UzeEngine` would produce.
/// Writes `<dir>/<name>/SKILL.md` under the package root and returns the
/// discovered `Resource` for it — the same shape `UzeEngine` would produce.
fn skill_resource(package: &StoredPackage, dir: &str, name: &str) -> Resource {
    let path = package.root.join(dir).join(name).join("SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("---\nname: {name}\n---\n\nBody.\n")).unwrap();
    Resource::from_package(
        package.id.clone(),
        package.root.clone(),
        Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path,
            payload: Vec::new(),
        },
    )
}
/// A skill resource at a path outside every convention a native package
/// tier reads from — used for the "extra discovered resource" coverage
/// case, never for a case that expects it covered.
fn skill_resource_outside_conventions(package: &StoredPackage, name: &str) -> Resource {
    skill_resource(package, "unconventional-location", name)
}
fn mcp_resource(package: &StoredPackage, name: &str, payload: &str) -> Resource {
    let path = package.root.join("mcp.json");
    Resource::from_package_named(
        package.id.clone(),
        package.root.clone(),
        Capability {
            kind: CapabilityKind::Mcp,
            representation: Representation::Standard,
            path,
            payload: payload.as_bytes().to_vec(),
        },
        name.to_owned(),
    )
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
//
// One shared invariant, exercised identically for Claude, Codex, and
// Antigravity (OpenCode has no package-level native concept at all — never
// asked for one, per this suite's own brief): `provided_resource_identities`
// must be an exact `discovered ∩ safely-representable` intersection, never
// a "manifest/structural surface exists → cover everything" shortcut, and
// every resource NOT in that set must still resolve to a non-Unsupported
// individual `exposure_plan` — never silently dropped.
//
// These fixtures ship no vendor envelope beyond the canonical manifest, so
// for Claude/Codex this exercises the GENERATED route (ADR-013)
// while for Antigravity the canonical plugin.json IS the vendor manifest
// (generation only kicks in for canonical-MCP translation) — the same
// coverage invariant applies either way, and proving it is what item 4
// asks for. Item 3 (explicit precedence) gets its own,
// separately-enveloped fixtures below.

fn assert_exact_package_coverage(
    integration: &dyn IntegrationPort,
    package: &StoredPackage,
    resources: &[&Resource],
    expected_covered: &BTreeSet<String>,
    case: &str,
) {
    let plan = integration
        .package_exposure_plan(package, resources)
        .unwrap_or_else(|| panic!("[{case}] expected a package exposure plan"));
    assert_eq!(
        plan.route,
        CompatibilityRoute::Native,
        "[{case}] a package delivery plan must route Native"
    );
    assert_eq!(
        &plan.provided_resource_identities, expected_covered,
        "[{case}] provided_resource_identities must be an exact discovered ∩ declared intersection"
    );
    // Item 5: every uncovered resource must still resolve through the
    // normal per-resource fallback, never silently disappear.
    for resource in resources {
        if !expected_covered.contains(&resource.identity()) {
            let fallback = integration.exposure_plan(resource);
            assert!(
                !matches!(fallback.mechanism, ExposureMechanism::Unsupported { .. }),
                "[{case}] uncovered resource {} must still route through capability-level \
                 fallback, not Unsupported",
                resource.identity()
            );
        }
    }
}

/// Claude: no explicit `.claude-plugin/plugin.json` → generated route.
fn claude_coverage_fixture(
    label: &str,
    skill_dirs: &[&str],
    with_extra_skill: bool,
    with_mcp: bool,
) -> (PathBuf, StoredPackage, Vec<Resource>, BTreeSet<String>) {
    let files: &[(&str, &str)] = if with_mcp {
        &[("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#)]
    } else {
        &[]
    };
    let (root, package) = build_package(label, "flow", files);
    let mut resources = Vec::new();
    let mut expected = BTreeSet::new();
    for name in skill_dirs {
        let resource = skill_resource(&package, "skills", name);
        expected.insert(resource.identity());
        resources.push(resource);
    }
    if with_extra_skill {
        resources.push(skill_resource_outside_conventions(&package, "outsider"));
    }
    if with_mcp {
        let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
        expected.insert(mcp.identity());
        resources.push(mcp);
    }
    (root, package, resources, expected)
}

#[test]
fn claude_generated_coverage_full() {
    let (root, package, resources, expected) =
        claude_coverage_fixture("claude-full", &["commit"], false, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "claude/full");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_generated_coverage_subset_plus_extra_discovered() {
    // Two skills declared/conventional, one MCP; only-the-conventional-ones
    // must be covered — the "subset" and "extra discovered" cases fold
    // together naturally here since both are proven by the same
    // discovered-outside-the-conventional-surface resource.
    let (root, package, resources, expected) =
        claude_coverage_fixture("claude-subset", &["commit", "deploy"], true, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "claude/subset");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_generated_coverage_malformed_explicit_envelope_yields_empty_but_no_crash() {
    // A malformed EXPLICIT envelope: present, so explicit route is taken
    // (never generation), but unparseable, so coverage is empty rather
    // than a crash or a silent "cover everything."
    let (root, package) = build_package(
        "claude-malformed",
        "flow",
        &[(".claude-plugin/plugin.json", "{not json")],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("a present, even malformed, explicit envelope still takes the explicit route");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    assert!(
        plan.provided_resource_identities.is_empty(),
        "malformed declaration must yield empty coverage, not a crash and not full coverage"
    );
    let fallback = integration.exposure_plan(&skill);
    assert!(!matches!(
        fallback.mechanism,
        ExposureMechanism::Unsupported { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_generated_coverage_path_escape_is_rejected() {
    let (root, package) = build_package(
        "claude-escape",
        "flow",
        &[(
            ".claude-plugin/plugin.json",
            r#"{"name":"flow","skills":["../../etc","/absolute-that-still-resolves-relative"]}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("explicit envelope present");
    assert!(
        plan.provided_resource_identities.is_empty(),
        "a `..`-escaping declaration must never cover a real resource that happens to share no \
         name with it"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_generated_coverage_duplicate_declaration_is_deduplicated_not_double_counted() {
    let (root, package) = build_package(
        "claude-duplicate",
        "flow",
        &[(
            ".claude-plugin/plugin.json",
            r#"{"name":"flow","skills":["./skills/commit","./skills/commit","skills/commit"]}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("explicit envelope present");
    assert_eq!(
        plan.provided_resource_identities,
        BTreeSet::from([skill.identity()]),
        "a repeated declaration must still resolve to exactly one covered identity"
    );
    let _ = fs::remove_dir_all(root);
}

/// Codex: no explicit `.codex-plugin/plugin.json` → generated route.
fn codex_coverage_fixture(
    label: &str,
    skill_dirs: &[&str],
    with_extra_skill: bool,
    with_mcp: bool,
) -> (PathBuf, StoredPackage, Vec<Resource>, BTreeSet<String>) {
    let files: &[(&str, &str)] = if with_mcp {
        &[("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#)]
    } else {
        &[]
    };
    let (root, package) = build_package(label, "flow", files);
    let mut resources = Vec::new();
    let mut expected = BTreeSet::new();
    for name in skill_dirs {
        let resource = skill_resource(&package, "skills", name);
        expected.insert(resource.identity());
        resources.push(resource);
    }
    if with_extra_skill {
        resources.push(skill_resource_outside_conventions(&package, "outsider"));
    }
    if with_mcp {
        let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
        expected.insert(mcp.identity());
        resources.push(mcp);
    }
    (root, package, resources, expected)
}

#[test]
fn codex_generated_coverage_full() {
    let (root, package, resources, expected) =
        codex_coverage_fixture("codex-full", &["commit"], false, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "codex/full");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_generated_coverage_subset_plus_extra_discovered() {
    let (root, package, resources, expected) =
        codex_coverage_fixture("codex-subset", &["commit", "deploy"], true, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "codex/subset");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_explicit_coverage_malformed_envelope_yields_empty_but_no_crash() {
    let (root, package) = build_package(
        "codex-malformed",
        "flow",
        &[(".codex-plugin/plugin.json", "{not json")],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("a present, even malformed, explicit envelope still takes the explicit route");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    assert!(plan.provided_resource_identities.is_empty());
    let fallback = integration.exposure_plan(&skill);
    assert!(!matches!(
        fallback.mechanism,
        ExposureMechanism::Unsupported { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_explicit_coverage_path_escape_is_rejected() {
    let (root, package) = build_package(
        "codex-escape",
        "flow",
        &[(
            ".codex-plugin/plugin.json",
            r#"{"name":"flow","skills":"../../etc"}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("explicit envelope present");
    assert!(plan.provided_resource_identities.is_empty());
    let _ = fs::remove_dir_all(root);
}

/// Antigravity: the canonical `plugin.json` IS the explicit envelope (and
/// the vendor manifest), so the same "envelope-less package" fixture takes
/// the GENERATED route only when canonical `mcp.json` needs translating —
/// which is exactly this fixture's shape (skill + mcp.json). Coverage and
/// the uncovered-resource fallback behave exactly like the other vendors'.
fn antigravity_coverage_fixture(
    label: &str,
    skill_dirs: &[&str],
    with_extra_skill: bool,
    with_mcp: bool,
) -> (PathBuf, StoredPackage, Vec<Resource>, BTreeSet<String>) {
    let files: &[(&str, &str)] = if with_mcp {
        &[("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#)]
    } else {
        &[]
    };
    let (root, package) = build_package(label, "flow", files);
    let mut resources = Vec::new();
    let mut expected = BTreeSet::new();
    for name in skill_dirs {
        let resource = skill_resource(&package, "skills", name);
        expected.insert(resource.identity());
        resources.push(resource);
    }
    if with_extra_skill {
        resources.push(skill_resource_outside_conventions(&package, "outsider"));
    }
    if with_mcp {
        let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
        expected.insert(mcp.identity());
        resources.push(mcp);
    }
    (root, package, resources, expected)
}

#[test]
fn antigravity_generated_coverage_full() {
    let (root, package, resources, expected) =
        antigravity_coverage_fixture("antigravity-full", &["commit"], false, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "antigravity/full");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn antigravity_generated_coverage_subset_plus_extra_discovered() {
    let (root, package, resources, expected) =
        antigravity_coverage_fixture("antigravity-subset", &["commit", "deploy"], true, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(
        &integration,
        &package,
        &refs,
        &expected,
        "antigravity/subset",
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn antigravity_explicit_coverage_malformed_manifest_yields_no_native_plan() {
    // A malformed canonical plugin.json is an unreadable vendor manifest:
    // no explicit route, and generation is never consulted to paper over it
    // (plugging the malformed manifest with a synthesized one would be
    // exactly the "silently displaced explicit envelope" this suite
    // forbids). The resource still routes through capability-level
    // delivery.
    let (root, package) = build_package(
        "antigravity-malformed",
        "flow",
        &[("plugin.json", "{not json")],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert!(
        integration
            .package_exposure_plan(&package, &resources)
            .is_none(),
        "malformed canonical manifest must never be silently displaced by generation"
    );
    let fallback = integration.exposure_plan(&skill);
    assert!(!matches!(
        fallback.mechanism,
        ExposureMechanism::Unsupported { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 3. Explicit native envelope precedence — PACKAGE_DELIVERY.
// ============================================================================
//
// Presence of an explicit envelope — not its validity — decides the
// branch. An explicit envelope, even a malformed one, must never be
// silently displaced by generation. Claude/Codex's malformed-
// envelope cases above already prove "explicit still wins when malformed";
// this section adds the complementary case: an explicit envelope that
// declares LESS than generation would, proving generation is never
// consulted at all once an explicit envelope file exists.

#[test]
fn claude_explicit_envelope_with_partial_declaration_is_never_topped_up_by_generation() {
    let (root, package) = build_package(
        "claude-precedence",
        "flow",
        &[
            (
                ".claude-plugin/plugin.json",
                r#"{"name":"flow","skills":["./skills/commit"]}"#,
            ),
            ("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#),
        ],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
    let resources = vec![&skill, &mcp];
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("explicit envelope present");
    // The explicit envelope declares only the skill; even though the
    // package ALSO has a root mcp.json generation would have picked up,
    // presence of the explicit envelope must keep coverage exactly at
    // what it itself declares.
    assert_eq!(
        plan.provided_resource_identities,
        BTreeSet::from([skill.identity()]),
        "explicit envelope's own declared subset must never be topped up by what generation \
         would have additionally covered"
    );
    let mcp_fallback = integration.exposure_plan(&mcp);
    assert!(!matches!(
        mcp_fallback.mechanism,
        ExposureMechanism::Unsupported { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_explicit_envelope_with_partial_declaration_is_never_topped_up_by_generation() {
    let (root, package) = build_package(
        "codex-precedence",
        "flow",
        &[
            (
                ".codex-plugin/plugin.json",
                r#"{"name":"flow","skills":"./skills/"}"#,
            ),
            ("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#),
        ],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
    let resources = vec![&skill, &mcp];
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("explicit envelope present");
    // Codex's explicit `mcpServers` field points at an external file
    // (`.mcp.json` by convention), never the root `mcp.json` generation
    // reads — so an explicit envelope declaring only `skills` truly
    // cannot see the root mcp.json at all, proving precedence rather than
    // assuming it.
    assert_eq!(
        plan.provided_resource_identities,
        BTreeSet::from([skill.identity()])
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn antigravity_explicit_envelope_with_partial_declaration_is_never_topped_up_by_generation() {
    // Antigravity's explicit envelope is the canonical plugin.json itself.
    // When the author ALSO ships a vendor mcp_config.json declaring a
    // subset, generation (which would translate the full canonical
    // mcp.json) must never be consulted: coverage is exactly the declared
    // subset, and the undeclared MCP resource falls back individually.
    let (root, package) = build_package(
        "antigravity-precedence",
        "flow",
        &[
            (
                "mcp.json",
                r#"{"mcpServers":{"mcp-a":{"command":"a"},"mcp-b":{"command":"b"}}}"#,
            ),
            (
                "mcp_config.json",
                r#"{"mcpServers":{"mcp-b":{"command":"b"}}}"#,
            ),
        ],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let mcp_declared = mcp_resource(&package, "mcp-b", r#"{"command":"b"}"#);
    let mcp_undeclared = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
    let resources = vec![&skill, &mcp_declared, &mcp_undeclared];
    let home = UzeHome::at(root.join("uze"));
    let integration = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("the author-shipped mcp_config.json keeps the explicit route");
    assert_eq!(
        plan.provided_resource_identities,
        BTreeSet::from([skill.identity(), mcp_declared.identity()]),
        "explicit declaration's own subset must never be topped up by what generation would \
         have covered from the canonical mcp.json"
    );
    let fallback = integration.exposure_plan(&mcp_undeclared);
    assert!(!matches!(
        fallback.mechanism,
        ExposureMechanism::Unsupported { .. }
    ));
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// Path safety hardening — PACKAGE_DELIVERY.
// ============================================================================
//
// Regression coverage for a real bug found by the Path Safety +
// Foreign Importer Cleanup audit: Claude's per-entry manifest-path
// normalization used to strip a leading `/` before testing whether a
// declaration was absolute, so `/skills/foo` silently became the relative
// declaration `skills/foo` and was ACCEPTED — Codex's independently
// written equivalent never did this. Both now share one fixed predicate
// (`crate::shared::path::normalize_declared_relative_path`); this proves
// the INVARIANT the fix restores, not either vendor's specific syntax:
//
//   An invalid/unsafe native manifest path can never be made valid by
//   destructive normalization.
//
//   An invalid native coverage declaration must not suppress the
//   resource's normal capability-level fallback.
//
// Antigravity has no manifest-declared path field at all (its coverage is
// purely structural — see `antigravity/plugin.rs`), so it is exempt from
// this section entirely, not silently assumed safe.

fn assert_unsafe_declaration_never_covers_the_colliding_resource(
    integration: &dyn IntegrationPort,
    package: &StoredPackage,
    resource: &Resource,
    case: &str,
) {
    if let Some(plan) = integration.package_exposure_plan(package, &[resource]) {
        assert!(
            !plan
                .provided_resource_identities
                .contains(&resource.identity()),
            "[{case}] an unsafe declaration must never cover the real resource it collides \
             with, even though a destructive normalizer would have made them match"
        );
    }
    // The resource must still be attachable through the normal
    // capability-level fallback — an invalid declaration must never
    // suppress delivery of a real, otherwise-valid capability.
    let fallback = integration.exposure_plan(resource);
    assert!(
        !matches!(fallback.mechanism, ExposureMechanism::Unsupported { .. }),
        "[{case}] a resource left uncovered by an invalid declaration must still route through \
         normal fallback, never Unsupported"
    );
}

#[test]
fn claude_absolute_declaration_never_covers_colliding_resource_and_fallback_survives() {
    let (root, package) = build_package(
        "claude-path-safety-absolute",
        "flow",
        &[(
            ".claude-plugin/plugin.json",
            r#"{"name":"flow","skills":["/skills/commit"]}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    assert_unsafe_declaration_never_covers_the_colliding_resource(
        &integration,
        &package,
        &skill,
        "claude/absolute",
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_whitespace_padded_absolute_declaration_never_covers_colliding_resource() {
    let (root, package) = build_package(
        "claude-path-safety-padded",
        "flow",
        &[(
            ".claude-plugin/plugin.json",
            r#"{"name":"flow","skills":["  /skills/commit  "]}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    assert_unsafe_declaration_never_covers_the_colliding_resource(
        &integration,
        &package,
        &skill,
        "claude/padded-absolute",
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_absolute_declaration_never_covers_colliding_resource_and_fallback_survives() {
    let (root, package) = build_package(
        "codex-path-safety-absolute",
        "flow",
        &[(
            ".codex-plugin/plugin.json",
            r#"{"name":"flow","skills":"/skills/"}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_unsafe_declaration_never_covers_the_colliding_resource(
        &integration,
        &package,
        &skill,
        "codex/absolute",
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn codex_escaping_declaration_never_covers_colliding_resource_and_fallback_survives() {
    let (root, package) = build_package(
        "codex-path-safety-escape",
        "flow",
        &[(
            ".codex-plugin/plugin.json",
            r#"{"name":"flow","skills":"skills/../skills"}"#,
        )],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_unsafe_declaration_never_covers_the_colliding_resource(
        &integration,
        &package,
        &skill,
        "codex/escape",
    );
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 7/8. attach → inspect Matched → detach → Missing, and destructive detach
// blocked on Drifted/Conflict — LIFECYCLE (ADR-009).
// ============================================================================
//
// Exercised through Skill delivery on purpose: it is the one capability
// every one of the four harnesses actually implements, and — for all
// four — its managed artifact is a pure filesystem `SymlinkReference`
// (`IntegrationPort::attach_receipt`'s own generic default builds it off
// `ExposureMechanism::ManagedUserScopeReference`, and no integration
// overrides `attach_receipt`), so this proves the lifecycle invariant
// without spawning any vendor process for any of the four.

fn snapshot(root: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.insert(path);
        }
    }
    out
}

#[test]
fn computing_a_package_exposure_plan_never_mutates_store_bytes_on_any_harness() {
    let (root, package, resources, _expected) =
        claude_coverage_fixture("store-untouched", &["commit"], false, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let before = snapshot(&package.root);

    let uze_home = UzeHome::at(root.join("uze"));
    let claude = ClaudeIntegration::new(root.join("claude"), uze_home.clone());
    let codex = CodexIntegration::new(root.join("agents"), uze_home.clone());
    let antigravity = AntigravityIntegration::new(root.join("agents"), uze_home);
    let _ = claude.package_exposure_plan(&package, &refs);
    let _ = codex.package_exposure_plan(&package, &refs);
    let _ = antigravity.package_exposure_plan(&package, &refs);

    let after = snapshot(&package.root);
    assert_eq!(
        before, after,
        "computing a package exposure plan on any harness must never mutate the Store's own \
         package tree"
    );
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 12. Core vendor neutrality remains intact — structural.
// ============================================================================
//
// `uze-core` production logic is vendor-neutral: no line of it may name a
// specific harness. Scans every non-test `.rs` file under
// `crates/uze-core/src/` for the four harness names appearing as a live
// identifier or string literal, mirroring `tests/runtime_shim_boundary.rs`'s
// technique for the shim-boundary invariant.
//
// This invariant was strengthened by the Path Safety + Foreign Importer
// Cleanup milestone (ADR-005): `crates/uze-core/src/importers/
// claude_plugin.rs` used to be the one real, production exception (a
// vendor-named foreign-format importer, confirmed dead — never reached by
// `Store::ingest` or any other production path — and removed). With it
// gone, this test needs only one scope narrowing, not two:
// `#[cfg(test)]` module bodies are skipped, because Core's own unit tests
// use realistic strings like `"claude-code"`/`"codex"` as illustrative
// fixture values for genuinely generic fields (`AttachmentReceipt.
// integration: String`) — that is not vendor coupling, it is a test
// picking a recognizable example over `"foo"`.
