//! Shared skill-root projection: Codex and OpenCode share exactly one
//! physical entry per default skill; policy-sensitive skills detect
//! cross-integration policy loss; user-only skills still install cleanly.
//! Migrated from the former `tests/skill_invocation_conformance.rs` (groups
//! 6-7).

#![allow(dead_code)] // helper subset reused across the moved tests

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze::{
    PackageSource, UzeApplication, UzeHome,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{AttachmentInspection, AttachmentReceipt, HarnessDetection, IntegrationPort},
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::HarnessCapabilities,
    store::StoredPackage,
};
use uze_integrations::{codex::CodexIntegration, opencode::OpenCodeIntegration};

use uze_test_support::temp::scratch;

fn temp(label: &str) -> PathBuf {
    scratch(label)
}

fn flow_fixture() -> PathBuf {
    uze_test_support::fixtures::canonical("flow")
}

fn user_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Explicit user action\ninvoke:\n  model: false\n  user: true\n---\n\nExplicit body.\n"
    )
}

fn model_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Background knowledge\ninvoke:\n  model: true\n  user: false\n---\n\nBody.\n"
    )
}

fn default_body(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Interactive skill\n---\n\nBody.\n")
}

fn invalid_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Uninvokable\ninvoke:\n  model: false\n  user: false\n---\n\nBody.\n"
    )
}

/// Never spawns a process; the fake Codex (below) stands in only when a
/// generated envelope's `attach_package` shells out.
struct NoopProcessRunner;

impl ProcessRunner for NoopProcessRunner {
    fn run(&self, _spec: &ProcessSpec) -> uze::Result<ProcessResult> {
        Ok(ProcessResult {
            success: true,
            timed_out: false,
        })
    }
}

fn always_present<T: IntegrationPort>(integration: T) -> AlwaysPresent<T> {
    AlwaysPresent(integration)
}

struct AlwaysPresent<T: IntegrationPort>(T);

impl<T: IntegrationPort> IntegrationPort for AlwaysPresent<T> {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        self.0.capabilities()
    }
    fn exposure_plan(&self, resource: &ProjectResource) -> ExposurePlan {
        self.0.exposure_plan(resource)
    }
    fn exposure_name_candidates(&self, resource: &ProjectResource) -> Vec<String> {
        self.0.exposure_name_candidates(resource)
    }
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        self.0.shared_agent_skill_root()
    }
    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&ProjectResource],
    ) -> Option<PackageExposurePlan> {
        self.0.package_exposure_plan(package, resources)
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: Some("9.9.9".to_owned()),
        }
    }
    fn provision(
        &self,
        _runner: &dyn ProcessRunner,
    ) -> uze::Result<uze::provisioning::ProvisioningResult> {
        Ok(uze::provisioning::ProvisioningResult::verified(
            uze::provisioning::ProvisionAction::None,
            "test-always-present",
            self.detect(),
        ))
    }
    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> uze::Result<()> {
        self.0.install(home, detection)
    }
    fn status(&self, home: &UzeHome) -> uze::integration::IntegrationStatus {
        self.0.status(home)
    }
    fn attach(&self, resource: &ProjectResource) -> uze::Result<Option<PathBuf>> {
        self.0.attach(resource)
    }
    fn attach_package(
        &self,
        package: &StoredPackage,
        plan: &PackageExposurePlan,
    ) -> uze::Result<Option<AttachmentReceipt>> {
        self.0.attach_package(package, plan)
    }
    fn attach_receipt(&self, resource: &ProjectResource) -> uze::Result<Option<AttachmentReceipt>> {
        self.0.attach_receipt(resource)
    }
    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        self.0.inspect_receipt(receipt)
    }
    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> uze::Result<AttachmentInspection> {
        self.0.detach_receipt(receipt)
    }
}

/// A fake `codex` that answers every plugin CLI call successfully.
#[cfg(unix)]
fn fake_codex_bin_dir(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join("fake-bin");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    dir
}

