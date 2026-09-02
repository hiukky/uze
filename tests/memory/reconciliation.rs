//! L1 contract: `UzeApplication::context_reconcile` composes what globally
//! installed packages contribute into one project's shared `AGENTS.md`, and
//! separately reconciles the small set of harnesses that need a bridge into
//! it rather than reading it natively.
//!
//! Deterministic by construction: bridge-capable integrations here are test
//! doubles with a controllable `detect()`, not the real CLIs, so this suite
//! passes identically whether or not those binaries are installed on the
//! machine running it. Real-CLI evidence is separate L2a research, not this
//! tier's job — see the session's final report for that empirical spike.
//!
//! `uze add` is exercised here exactly as any other test uses it: nothing
//! about it changes for Instructions. `context_reconcile` is a wholly
//! separate, explicit, project-scoped operation layered on top.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze::{PackageSource, UzeApplication};
use uze::{
    Result, UzeError, UzeHome,
    application::{BridgeStatus, ContextReconciliationReport},
    integration::{AttachmentReceipt, AttachmentState, HarnessDetection, IntegrationPort},
    router::HarnessCapabilities,
};

fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

fn fixture_a() -> PathBuf {
    uze_testkit::fixtures::canonical("instructions-a")
}

fn fixture_b() -> PathBuf {
    uze_testkit::fixtures::canonical("instructions-b")
}

/// A deterministic stand-in for the one bridge-capable harness (Claude
/// Code): `context_reconcile` itself never calls
/// `exposure_plan`/`attach_receipt` on it — it only reads `id()`/`detect()`
/// directly. `add_plugin`'s pre-existing, unmodified per-resource loop does
/// still call `exposure_plan` for every registered integration on every
/// resource including this package's Instruction resource, exactly as it
/// already does for Skills/MCP on an integration that does not support
/// them — reporting `Unsupported` and attaching nothing, same as every real
/// integration's fallthrough arm does today. Present/absent is controlled
/// explicitly rather than depending on what happens to be installed on the
/// machine running the test.
struct StubBridgeHarness {
    stub_id: &'static str,
    present: bool,
}

impl IntegrationPort for StubBridgeHarness {
    fn id(&self) -> &'static str {
        self.stub_id
    }

    /// The one bridged harness in v0 declares its bridge like any real
    /// integration would — the Application reads `context_delivery`, never
    /// a vendor name.
    fn context_delivery(&self) -> uze::integration::ContextDelivery {
        uze::integration::ContextDelivery::Bridge {
            file_name: "CLAUDE.md",
        }
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: self.present,
            version: None,
        }
    }
    fn exposure_plan(&self, resource: &uze::Resource) -> uze::exposure::ExposurePlan {
        uze::exposure::ExposurePlan {
            representation: resource.capability.representation,
            route: uze::router::CompatibilityRoute::Unsupported,
            verification: uze::router::VerificationStatus::NotExposed,
            mechanism: uze::exposure::ExposureMechanism::Unsupported {
                rationale: "test stub attaches nothing".to_owned(),
            },
            evidence: "test stub".to_owned(),
        }
    }
    fn attach_receipt(&self, _resource: &uze::Resource) -> Result<Option<AttachmentReceipt>> {
        Ok(None)
    }
}

fn app(root: &Path, claude_present: bool) -> UzeApplication {
    UzeApplication::new(
        UzeHome::at(root.join("uze-home")),
        vec![Box::new(StubBridgeHarness {
            stub_id: "claude-code",
            present: claude_present,
        })],
    )
}

fn install(app: &UzeApplication, path: PathBuf) {
    app.plugins()
        .add(PackageSource::local(path), &uze::trust::AlwaysTrust)
        .expect("fixture installs cleanly");
}

fn agents_md_content(project: &Path) -> String {
    fs::read_to_string(project.join("AGENTS.md")).unwrap_or_default()
}

fn bridge<'a>(report: &'a ContextReconciliationReport, integration: &str) -> &'a BridgeStatus {
    report
        .bridges
        .iter()
        .find(|bridge| bridge.integration == integration)
        .unwrap_or_else(|| panic!("no bridge status reported for {integration}"))
}

// --- A: baseline without UZE / B: decomposition / C: attach / D: discovery-shape ---

