//! L1 contract for the read-only Context Manager surface:
//! `UzeApplication::context_inspect`/`context_plan`.
//!
//! The central proof this file exists for: inspection and planning are
//! genuinely zero-write, in every state a real project can be in, including
//! states UZE never created (Fase 6 — a project that already has its own
//! CLAUDE.md/GEMINI.md/AGENTS.md, written entirely by hand, long before UZE
//! ever touched it).

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{PackageSource, UzeApplication};
use uze::{
    Result, UzeHome,
    application::{HarnessContextDelivery, Portability, ProjectContextStatus},
    integration::{AttachmentReceipt, AttachmentState, HarnessDetection, IntegrationPort},
    router::HarnessCapabilities,
};

// --- uze status: thin composition, distinct scope from doctor -------------

#[test]
fn status_reports_healthy_with_zero_issues_once_reconciled() {
    let root = temp("status-healthy");
    let application = app(&root, true, true);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context_reconcile(&project).unwrap();

    let status = application.status(&project).unwrap();
    assert!(matches!(status.portability, Portability::Portable));
    assert_eq!(status.packages_installed, 1);
    assert_eq!(status.packages_contributing_here, 1);
    assert!(status.issues.is_empty());
}

#[test]
fn status_surfaces_a_missing_bridge_as_an_issue_before_reconcile() {
    let root = temp("status-issue");
    let application = app(&root, true, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    let status = application.status(&project).unwrap();
    assert!(!status.issues.is_empty());
    assert!(status.issues.iter().any(|issue| issue.contains("Missing")));
}

#[test]
fn status_distinguishes_installed_from_contributing_here() {
    let root = temp("status-counts");
    let application = app(&root, false, false);
    install(&application, fixture_a());
    // A second, Skill-only package: installed globally, but contributes no
    // Instruction resource, so it must not count as "contributing here".
    install(
        &application,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packages/uze"),
    );
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context_reconcile(&project).unwrap();

    let status = application.status(&project).unwrap();
    assert_eq!(status.packages_installed, 2);
    assert_eq!(status.packages_contributing_here, 1);
}

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-context-inspect-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn fixture_a() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/packages/agent-plugin-instructions-a")
}

struct StubBridgeHarness {
    stub_id: &'static str,
    present: bool,
}

impl IntegrationPort for StubBridgeHarness {
    fn id(&self) -> &'static str {
        self.stub_id
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

fn app(root: &Path, claude_present: bool, gemini_present: bool) -> UzeApplication {
    UzeApplication::new(
        UzeHome::at(root.join("uze-home")),
        vec![
            Box::new(StubBridgeHarness {
                stub_id: "claude-code",
                present: claude_present,
            }),
            Box::new(StubBridgeHarness {
                stub_id: "gemini",
                present: gemini_present,
            }),
        ],
    )
}

fn install(app: &UzeApplication, path: PathBuf) {
    app.add_plugin(PackageSource::local(path), &uze::trust::AlwaysTrust)
        .expect("fixture installs cleanly");
}

/// Every byte, of every file, anywhere under `dir` — the strongest
/// "nothing changed" proof available: not just AGENTS.md, not just the
/// files this suite happens to check, but the whole directory tree.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.filter_map(std::result::Result::ok).collect();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else if let Ok(bytes) = fs::read(&path) {
                out.push((path.strip_prefix(root).unwrap().to_path_buf(), bytes));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

fn harness_delivery<'a>(
    status: &'a ProjectContextStatus,
    integration: &str,
) -> &'a HarnessContextDelivery {
    &status
        .harnesses
        .iter()
        .find(|harness| harness.integration == integration)
        .unwrap_or_else(|| panic!("no harness status for {integration}"))
        .delivery
}

// --- Fase 2: inspect/plan are genuinely zero-write -------------------------

#[test]
fn context_inspect_never_writes_anything_in_a_populated_project() {
    let root = temp("inspect-snapshot");
    let application = app(&root, true, true);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    // Reconcile once so there's real managed state to inspect.
    application.context_reconcile(&project).unwrap();

    let before = snapshot(&project);
    let status = application.context_inspect(&project).unwrap();
    let after = snapshot(&project);
    assert_eq!(
        before, after,
        "context_inspect must not create, delete, or modify any file"
    );
    assert_eq!(status.contributions[0].state, AttachmentState::Matched);
}

