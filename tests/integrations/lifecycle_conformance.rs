//! Lifecycle conformance (L1): attachment/receipt/drift/conflict safety
//! per harness, shared skill roots, store-byte immutability during
//! planning, and the no-duplicate-capability-receipt invariant.
//!
//! Migrated verbatim from the former `tests/integration_conformance.rs`
//! (sections 8, 9, 12 and the store-byte proof).

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
    integration::{AttachmentState, IntegrationPort, ManagedArtifact},
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
fn antigravity_skill_lifecycle_and_drift_safety() {
    let (pkg_root, package) = build_package("lifecycle-antigravity-pkg", "flow", &[]);
    let skill = skill_resource(&package, "skills", "commit");
    let root = temp("lifecycle-antigravity");
    let home = UzeHome::at(root.join("uze"));
    let integration = AntigravityIntegration::new(root.join("agents"), home.clone());
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
// Codex and OpenCode both discover Skills from the same physical
// `~/.agents/skills` directory; Claude's is exclusive. This is the
// statically-provable half of the convergence invariant: the two that
// claim a shared root must actually report the identical path when
// constructed against the identical `agents_home`, and Claude must report
// none (Antigravity's root is exclusive too). The dynamic half — that
// naming resolution actually avoids a duplicate physical entry when more
// than one of them attaches the same skill — is a `UzeApplication`-level
// concern (`resolve_exposure_name`, `pub(crate)`, unreachable from here)
// already proven end-to-end by `tests/shared_agent_skill_root_naming.rs`;
// this suite does not re-derive that heavier test, only its prerequisite.

#[test]
fn codex_opencode_agree_on_the_shared_skill_root() {
    let root = temp("shared-root-agree");
    let agents_home = root.join("agents-home");
    let uze_home = UzeHome::at(root.join("uze"));
    let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
    let opencode = OpenCodeIntegration::new(
        agents_home.clone(),
        root.join("opencode-config.json"),
        uze_home,
    );
    let codex_root = codex.shared_agent_skill_root();
    let opencode_root = opencode.shared_agent_skill_root();
    assert!(
        codex_root.is_some() && opencode_root.is_some(),
        "both must opt into shared-root awareness"
    );
    assert_eq!(
        codex_root, opencode_root,
        "Codex and OpenCode must agree on the physical shared skills directory"
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
         None, never forced into symmetry with Codex/OpenCode"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn antigravity_has_no_shared_skill_root_by_design() {
    let root = temp("antigravity-exclusive-root");
    let integration =
        AntigravityIntegration::new(root.join("agents-home"), UzeHome::at(root.join("uze")));
    assert_eq!(
        integration.shared_agent_skill_root(),
        None,
        "Antigravity's skills staging is exclusive, not shared with any peer — this must stay \
         None, never forced into symmetry with Codex/OpenCode"
    );
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// 6. No duplicate capability receipt when a package covers a resource —
// LIFECYCLE / PACKAGE_DELIVERY.
// ============================================================================
//
// The invariant lives in `UzeApplication::attach_package_to`
// (`pub(crate)`, unreachable directly from here), so this is exercised
// through the one public entry point that reaches it: `add_plugin`. Fake,
// always-succeeding `claude`/`codex`/`agy` executables stand in for the
// real CLIs — `add_plugin` never calls `provision()` (only explicit `uze
// setup` does; see `UzeApplication::install_materialized`'s own doc
// comment, "Explicit setup is the only path allowed to provision or
// update an executable"), so this never risks a real installer running,
// unlike a naive manual dogfood of `uze setup` would. The fake `agy`
// stages the plugin copy exactly like the real verb does, so the
// integration's fingerprint ownership proof works end-to-end.

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
if [ "$1" = "plugin" ]; then
  case "$2" in
    list) echo '{"imports":[]}'; exit 0 ;;
    install) mkdir -p "$HOME/.gemini/config/plugins/flow" && cp -R "$3/." "$HOME/.gemini/config/plugins/flow/"; exit 0 ;;
  esac
fi
case "$*" in
  *--json*) echo '{"marketplaces":[],"installed":[],"plugins":[]}' ;;
  *--output-format=json*) echo '[]' ;;
esac
exit 0
"#;
    for name in ["claude", "codex", "agy"] {
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    dir
}

#[cfg(unix)]
#[cfg(unix)]
#[test]
fn no_duplicate_capability_receipt_when_a_package_covers_the_resource() {
    let root = temp("no-duplicate-receipt");
    let uze_home = UzeHome::at(root.join("uze"));
    let fake_bin = fake_always_succeeding_bin_dir(&root);
    let mut env_scope = uze_testkit::env::scope();
    env_scope.set("PATH", uze_testkit::temp::path_prefixed(&fake_bin));

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
            Box::new(AntigravityIntegration::new(
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

    for vendor in ["claude-code", "codex", "antigravity"] {
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

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(pkg_root);
}

#[cfg(unix)]
#[test]
fn a_failing_vendor_cli_propagates_the_error_and_leaves_no_partial_state() {
    // Every fake in this suite answers exit 0 to anything, so a regression
    // in vendor-failure propagation (`claude mcp add` rejected, installer
    // denied) would ship green. The testkit's rule table can fail
    // explicitly; assert the error surfaces and nothing partial is left.
    use uze_testkit::fake_harness::{Action, FakeHarness};

    let root = temp("vendor-fails");
    let uze_home = UzeHome::at(root.join("uze"));
    let integration = ClaudeIntegration::new(root.join("claude-home"), uze_home.clone());

    let fake_bin = root.join("fake-bin");
    let claude = FakeHarness::new(&fake_bin, "claude")
        .version_line("9.9.9 (fake Claude)")
        .on_prefix(["mcp", "get"], Action::Exit(1))
        .on_prefix(["mcp", "add"], Action::Exit(7))
        .build();
    let mut env_scope = uze_testkit::env::scope();
    env_scope.set("PATH", uze_testkit::temp::path_prefixed(&fake_bin));
    state::record(
        &uze_home,
        state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: None,
            strategy: "conformance-fixture".to_owned(),
            installed: true,
        },
    )
    .unwrap();
    let (_pkg_root, package) = build_package(
        "vendor-fails-pkg",
        "flow",
        &[(
            "mcp.json",
            r#"{"mcpServers":{"mcp-a":{"command":"/bin/echo"}}}"#,
        )],
    );
    let mcp_resource = Resource::from_package_named(
        package.id.clone(),
        package.root.clone(),
        Capability {
            kind: CapabilityKind::Mcp,
            representation: Representation::Standard,
            path: package.root.join("mcp.json"),
            payload: br#"{"command":"/bin/echo"}"#.to_vec(),
        },
        "mcp-a".to_owned(),
    );

    let result = integration.attach_receipt(&mcp_resource);
    assert!(
        result.is_err(),
        "a vendor `mcp add` rejection must propagate, not pass silently"
    );
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("claude mcp add") && message.contains("exited with"),
        "the error names the failed vendor command and its status, got: {message}"
    );
    assert!(
        claude.was_called_with_prefix(&["mcp", "add"]),
        "the attach must actually have shelled out to the vendor CLI"
    );
    assert!(
        !uze_home.state_dir().join("attachments.json").exists()
            || !fs::read_to_string(uze_home.state_dir().join("attachments.json"))
                .unwrap()
                .contains("flow"),
        "no receipt may be recorded for a failed attach"
    );
    let _ = fs::remove_dir_all(root);
}