#[cfg(unix)]
fn with_fake_codex(root: &Path, f: impl FnOnce()) {
    let fake_bin = fake_codex_bin_dir(root);
    let mut scope = uze_test_support::env::scope();
    scope.set("PATH", uze_test_support::temp::path_prefixed(&fake_bin));
    f();
}

#[test]
#[cfg(unix)]
fn codex_and_opencode_reuse_one_physical_entry_for_a_default_skill() {
    with_fake_codex(&temp("shared-default"), || {
        let root = temp("shared-default");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        application
            .add_plugin(
                PackageSource::local(flow_fixture()),
                &uze::trust::AlwaysTrust,
            )
            .expect("a default Skill installs cleanly with both harnesses");
        let entry = agents_home.join("skills/flow:commit");
        assert!(entry.is_symlink());
        let target = fs::read_link(&entry).unwrap();
        assert!(
            target.is_dir() && target.join("SKILL.md").exists(),
            "one physical entry owned by the first attacher, reused by both"
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn model_only_skill_shared_root_detects_cross_integration_policy_loss() {
    // A model-only canonical Skill is NOT covered by Codex's generated
    // package envelope (user=false cannot be enforced there), so Codex
    // attaches it capability-level into `~/.agents/skills/flow:legacy`.
    // OpenCode then REUSES that shared entry but needs its own `slash:
    // false` encoding; silently reusing Codex's wrapper would drop the
    // policy, so the second attacher must fail deterministically with a
    // ProjectionConflict — never a silent semantic degradation (ADR-030
    // §25).
    with_fake_codex(&temp("shared-model-only"), || {
        let root = temp("shared-model-only");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/legacy")).unwrap();
        fs::write(
            fixture_root.join("skills/legacy/SKILL.md"),
            model_only_body("legacy"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"model-only fixture"}"#,
        )
        .unwrap();

        let result = application.add_plugin(
            PackageSource::local(&fixture_root),
            &uze::trust::AlwaysTrust,
        );
        match result {
            Err(uze::UzeError::ProjectionConflict(details)) => {
                assert!(
                    details.entry.ends_with("skills/flow:legacy"),
                    "the conflict is about the shared physical entry: {}",
                    details.entry.display()
                );
                assert!(
                    details.requested_integration == "opencode"
                        || details.existing_integration == "opencode",
                    "OpenCode is the integration whose invocation encoding is at stake"
                );
            }
            Err(other) => panic!("expected a deterministic ProjectionConflict, got {other:#?}"),
            Ok(_) => panic!(
                "reusing an incompatible wrapper must not silently drop the invocation policy"
            ),
        }
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn user_only_skill_installs_cleanly_with_codex_and_opencode() {
    // A user-only Skill is covered by Codex's generated package envelope
    // (policy sidecar) and attached by OpenCode as a wrapper with
    // `metadata.opencode/autoinvoke: false` — both harnesses get their
    // native encoding, and the shared root carries exactly one entry.
    with_fake_codex(&temp("shared-user-only"), || {
        let root = temp("shared-user-only");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/review")).unwrap();
        fs::write(
            fixture_root.join("skills/review/SKILL.md"),
            user_only_body("review"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"user-only fixture"}"#,
        )
        .unwrap();

        application
            .add_plugin(
                PackageSource::local(&fixture_root),
                &uze::trust::AlwaysTrust,
            )
            .expect("each harness gets its own native encoding for a user-only Skill");
        let entry = agents_home.join("skills/flow:review");
        assert!(entry.is_symlink(), "OpenCode projects the shared entry");
        let target = fs::read_link(&entry).unwrap();
        let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
        assert!(
            wrapper.contains("opencode/autoinvoke: false"),
            "the shared entry carries OpenCode's invocation encoding"
        );
        fs::remove_dir_all(root).unwrap();
    });
}
