//! Deterministic contract tests for the official `uze` Skill package
//! (`plugins/uze/`).
//!
//! These are the non-probabilistic half of Fase 12: does the package
//! install through the exact same pipeline as any other package, does it
//! get discovered, does `status`/`context inspect`/`context plan` produce
//! valid JSON. The Skill's actual *reasoning* (whether it makes good
//! decisions about a real project) is not testable this way — see
//! `docs/capabilities/uze-skill.md` for the agentic eval scenarios that
//! cover that half instead.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze::{PackageSource, UzeApplication};
use uze::{
    Result, UzeHome,
    capability::CapabilityKind,
    integration::{AttachmentReceipt, AttachmentState, HarnessDetection, IntegrationPort},
    router::HarnessCapabilities,
};

fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

fn official_package() -> PathBuf {
    uze_testkit::fixtures::official_plugin()
}

/// Mirrors the real integrations only in `id()`/`detect()` — proves the
/// package needs nothing beyond the standard `IntegrationPort` surface any
/// third-party package's Skill already goes through.
struct StubIntegration {
    stub_id: &'static str,
}

impl IntegrationPort for StubIntegration {
    fn id(&self) -> &'static str {
        self.stub_id
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
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

fn app(root: &Path) -> UzeApplication {
    UzeApplication::new(
        UzeHome::at(root.join("uze-home")),
        vec![
            Box::new(StubIntegration {
                stub_id: "claude-code",
            }),
            Box::new(StubIntegration { stub_id: "codex" }),
            Box::new(StubIntegration {
                stub_id: "opencode",
            }),
            Box::new(StubIntegration {
                stub_id: "antigravity",
            }),
        ],
    )
}

// --- installs through the exact same pipeline as any other package --------

#[test]
fn the_official_package_installs_through_the_unmodified_pipeline() {
    let root = temp("install");
    let application = app(&root);
    let report = application
        .add_plugin(
            PackageSource::local(official_package()),
            &uze::trust::AlwaysTrust,
        )
        .expect("the official uze package is a valid Agent Plugins 1.0 package");
    assert_eq!(report.plugin.id, "uze");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_official_package_contributes_exactly_one_agent_skill_resource() {
    let root = temp("resources");
    let application = app(&root);
    application
        .add_plugin(
            PackageSource::local(official_package()),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let inspection = application.inspect_plugin("uze").unwrap();
    assert_eq!(inspection.capabilities.len(), 1);
    assert_eq!(inspection.capabilities[0].kind, CapabilityKind::AgentSkill);
    // The skill's own logical name (its directory, `skills/init`), not the
    // generic `SKILL.md` file name — a display-only read-model improvement.
    assert_eq!(inspection.capabilities[0].name, "init");
    fs::remove_dir_all(root).unwrap();
}

/// The package installs and is discovered without any special-case package
/// id check anywhere in the pipeline — proven structurally by using a
/// *renamed copy* of the exact same package content under a different
/// package id and confirming it behaves identically.
#[test]
fn the_package_receives_no_special_treatment_a_renamed_copy_behaves_identically() {
    let root = temp("no-special-case");
    let renamed = root.join("renamed-copy");
    fs::create_dir_all(renamed.join("skills/init")).unwrap();
    fs::write(
        renamed.join("plugin.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"totally-different-name"}"#,
    )
    .unwrap();
    fs::copy(
        official_package().join("skills/init/SKILL.md"),
        renamed.join("skills/init/SKILL.md"),
    )
    .unwrap();

    let application = app(&root);
    let report = application
        .add_plugin(PackageSource::local(renamed), &uze::trust::AlwaysTrust)
        .expect("an identical package under a different id installs the same way");
    assert_eq!(report.plugin.id, "totally-different-name");
    let inspection = application
        .inspect_plugin("totally-different-name")
        .unwrap();
    assert_eq!(inspection.capabilities.len(), 1);
    assert_eq!(inspection.capabilities[0].kind, CapabilityKind::AgentSkill);
    fs::remove_dir_all(root).unwrap();
}

// --- status / context inspect / context plan produce valid, well-shaped JSON ---

#[test]
fn status_context_inspect_and_context_plan_produce_valid_json() {
    let root = temp("json-shape");
    let application = app(&root);
    application
        .add_plugin(
            PackageSource::local(official_package()),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();

    let status = application.status(&project).unwrap();
    let status_json = serde_json::to_value(&status).unwrap();
    assert!(status_json.get("portability").is_some());
    assert!(status_json.get("harnesses").is_some());

    let inspection = application.context_inspect(&project).unwrap();
    let inspection_json = serde_json::to_value(&inspection).unwrap();
    assert!(inspection_json.get("sources").is_some());

    let plan = application.context_plan(&project).unwrap();
    let plan_json = serde_json::to_value(&plan).unwrap();
    assert!(plan_json.get("agents_md_plan").is_some());
    fs::remove_dir_all(root).unwrap();
}

// --- reconcile continues to respect ownership; drift stays blocked --------

#[test]
fn installing_the_skill_package_never_touches_managed_regions_of_other_packages() {
    let root = temp("ownership-preserved");
    let application = app(&root);
    application
        .add_plugin(
            PackageSource::local(uze_testkit::fixtures::canonical("instructions-a")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context_reconcile(&project).unwrap();
    let before = fs::read_to_string(project.join("AGENTS.md")).unwrap();

    // Installing an unrelated Skill-only package (the official uze skill
    // itself) must not touch the project's AGENTS.md at all — it
    // contributes no Instruction resource.
    application
        .add_plugin(
            PackageSource::local(official_package()),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(project.join("AGENTS.md")).unwrap(),
        before
    );

    // Reconciling again picks up nothing new from the skill package (it has
    // no AGENTS.md contribution) and still respects the existing region.
    let report = application.context_reconcile(&project).unwrap();
    assert_eq!(report.packages.len(), 1);
    assert_eq!(report.packages[0].state, AttachmentState::Matched);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn drift_is_still_blocked_with_the_skill_package_also_installed() {
    let root = temp("drift-still-blocked");
    let application = app(&root);
    application
        .add_plugin(
            PackageSource::local(official_package()),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    application
        .add_plugin(
            PackageSource::local(uze_testkit::fixtures::canonical("instructions-a")),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    application.context_reconcile(&project).unwrap();

    let agents_md = project.join("AGENTS.md");
    let tampered = fs::read_to_string(&agents_md)
        .unwrap()
        .replace("Fixture A conformance marker", "TAMPERED");
    fs::write(&agents_md, &tampered).unwrap();

    let report = application.context_reconcile(&project).unwrap();
    assert_eq!(report.packages[0].state, AttachmentState::Drifted);
    assert_eq!(fs::read_to_string(&agents_md).unwrap(), tampered);
    fs::remove_dir_all(root).unwrap();
}