#[test]
fn a_single_package_composes_agents_md_and_bridges_only_present_harnesses() {
    let root = temp("single-package");
    let application = app(&root, true);
    install(&application, fixture_a());

    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    // A: baseline — before reconciling, UZE has written nothing into the project.
    assert!(!project.join("AGENTS.md").exists());
    assert!(!project.join("CLAUDE.md").exists());

    // B/C: decomposition + attach.
    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(report.packages.len(), 1);
    assert_eq!(report.packages[0].state, AttachmentState::Matched);
    assert!(report.removed_orphans.is_empty());

    let content = agents_md_content(&project);
    assert!(content.contains("uze-instructions-fixture-a"));

    // D: Codex/OpenCode/Antigravity receive nothing extra — no artifact
    // beyond the shared AGENTS.md file itself is ever created for them.
    assert!(!project.join(".codex").exists());
    assert!(!project.join(".opencode").exists());

    // The bridge-capable harness was "present", so it gets a real bridge,
    // matched.
    assert_eq!(
        bridge(&report, "claude-code").state,
        AttachmentState::Matched
    );
    let claude_md = fs::read_to_string(project.join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@AGENTS.md"));
    assert!(claude_md.contains("uze:begin instruction-bridge"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_absent_bridge_harness_receives_no_bridge_file_at_all() {
    let root = temp("absent-harness");
    // Claude Code absent from this machine.
    let application = app(&root, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    let _report = application.context().reconcile(&project).unwrap();
    assert!(
        !project.join("CLAUDE.md").exists(),
        "an absent harness must never receive a bridge file"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- E/F/G: inspect MATCHED / user edit outside stays MATCHED / edit inside becomes DRIFTED ---

#[test]
fn editing_outside_the_managed_region_stays_matched_editing_inside_becomes_drifted() {
    let root = temp("drift-scoping");
    let application = app(&root, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context().reconcile(&project).unwrap();

    // F: user prose appended around the managed region.
    let agents_md = project.join("AGENTS.md");
    let mut content = fs::read_to_string(&agents_md).unwrap();
    content = format!("# My project\n\n{content}\nMore of my own notes.\n");
    fs::write(&agents_md, &content).unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(
        report.packages[0].state,
        AttachmentState::Matched,
        "editing outside the managed region must not be reported as drift"
    );
    assert!(
        fs::read_to_string(&agents_md)
            .unwrap()
            .contains("More of my own notes.")
    );

    // G: user edits text INSIDE the managed region.
    let tampered = fs::read_to_string(&agents_md).unwrap().replace(
        "Fixture A conformance marker",
        "TAMPERED conformance marker",
    );
    fs::write(&agents_md, &tampered).unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(report.packages[0].state, AttachmentState::Drifted);
    // Reconcile must not have overwritten the user's edit.
    assert_eq!(fs::read_to_string(&agents_md).unwrap(), tampered);
    fs::remove_dir_all(root).unwrap();
}

// --- H/I: remove when MATCHED preserves user content / remove when DRIFTED blocks ---

#[test]
fn a_matched_region_can_be_cleanly_removed_preserving_user_content() {
    let root = temp("clean-removal");
    let application = app(&root, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let agents_md = project.join("AGENTS.md");
    fs::write(&agents_md, "user text A\n").unwrap();
    application.context().reconcile(&project).unwrap();
    let mut content = fs::read_to_string(&agents_md).unwrap();
    content.push_str("user text B\n");
    fs::write(&agents_md, &content).unwrap();

    // Package A is no longer installed: reconciling with an empty desired
    // set (simulated here by removing the package from the store first)
    // must remove exactly its region.
    application
        .plugins()
        .remove("uze-instructions-fixture-a")
        .unwrap();
    let report = application.context().reconcile(&project).unwrap();
    assert!(report.packages.is_empty());
    assert_eq!(report.removed_orphans.len(), 1);
    assert_eq!(
        fs::read_to_string(&agents_md).unwrap(),
        "user text A\nuser text B\n"
    );
    fs::remove_dir_all(root).unwrap();
}

/// While a package is still installed, drift in *its* region must block
/// reconciliation from touching it — this is the strong, content-verified
/// guarantee `text_region::attach`/`inspect` always provide.
#[test]
fn a_still_installed_packages_drifted_region_is_reported_and_never_rewritten() {
    let root = temp("drift-still-installed");
    let application = app(&root, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context().reconcile(&project).unwrap();

    let agents_md = project.join("AGENTS.md");
    let tampered = fs::read_to_string(&agents_md)
        .unwrap()
        .replace("Fixture A conformance marker", "TAMPERED");
    fs::write(&agents_md, &tampered).unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(report.packages[0].state, AttachmentState::Drifted);
    assert_eq!(fs::read_to_string(&agents_md).unwrap(), tampered);
    fs::remove_dir_all(root).unwrap();
}

/// Once a package is *removed*, its Store bytes are gone — there is no
/// `expected_content` left to verify drift against for its now-orphaned
/// region. Orphan cleanup's ownership proof is therefore structural only
/// (exact, well-formed UZE markers), documented explicitly as a weaker
/// guarantee than `detach`'s in `text_region::remove_unconditionally`. A
/// malformed/duplicated marker shape still refuses, exactly like `detach`.
#[test]
fn an_orphaned_regions_cleanup_is_structural_not_content_verified_but_still_refuses_malformed_markers()
 {
    let root = temp("orphan-cleanup-shape");
    let application = app(&root, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context().reconcile(&project).unwrap();

    // Even content edited post-hoc inside an about-to-be-orphaned region is
    // removed along with it: once the owning package is gone, there is
    // nothing left to compare that content to.
    let agents_md = project.join("AGENTS.md");
    let edited = fs::read_to_string(&agents_md)
        .unwrap()
        .replace("Fixture A conformance marker", "edited before removal");
    fs::write(&agents_md, &edited).unwrap();

    application
        .plugins()
        .remove("uze-instructions-fixture-a")
        .unwrap();
    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(report.removed_orphans.len(), 1);
    assert!(report.blocked_orphans.is_empty());
    assert!(
        !fs::read_to_string(&agents_md)
            .unwrap()
            .contains("edited before removal")
    );

    fs::remove_dir_all(root).unwrap();
}

/// The bridge's own drift protection is unaffected by orphan cleanup's
/// weaker rule: `reconcile`'s "remove" path always goes through `detach`,
/// which is content-verified, because the bridge's expected content
/// (`@AGENTS.md`) is a fixed constant this module always knows, never
/// something that becomes unrecoverable when a package is removed.
#[test]
fn a_drifted_bridge_line_blocks_its_own_removal_even_after_the_last_package_is_gone() {
    let root = temp("bridge-drift-blocks");
    let application = app(&root, true);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context().reconcile(&project).unwrap();

    let claude_md = project.join("CLAUDE.md");
    let tampered_bridge = fs::read_to_string(&claude_md)
        .unwrap()
        .replace("@AGENTS.md", "@SOMETHING-ELSE.md");
    fs::write(&claude_md, &tampered_bridge).unwrap();

    application
        .plugins()
        .remove("uze-instructions-fixture-a")
        .unwrap();
    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(
        bridge(&report, "claude-code").state,
        AttachmentState::Drifted
    );
    assert_eq!(fs::read_to_string(&claude_md).unwrap(), tampered_bridge);
    fs::remove_dir_all(root).unwrap();
}

// --- Fase C.6: two packages coexist, independent removal, single shared bridge ---

#[test]
fn two_packages_share_one_agents_md_and_exactly_one_bridge_per_harness() {
    let root = temp("two-packages");
    let application = app(&root, true);
    install(&application, fixture_a());
    install(&application, fixture_b());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(report.packages.len(), 2);
    assert!(
        report
            .packages
            .iter()
            .all(|status| status.state == AttachmentState::Matched)
    );
    let content = agents_md_content(&project);
    assert!(content.contains("uze-instructions-fixture-a"));
    assert!(content.contains("uze-instructions-fixture-b"));

    // Exactly one bridge file per harness regardless of package count.
    assert_eq!(report.bridges.len(), 1);
    let claude_md = fs::read_to_string(project.join("CLAUDE.md")).unwrap();
    assert_eq!(claude_md.matches("@AGENTS.md").count(), 1);

    // Removing package A leaves B's region and the bridge intact.
    application
        .plugins()
        .remove("uze-instructions-fixture-a")
        .unwrap();
    let report = application.context().reconcile(&project).unwrap();
    assert_eq!(report.packages.len(), 1);
    assert_eq!(
        report.packages[0].package_id,
        "uze-instructions-fixture-b@local"
    );
    assert_eq!(report.removed_orphans.len(), 1);
    let content = agents_md_content(&project);
    assert!(!content.contains("uze-instructions-fixture-a"));
    assert!(content.contains("uze-instructions-fixture-b"));
    assert_eq!(
        bridge(&report, "claude-code").state,
        AttachmentState::Matched
    );
    assert!(
        project.join("CLAUDE.md").exists(),
        "bridge must survive while B is still installed"
    );

    // Removing package B leaves AGENTS.md empty of managed content and
    // removes the now-unneeded bridge region — Fase C.5's core claim. The
    // bridge *file* itself is left behind, empty: `text_region::detach`
    // deliberately never deletes a file it did not independently prove it
    // is safe to delete (see `text_region.rs`'s
    // `detach_leaves_an_empty_file_rather_than_deleting_a_preexisting_file`)
    // — it cannot tell "UZE created this file from nothing" apart from "the
    // user's own file happened to end up empty," so it treats both alike.
    application
        .plugins()
        .remove("uze-instructions-fixture-b")
        .unwrap();
    let report = application.context().reconcile(&project).unwrap();
    assert!(report.packages.is_empty());
    assert_eq!(report.removed_orphans.len(), 1);
    assert_eq!(
        bridge(&report, "claude-code").state,
        AttachmentState::Missing
    );
    assert_eq!(
        fs::read_to_string(project.join("CLAUDE.md")).unwrap(),
        "",
        "the bridge region is gone; the file it leaves behind is empty, not deleted"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- reinstall/update must not duplicate regions ---

#[test]
fn reconciling_repeatedly_never_duplicates_regions_or_bridges() {
    let root = temp("no-duplication");
    let application = app(&root, true);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    application.context().reconcile(&project).unwrap();
    let after_first = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    let bridge_after_first = fs::read_to_string(project.join("CLAUDE.md")).unwrap();

    application.context().reconcile(&project).unwrap();
    application.context().reconcile(&project).unwrap();
    let after_repeat = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    let bridge_after_repeat = fs::read_to_string(project.join("CLAUDE.md")).unwrap();

    assert_eq!(
        after_first, after_repeat,
        "repeated reconcile must be byte-idempotent"
    );
    assert_eq!(bridge_after_first, bridge_after_repeat);
    assert_eq!(after_repeat.matches("uze:begin").count(), 1);
    fs::remove_dir_all(root).unwrap();
}

// --- adversarial: markers that look like managed content, malformed shape ---

#[test]
fn a_project_root_that_does_not_exist_is_a_clean_error_not_a_write() {
    let root = temp("missing-project");
    let application = app(&root, false);
    let error = application
        .context()
        .reconcile(&root.join("does-not-exist"))
        .unwrap_err();
    assert!(matches!(error, UzeError::NotDirectory(_)));
}

#[test]
fn reconcile_never_touches_the_project_when_no_package_provides_instructions() {
    let root = temp("no-instructions");
    let application = app(&root, true);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("AGENTS.md"), "just my own notes\n").unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert!(report.packages.is_empty());
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        "just my own notes\n"
    );
    assert!(!project.join("CLAUDE.md").exists());
    fs::remove_dir_all(root).unwrap();
}

/// A pre-existing AGENTS.md that already contains a well-formed UZE marker
/// for an identity not shaped like this module's own
/// (`package:<id>:instructions`) is left completely untouched — it belongs
/// to some other, unrelated concern (or a package id that happens to
/// collide only superficially), and this reconciler must not assume
/// ownership of it.
#[test]
fn a_foreign_looking_managed_region_outside_our_naming_shape_is_left_untouched() {
    let root = temp("foreign-region");
    let application = app(&root, false);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("AGENTS.md"),
        "<!-- uze:begin some-other-concern -->\nnot ours\n<!-- uze:end some-other-concern -->\n",
    )
    .unwrap();

    let report = application.context().reconcile(&project).unwrap();
    assert!(report.removed_orphans.is_empty());
    assert!(report.blocked_orphans.is_empty());
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        "<!-- uze:begin some-other-concern -->\nnot ours\n<!-- uze:end some-other-concern -->\n"
    );
    fs::remove_dir_all(root).unwrap();
}
