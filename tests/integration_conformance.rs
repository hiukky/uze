//! Integration Conformance Test Suite.
//!
//! Formalizes behavioral invariants that Claude, Codex, Gemini, and
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
//! Gemini's manifest shape is never checked against Claude's or Codex's;
//! OpenCode is never asked for package-level delivery (it has none, by
//! design); no publication/catalogue model is assumed identical across
//! vendors (Gemini and OpenCode publish nothing at all, and that's
//! correct). Every assertion below is phrased as an *outcome* invariant
//! (route, coverage set, lifecycle state) — never as "the JSON must look
//! like X."

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

// `PATH` is process-global; every test below that mutates it must not
// interleave with another one doing the same under the default parallel
// test runner — same discipline, same reason, as
// `uze_core::harness_runtime`'s own `PATH_ENV_GUARD`.
static PATH_ENV_GUARD: Mutex<()> = Mutex::new(());

use uze::{
    acquisition::{PackageSource, Provenance, ResolvedSource},
    capability::{Capability, CapabilityKind, Representation},
    exposure::ExposureMechanism,
    home::UzeHome,
    integration::{AttachmentState, IntegrationPort, ManagedArtifact},
    project::Resource,
    router::CompatibilityRoute,
    state,
    store::{PackageId, StoredPackage},
};

use uze::integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, gemini::GeminiIntegration,
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

/// Claude, Codex, and Gemini deliver Skills and MCP as their own native
/// package/extension — explicit or generated envelope, ADR-013 §2 / ADR-020
/// / ADR-021 — so both kinds must be declared `native`, never `adaptable`.
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