#[test]
fn context_plan_never_writes_anything() {
    let root = temp("plan-snapshot");
    let application = app(&root, true, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    // Before any reconcile at all — the state with the most "would create"
    // actions, and therefore the state most tempting to accidentally write.
    let before = snapshot(&project);
    let plan = application.context_plan(&project).unwrap();
    let after = snapshot(&project);
    assert_eq!(
        before, after,
        "context_plan must not write anything even when everything is missing"
    );
    assert!(plan.has_changes());
}

// --- Fase 3: portability observation ---------------------------------------

#[test]
fn a_project_with_only_claude_md_is_vendor_locked() {
    let root = temp("claude-only");
    let application = app(&root, true, false);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("CLAUDE.md"), "# My Claude-only instructions\n").unwrap();

    let before = snapshot(&project);
    let status = application.context_inspect(&project).unwrap();
    assert_eq!(snapshot(&project), before, "inspect must not write");

    assert!(matches!(
        status.portability,
        Portability::VendorLocked { .. }
    ));
    if let Portability::VendorLocked { files } = &status.portability {
        assert_eq!(files, &[project.join("CLAUDE.md")]);
    }
}

#[test]
fn agents_md_plus_a_bridging_claude_md_is_portable() {
    let root = temp("portable");
    let application = app(&root, true, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context_reconcile(&project).unwrap();

    let status = application.context_inspect(&project).unwrap();
    assert!(matches!(status.portability, Portability::Portable));
    assert!(matches!(
        harness_delivery(&status, "claude-code"),
        HarnessContextDelivery::Bridge {
            needed: true,
            state: AttachmentState::Matched
        }
    ));
}

#[test]
fn claude_and_gemini_with_different_content_and_no_agents_md_warn_about_divergence() {
    let root = temp("divergent-vendor");
    let application = app(&root, true, true);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("CLAUDE.md"), "Claude-specific instructions.\n").unwrap();
    fs::write(
        project.join("GEMINI.md"),
        "Totally different Gemini instructions.\n",
    )
    .unwrap();

    let status = application.context_inspect(&project).unwrap();
    assert!(matches!(
        status.portability,
        Portability::VendorLocked { .. }
    ));
    if let Portability::VendorLocked { files } = &status.portability {
        assert_eq!(files.len(), 2);
    }
    assert!(
        status
            .warnings
            .iter()
            .any(|warning| warning.contains("divergent")),
        "expected a divergence warning, got: {:?}",
        status.warnings
    );
}

#[test]
fn an_absent_bridge_harness_shows_not_detected_not_a_gap() {
    let root = temp("not-detected");
    let application = app(&root, true, false); // Gemini absent
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context_reconcile(&project).unwrap();

    let status = application.context_inspect(&project).unwrap();
    assert!(matches!(
        harness_delivery(&status, "gemini"),
        HarnessContextDelivery::NotDetected
    ));
    // An absent harness must never turn Portable into PartiallyPortable.
    assert!(matches!(status.portability, Portability::Portable));
}

// --- Fase 6: real projects with pre-existing, hand-written files ----------

/// A) CLAUDE.md with manual content, no AGENTS.md.
#[test]
fn scenario_a_manual_claude_md_survives_untouched() {
    let root = temp("scenario-a");
    let application = app(&root, true, false);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("CLAUDE.md"),
        "My hand-written Claude instructions.\n",
    )
    .unwrap();

    let before = fs::read(project.join("CLAUDE.md")).unwrap();
    application.context_inspect(&project).unwrap();
    assert_eq!(fs::read(project.join("CLAUDE.md")).unwrap(), before);

    let status = application.context_inspect(&project).unwrap();
    let claude_source = status
        .sources
        .iter()
        .find(|s| s.file_name == "CLAUDE.md")
        .unwrap();
    assert!(claude_source.has_user_content);
    assert!(claude_source.managed_region_identities.is_empty());
}

/// B) GEMINI.md with manual content, no AGENTS.md.
#[test]
fn scenario_b_manual_gemini_md_survives_untouched() {
    let root = temp("scenario-b");
    let application = app(&root, false, true);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("GEMINI.md"),
        "My hand-written Gemini instructions.\n",
    )
    .unwrap();

    let before = fs::read(project.join("GEMINI.md")).unwrap();
    application.context_inspect(&project).unwrap();
    assert_eq!(fs::read(project.join("GEMINI.md")).unwrap(), before);
}

/// C) CLAUDE.md + GEMINI.md, different manual content, no AGENTS.md.
#[test]
fn scenario_c_two_divergent_manual_files_both_survive() {
    let root = temp("scenario-c");
    let application = app(&root, true, true);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("CLAUDE.md"), "Claude notes.\n").unwrap();
    fs::write(project.join("GEMINI.md"), "Gemini notes, unrelated.\n").unwrap();

    let before = snapshot(&project);
    application.context_inspect(&project).unwrap();
    application.context_plan(&project).unwrap();
    assert_eq!(snapshot(&project), before);
}

/// D) AGENTS.md manual, already existing, no packages installed.
#[test]
fn scenario_d_manual_agents_md_with_no_packages_is_left_alone() {
    let root = temp("scenario-d");
    let application = app(&root, false, false);
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("AGENTS.md"), "My own project conventions.\n").unwrap();

    let before = fs::read(project.join("AGENTS.md")).unwrap();
    let status = application.context_inspect(&project).unwrap();
    assert_eq!(fs::read(project.join("AGENTS.md")).unwrap(), before);
    let agents_source = status
        .sources
        .iter()
        .find(|s| s.file_name == "AGENTS.md")
        .unwrap();
    assert!(agents_source.has_user_content);
    assert!(agents_source.managed_region_identities.is_empty());
    assert!(status.contributions.is_empty());

    // Reconcile must also leave it alone: no packages means nothing to add.
    application.context_reconcile(&project).unwrap();
    assert_eq!(fs::read(project.join("AGENTS.md")).unwrap(), before);
}

/// E) AGENTS.md manual + UZE regions coexisting.
#[test]
fn scenario_e_manual_agents_md_plus_uze_region_coexist() {
    let root = temp("scenario-e");
    let application = app(&root, false, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("AGENTS.md"),
        "# My own conventions\n\nAlways write tests.\n",
    )
    .unwrap();

    application.context_reconcile(&project).unwrap();
    let content = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    assert!(content.starts_with("# My own conventions\n\nAlways write tests.\n"));
    assert!(content.contains("uze-instructions-fixture-a"));

    let status = application.context_inspect(&project).unwrap();
    let agents_source = status
        .sources
        .iter()
        .find(|s| s.file_name == "AGENTS.md")
        .unwrap();
    assert!(
        agents_source.has_user_content,
        "manual conventions are still there"
    );
    assert_eq!(agents_source.managed_region_identities.len(), 1);
}

/// F) CLAUDE.md manual + UZE bridge coexisting.
#[test]
fn scenario_f_manual_claude_md_content_plus_bridge_coexist() {
    let root = temp("scenario-f");
    let application = app(&root, true, false);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("CLAUDE.md"),
        "## My personal Claude workflow notes\n",
    )
    .unwrap();

    application.context_reconcile(&project).unwrap();
    let content = fs::read_to_string(project.join("CLAUDE.md")).unwrap();
    assert!(content.starts_with("## My personal Claude workflow notes\n"));
    assert!(content.contains("@AGENTS.md"));

    let status = application.context_inspect(&project).unwrap();
    let claude_source = status
        .sources
        .iter()
        .find(|s| s.file_name == "CLAUDE.md")
        .unwrap();
    assert!(
        claude_source.has_user_content,
        "the hand-written notes are still there, outside the bridge region"
    );
    assert_eq!(
        claude_source.managed_region_identities,
        vec!["instruction-bridge".to_owned()]
    );
    // And no automatic migration ever happened: CLAUDE.md's own prose was
    // never copied into AGENTS.md, and AGENTS.md holds only the package's
    // own contribution.
    let agents_content = fs::read_to_string(project.join("AGENTS.md")).unwrap();
    assert!(!agents_content.contains("personal Claude workflow"));
}

/// All three recognized files present at once, fully reconciled: exactly
/// the "everything together" state Fase 10 asks for, on top of the
/// per-scenario A–F coverage above.
#[test]
fn all_three_recognized_files_together_are_fully_portable() {
    let root = temp("all-three");
    let application = app(&root, true, true);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("CLAUDE.md"),
        "## My Claude-only workflow notes\n",
    )
    .unwrap();
    fs::write(
        project.join("GEMINI.md"),
        "## My Gemini-only workflow notes\n",
    )
    .unwrap();

    application.context_reconcile(&project).unwrap();
    let status = application.context_inspect(&project).unwrap();

    assert!(matches!(status.portability, Portability::Portable));
    for file_name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
        let source = status
            .sources
            .iter()
            .find(|s| s.file_name == file_name)
            .unwrap();
        assert!(source.exists);
    }
    assert!(
        fs::read_to_string(project.join("CLAUDE.md"))
            .unwrap()
            .contains("My Claude-only workflow notes")
    );
    assert!(
        fs::read_to_string(project.join("GEMINI.md"))
            .unwrap()
            .contains("My Gemini-only workflow notes")
    );
    // Two warnings expected: CLAUDE.md and GEMINI.md each carry content
    // beyond their bridge, which is legitimate and disclosed, not a gap.
    assert_eq!(status.warnings.len(), 2);
}

// --- context operations never touch the Store -------------------------------

#[test]
fn context_operations_never_alter_the_installed_package_set() {
    let root = temp("store-untouched");
    let application = app(&root, true, true);
    install(&application, fixture_a());
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    let before = application.list_plugins().unwrap();
    application.context_inspect(&project).unwrap();
    application.context_plan(&project).unwrap();
    application.context_reconcile(&project).unwrap();
    let after = application.list_plugins().unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before[0].id, after[0].id);
}