/// OpenCode has no package-level native concept (deliberate, ADR-020's
/// non-goal, unchanged by ADR-021): Skills are consumed natively from the
/// shared `~/.agents/skills` discovery root (direct standard), MCP is
/// adapted through the managed `opencode.json` `mcp` config.
fn assert_opencode_native_skill_adapted_mcp(integration: &dyn IntegrationPort) {
    let capabilities = integration.capabilities();
    assert!(
        capabilities
            .direct_standard
            .contains(&CapabilityKind::AgentSkill),
        "opencode: AgentSkill must be declared direct_standard (native shared-root discovery)"
    );
    assert!(
        capabilities.adaptable.contains(&CapabilityKind::Mcp),
        "opencode: MCP must be declared adaptable (managed opencode.json config)"
    );
    assert!(
        capabilities.native.is_empty(),
        "opencode: no capability kind may be declared native (no package-level concept exists)"
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
fn gemini_reports_stable_identity_and_capabilities() {
    let root = temp("identity-gemini");
    let home = UzeHome::at(root.join("uze"));
    let integration = GeminiIntegration::new(root.join("agents"), home.clone());
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

// ============================================================================
// 2/4/5. Package coverage matrix + generated native projection +
// uncovered-capability fallback — PACKAGE_DELIVERY.
// ============================================================================
//
// One shared invariant, exercised identically for Claude, Codex, and
// Gemini (OpenCode has no package-level native concept at all — never
// asked for one, per this suite's own brief): `provided_resource_identities`
// must be an exact `discovered ∩ safely-representable` intersection, never
// a "manifest/structural surface exists → cover everything" shortcut, and
// every resource NOT in that set must still resolve to a non-Unsupported
// individual `exposure_plan` — never silently dropped.
//
// These fixtures ship NO vendor envelope, so for Claude/Codex/Gemini this
// exercises the GENERATED route (ADR-020/ADR-021) — the same coverage
// invariant applies whether the envelope is explicit or synthesized, and
// proving it against the generated route is what item 4 asks for. Item 3
// (explicit precedence) gets its own, separately-enveloped fixtures below.

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

/// Gemini: no explicit `gemini-extension.json` → generated route. Gemini's
/// own Skill-coverage rule is structural (no `skills` manifest field
/// exists at all, explicit or generated — confirmed by
/// `gemini/extension.rs`), which is exactly why this suite never asserts
/// Gemini's manifest shape against Claude's/Codex's: only the *outcome*
/// (exact coverage, uncovered-resource fallback) is asserted here.
fn gemini_coverage_fixture(
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
fn gemini_generated_coverage_full() {
    let (root, package, resources, expected) =
        gemini_coverage_fixture("gemini-full", &["commit"], false, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "gemini/full");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gemini_generated_coverage_subset_plus_extra_discovered() {
    let (root, package, resources, expected) =
        gemini_coverage_fixture("gemini-subset", &["commit", "deploy"], true, true);
    let refs: Vec<&Resource> = resources.iter().collect();
    let home = UzeHome::at(root.join("uze"));
    let integration = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_exact_package_coverage(&integration, &package, &refs, &expected, "gemini/subset");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gemini_explicit_coverage_malformed_envelope_yields_empty_mcp_but_no_crash() {
    // Gemini's Skill coverage is structural, not manifest-declared (see
    // this fixture builder's own doc comment) — a malformed manifest still
    // covers the Skill, but never fabricates MCP coverage it can't parse.
    let (root, package) = build_package(
        "gemini-malformed",
        "flow",
        &[("gemini-extension.json", "{not json")],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let resources = vec![&skill];
    let home = UzeHome::at(root.join("uze"));
    let integration = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("a present, even malformed, explicit envelope still takes the explicit route");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    assert_eq!(
        plan.provided_resource_identities,
        BTreeSet::from([skill.identity()])
    );
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 3. Explicit native envelope precedence — PACKAGE_DELIVERY.
// ============================================================================
//
// Presence of an explicit envelope — not its validity — decides the
// branch. An explicit envelope, even a malformed one, must never be
// silently displaced by generation. Claude/Codex/Gemini's malformed-
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
fn gemini_explicit_envelope_with_partial_declaration_is_never_topped_up_by_generation() {
    let (root, package) = build_package(
        "gemini-precedence",
        "flow",
        &[
            ("gemini-extension.json", r#"{"name":"flow"}"#),
            ("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#),
        ],
    );
    let skill = skill_resource(&package, "skills", "commit");
    let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
    let resources = vec![&skill, &mcp];
    let home = UzeHome::at(root.join("uze"));
    let integration = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    let plan = integration
        .package_exposure_plan(&package, &resources)
        .expect("explicit envelope present");
    // Gemini's Skill coverage is structural either way, so the skill is
    // still covered here — the invariant this proves is narrower but
    // still real: the explicit envelope's absence of an `mcpServers` key
    // must not fall back to generation's "read the root mcp.json" rule.
    assert_eq!(
        plan.provided_resource_identities,
        BTreeSet::from([skill.identity()]),
        "gemini-extension.json declares no mcpServers, and that absence must not be papered \
         over by generation reading the root mcp.json instead"
    );
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
// Gemini has no manifest-declared path field at all (its Skill coverage
// is purely structural — see `gemini/extension.rs`), so it is exempt from
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

fn assert_skill_lifecycle_and_drift_safety(integration: &dyn IntegrationPort, resource: &Resource) {
    let receipt = integration
        .attach_receipt(resource)
        .unwrap()
        .expect("attach_receipt must produce a receipt for a Skill once setup is recorded");
    assert_eq!(
        integration.inspect_receipt(&receipt).state,
        AttachmentState::Matched,
        "a freshly attached receipt must inspect as Matched"
    );

    if !matches!(receipt.artifact, ManagedArtifact::SymlinkReference { .. }) {
        panic!(
            "{}: expected a SymlinkReference artifact for Skill delivery, got {:?}",
            integration.id(),
            receipt.artifact
        );
    }

    // 7: destroy → inspect independently confirms Missing, not just trusts
    // detach's own return value.
    let detached = integration.detach_receipt(&receipt).unwrap();
    assert_eq!(detached.state, AttachmentState::Missing);
    assert_eq!(
        integration.inspect_receipt(&receipt).state,
        AttachmentState::Missing,
        "Missing must be independently re-provable by inspection, not only asserted once"
    );

    // Reattach for the drift/conflict half of this test.
    let receipt = integration
        .attach_receipt(resource)
        .unwrap()
        .expect("reattach must succeed after a clean detach");
    let ManagedArtifact::SymlinkReference { path, .. } = &receipt.artifact else {
        unreachable!("already matched this shape above");
    };

    // 8a: Drift — repoint the managed symlink at something else entirely.
    // Never observed as Matched; a detach attempt must be blocked (return
    // Drifted, not Missing) and must leave the repointed artifact in place.
    let elsewhere = path.parent().unwrap().join("conformance-drift-target");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::remove_file(path).unwrap();
    symlink(&elsewhere, path);
    assert_eq!(
        integration.inspect_receipt(&receipt).state,
        AttachmentState::Drifted
    );
    let blocked = integration.detach_receipt(&receipt).unwrap();
    assert_eq!(
        blocked.state,
        AttachmentState::Drifted,
        "a drifted artifact must never be silently destroyed by detach"
    );
    assert!(
        path.is_symlink(),
        "the drifted artifact must still exist, untouched, after a blocked detach"
    );
    assert_eq!(fs::read_link(path).unwrap(), elsewhere);

    // 8b: Conflict — replace the managed path with a foreign, non-symlink
    // file. Same discipline: inspection must say Conflict, detach must
    // refuse, and the foreign content must survive untouched.
    fs::remove_file(path).unwrap();
    fs::write(path, "foreign content this suite must never delete").unwrap();
    assert_eq!(
        integration.inspect_receipt(&receipt).state,
        AttachmentState::Conflict
    );
    let blocked = integration.detach_receipt(&receipt).unwrap();
    assert_eq!(blocked.state, AttachmentState::Conflict);
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "foreign content this suite must never delete",
        "content at a conflicting path must never be deleted or overwritten by a blocked detach"
    );

    fs::remove_file(path).ok();
}

#[cfg(unix)]
fn symlink(source: &Path, target: &Path) {
    std::os::unix::fs::symlink(source, target).unwrap();
}

#[cfg(unix)]
#[test]
fn claude_skill_lifecycle_and_drift_safety() {
    let (pkg_root, package) = build_package("lifecycle-claude-pkg", "flow", &[]);
    let skill = skill_resource(&package, "skills", "commit");
    let root = temp("lifecycle-claude");
    let home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &integration);
    assert_skill_lifecycle_and_drift_safety(&integration, &skill);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(pkg_root);
}

#[cfg(unix)]
#[test]
fn codex_skill_lifecycle_and_drift_safety() {
    let (pkg_root, package) = build_package("lifecycle-codex-pkg", "flow", &[]);
    let skill = skill_resource(&package, "skills", "commit");
    let root = temp("lifecycle-codex");
    let home = UzeHome::at(root.join("uze"));
    let integration = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_skill_lifecycle_and_drift_safety(&integration, &skill);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(pkg_root);
}

#[cfg(unix)]
#[test]
fn gemini_skill_lifecycle_and_drift_safety() {
    let (pkg_root, package) = build_package("lifecycle-gemini-pkg", "flow", &[]);
    let skill = skill_resource(&package, "skills", "commit");
    let root = temp("lifecycle-gemini");
    let home = UzeHome::at(root.join("uze"));
    let integration = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &integration);
    assert_skill_lifecycle_and_drift_safety(&integration, &skill);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(pkg_root);
}

#[cfg(unix)]
#[test]
fn opencode_skill_lifecycle_and_drift_safety() {
    let (pkg_root, package) = build_package("lifecycle-opencode-pkg", "flow", &[]);
    let skill = skill_resource(&package, "skills", "commit");
    let root = temp("lifecycle-opencode");
    let home = UzeHome::at(root.join("uze"));
    let integration = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("opencode-config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &integration);
    assert_skill_lifecycle_and_drift_safety(&integration, &skill);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(pkg_root);
}

// ============================================================================
// 9. Shared agent skill root convergence — SKILL.
// ============================================================================
//
// Codex, Gemini, and OpenCode all discover Skills from the same physical
// `~/.agents/skills` directory; Claude's is exclusive. This is the
// statically-provable half of the convergence invariant: the three that
// claim a shared root must actually report the identical path when
// constructed against the identical `agents_home`, and Claude must report
// none. The dynamic half — that naming resolution actually avoids a
// duplicate physical entry when more than one of the three attaches the
// same skill — is a `UzeApplication`-level concern
// (`resolve_exposure_name`, `pub(crate)`, unreachable from here) already
// proven end-to-end by `tests/shared_agent_skill_root_naming.rs`; this
// suite does not re-derive that heavier test, only its prerequisite.

#[test]
fn codex_gemini_opencode_agree_on_the_shared_skill_root() {
    let root = temp("shared-root-agree");
    let agents_home = root.join("agents-home");
    let uze_home = UzeHome::at(root.join("uze"));
    let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
    let gemini = GeminiIntegration::new(agents_home.clone(), uze_home.clone());
    let opencode = OpenCodeIntegration::new(
        agents_home.clone(),
        root.join("opencode-config.json"),
        uze_home,
    );
    let codex_root = codex.shared_agent_skill_root();
    let gemini_root = gemini.shared_agent_skill_root();
    let opencode_root = opencode.shared_agent_skill_root();
    assert!(
        codex_root.is_some() && gemini_root.is_some() && opencode_root.is_some(),
        "all three must opt into shared-root awareness"
    );
    assert_eq!(
        codex_root, gemini_root,
        "Codex and Gemini must agree on the physical shared skills directory"
    );
    assert_eq!(
        gemini_root, opencode_root,
        "Gemini and OpenCode must agree on the physical shared skills directory"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_has_no_shared_skill_root_by_design() {
    let root = temp("claude-exclusive-root");
    let integration = ClaudeIntegration::new(root.join("claude"), UzeHome::at(root.join("uze")));
    assert_eq!(
        integration.shared_agent_skill_root(),
        None,
        "Claude's skills directory is exclusive, not shared with any peer — this must stay \
         None, never forced into symmetry with Codex/Gemini/OpenCode"
    );
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 10. Upstream executable resolution must never recurse through UZE's own
// runtime shim — RUNTIME / PROVISIONING.
// ============================================================================
//
// `tests/runtime_shim_boundary.rs` proves this structurally (no source
// line spawns a bare `Command::new("<vendor>")`). This is the behavioral
// complement: given a `shims_dir` that precedes a real executable on
// `PATH` — the exact real-world shape once `uze setup <harness>` has run
// — `detect()` must resolve the real one, never the shim. All four run in
// one test function, deliberately: each mutates the process-global `PATH`,
// and Rust test functions in one binary run concurrently by default, so
// splitting this into four `#[test]`s would race on that shared global —
// `PATH_ENV_GUARD` additionally serializes this against every OTHER
// PATH-mutating test in this file (item 6's `add_plugin`-based test also
// mutates PATH; the two raced against each other under the default
// parallel runner before this guard was added).

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
    let root = temp("shim-boundary-behavioral");
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
        ("gemini", "0.0.1-POISON", "9.9.9"),
        ("opencode", "opencode2 vPOISON", "opencode2 v9.9.9"),
    ] {
        write_fake_executable(&shims_dir, name, poison);
        write_fake_executable(&real_dir, name, real);
    }

    let _guard = PATH_ENV_GUARD.lock().unwrap();
    let original_path = std::env::var("PATH").unwrap();
    // SAFETY: serialized against every other PATH-mutating test in this
    // binary by PATH_ENV_GUARD above; restored before returning.
    unsafe {
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}:{}",
                shims_dir.display(),
                real_dir.display(),
                original_path
            ),
        );
    }

    let claude = ClaudeIntegration::new(root.join("claude"), uze_home.clone());
    let codex = CodexIntegration::new(root.join("agents"), uze_home.clone());
    let gemini = GeminiIntegration::new(root.join("agents"), uze_home.clone());
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("opencode-config.json"),
        uze_home,
    );

    let claude_detection = claude.detect();
    let codex_detection = codex.detect();
    let gemini_detection = gemini.detect();
    let opencode_detection = opencode.detect();

    // SAFETY: restoring the process-global PATH this test overrode above.
    unsafe {
        std::env::set_var("PATH", original_path);
    }

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
        gemini_detection.version.as_deref(),
        Some("9.9.9"),
        "Gemini detect() must resolve the real binary, never its own shim"
    );
    assert_eq!(
        opencode_detection.version.as_deref(),
        Some("v9.9.9"),
        "OpenCode detect() must resolve the real binary, never its own shim"
    );

    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 11. Store bytes remain unchanged — folded into the generated-projection
// assertions above (item 4): every coverage-matrix fixture calls only
// `package_exposure_plan`, a documented read-only method, and each
// integration's own unit tests already prove `materialize_generated_*`
// never mutates the Store package directory
// (`materialize_generated_package_never_writes_into_the_store_package` and
// its Codex/Gemini equivalents). Re-asserted here as an explicit,
// cross-harness statement rather than left implicit.
// ============================================================================

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
    let gemini = GeminiIntegration::new(root.join("agents"), uze_home);
    let _ = claude.package_exposure_plan(&package, &refs);
    let _ = codex.package_exposure_plan(&package, &refs);
    let _ = gemini.package_exposure_plan(&package, &refs);

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
// Cleanup milestone (ADR-022): `crates/uze-core/src/importers/
// claude_plugin.rs` used to be the one real, production exception (a
// vendor-named foreign-format importer, confirmed dead — never reached by
// `Store::ingest` or any other production path — and removed). With it
// gone, this test needs only one scope narrowing, not two:
// `#[cfg(test)]` module bodies are skipped, because Core's own unit tests
// use realistic strings like `"claude-code"`/`"codex"` as illustrative
// fixture values for genuinely generic fields (`AttachmentReceipt.
// integration: String`) — that is not vendor coupling, it is a test
// picking a recognizable example over `"foo"`.

const VENDOR_NAMES: [&str; 4] = ["claude", "codex", "gemini", "opencode"];

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

// ============================================================================
// 6. No duplicate capability receipt when a package covers a resource —
// LIFECYCLE / PACKAGE_DELIVERY.
// ============================================================================
//
// The invariant lives in `UzeApplication::attach_package_to`
// (`pub(crate)`, unreachable directly from here), so this is exercised
// through the one public entry point that reaches it: `add_plugin`. Fake,
// always-succeeding `claude`/`codex`/`gemini` executables stand in for the
// real CLIs — `add_plugin` never calls `provision()` (only explicit `uze
// setup` does; see `UzeApplication::install_materialized`'s own doc
// comment, "Explicit setup is the only path allowed to provision or
// update an executable"), so this never risks a real installer running,
// unlike a naive manual dogfood of `uze setup` would.

/// `add_plugin` never calls `.provision()` (only explicit `uze setup`
/// does), so this is never exercised — present only so `add_plugin`'s
/// composition root has a concrete `ProcessRunner` to hold, never a real
/// one that could spawn an installer.
struct NeverCalledProcessRunner;

impl uze::provisioning::ProcessRunner for NeverCalledProcessRunner {
    fn run(
        &self,
        _spec: &uze::provisioning::ProcessSpec,
    ) -> uze::Result<uze::provisioning::ProcessResult> {
        panic!(
            "add_plugin must never invoke ProcessRunner::run — only explicit `uze setup` provisions"
        );
    }
}

#[cfg(unix)]
fn fake_always_succeeding_bin_dir(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join("fake-bin");
    fs::create_dir_all(&dir).unwrap();
    let script = r#"#!/bin/sh
case "$*" in
  *--json*) echo '{"marketplaces":[],"installed":[],"plugins":[]}' ;;
  *--output-format=json*) echo '[]' ;;
esac
exit 0
"#;
    for name in ["claude", "codex", "gemini"] {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    dir
}

#[cfg(unix)]
#[test]
fn no_duplicate_capability_receipt_when_a_package_covers_the_resource() {
    let root = temp("no-duplicate-receipt");
    let uze_home = UzeHome::at(root.join("uze"));
    let fake_bin = fake_always_succeeding_bin_dir(&root);
    let _guard = PATH_ENV_GUARD.lock().unwrap();
    let original_path = std::env::var("PATH").unwrap();
    // SAFETY: serialized against every other PATH-mutating test in this
    // binary by PATH_ENV_GUARD above; restored before returning.
    unsafe {
        std::env::set_var("PATH", format!("{}:{}", fake_bin.display(), original_path));
    }

    let application = uze::UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![
            Box::new(ClaudeIntegration::new(
                root.join("claude-home"),
                uze_home.clone(),
            )),
            Box::new(CodexIntegration::new(
                root.join("agents-home"),
                uze_home.clone(),
            )),
            Box::new(GeminiIntegration::new(
                root.join("agents-home"),
                uze_home.clone(),
            )),
        ],
        Box::new(NeverCalledProcessRunner),
    );

    let (pkg_root, _package) = build_package("no-duplicate-receipt-pkg", "flow", &[]);
    // `skill_resource` already wrote `skills/commit/SKILL.md` under this
    // package root; `add_plugin` re-discovers it through the normal
    // acquisition + Engine composition path, exactly like a real install.
    let _ = skill_resource(&_package, "skills", "commit");

    let report = application
        .add_plugin(
            uze::PackageSource::local(pkg_root.join("pkg")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();

    let receipts = state::receipts(&uze_home, Some(report.plugin.id.as_str())).unwrap();

    for vendor in ["claude-code", "codex", "gemini"] {
        let for_vendor: Vec<_> = receipts
            .iter()
            .filter(|(_, receipt)| receipt.integration == vendor)
            .collect();
        assert_eq!(
            for_vendor.len(),
            1,
            "{vendor}: expected exactly one receipt for the fully-package-covered skill, got {} \
             ({for_vendor:?}) — a package-level delivery must never ALSO produce a resource-level \
             capability receipt for a resource it already covers",
            for_vendor.len()
        );
        let (_, receipt) = for_vendor[0];
        assert!(
            matches!(receipt.artifact, ManagedArtifact::IntegrationOwned { .. }),
            "{vendor}: the one receipt for a package-covered resource must be package-level \
             (IntegrationOwned), not a resource-level artifact: {:?}",
            receipt.artifact
        );
        assert_eq!(
            receipt.resource_identity, None,
            "{vendor}: a package-level receipt must not carry a single resource_identity — it \
             covers the package, not one capability"
        );
    }

    // SAFETY: restoring the process-global PATH this test overrode above.
    unsafe {
        std::env::set_var("PATH", original_path);
    }
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(pkg_root);
}
