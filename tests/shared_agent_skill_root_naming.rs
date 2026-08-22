//! Regression coverage for the cross-harness naming collision fixed
//! alongside `IntegrationPort::shared_agent_skill_root`: OpenCode, Codex,
//! and Gemini CLI all discover Agent Skills from the same physical
//! `~/.agents/skills` directory. Before the fix, OpenCode's own
//! `short_then_qualified` naming policy (bare name first) and Codex/Gemini's
//! always-qualified default policy each computed a name independently, so
//! installing a package attached to all three left *two* symlinks for the
//! identical skill sitting in that one shared folder — visible to OpenCode,
//! which scans the whole directory, as a duplicate `/uze` and `/uze-uze`
//! slash command.
//!
//! Deterministic by construction: a `NoopProcessRunner` means no real
//! `opencode`/`codex`/`gemini` binary is ever spawned. Detection is forced
//! to present via an `AlwaysPresent` wrapper so the suite does not depend on
//! host binaries.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::integrations::{
    codex::CodexIntegration, gemini::GeminiIntegration, opencode::OpenCodeIntegration,
};
use uze::{
    PackageSource, UzeApplication, UzeHome,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentReceipt, HarnessDetection, IntegrationPort,
        IntegrationStatus, PublicationStatus,
    },
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::HarnessCapabilities,
    store::StoredPackage,
};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-shared-skill-root-{label}-{}-{nonce}",
        std::process::id()
    ))
}

struct NoopProcessRunner;

impl ProcessRunner for NoopProcessRunner {
    fn run(&self, _spec: &ProcessSpec) -> uze::Result<ProcessResult> {
        Ok(ProcessResult {
            success: true,
            timed_out: false,
        })
    }
}

struct AlwaysPresent<T: IntegrationPort>(T);

impl<T: IntegrationPort> IntegrationPort for AlwaysPresent<T> {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        self.0.capabilities()
    }
    fn runtime_support(&self) -> uze::runtime::RuntimeSupport {
        self.0.runtime_support()
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
        runner: &dyn ProcessRunner,
    ) -> uze::Result<uze::provisioning::ProvisioningResult> {
        self.0.provision(runner)
    }
    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> uze::Result<()> {
        self.0.install(home, detection)
    }
    fn status(&self, home: &UzeHome) -> IntegrationStatus {
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
    fn aliases(&self) -> &'static [&'static str] {
        self.0.aliases()
    }
    fn republish_packages(&self, packages: &[StoredPackage]) -> uze::Result<()> {
        self.0.republish_packages(packages)
    }
    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        self.0.publication(packages)
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

fn skill_fixture(root: &Path, package_id: &str, skill_name: &str) -> PathBuf {
    let dir = root.join(package_id);
    fs::create_dir_all(dir.join("skills").join(skill_name)).unwrap();
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"{package_id}"}}"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join("skills").join(skill_name).join("SKILL.md"),
        format!("---\nname: {skill_name}\ndescription: test fixture.\n---\n\nBody.\n"),
    )
    .unwrap();
    dir
}

#[test]
fn opencode_codex_and_gemini_share_exactly_one_symlink_for_the_same_skill() {
    let root = temp("three-harness");
    let agents_home = root.join("agents-home");
    let uze_home = UzeHome::at(root.join("uze-home"));

    // Codex and Gemini registered *before* OpenCode, deliberately: both use
    // the always-qualified default policy in isolation, so if attach order
    // alone decided the group's name, whichever of them resolves first
    // would lock the shared folder onto "acme-review" before OpenCode ever
    // got a chance to try its own preferred bare name. The fix must
    // converge on OpenCode's preference regardless of this order.
    let application = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![
            Box::new(AlwaysPresent(CodexIntegration::new(
                agents_home.clone(),
                uze_home.clone(),
            ))),
            Box::new(AlwaysPresent(GeminiIntegration::new(
                agents_home.clone(),
                uze_home.clone(),
            ))),
            Box::new(AlwaysPresent(OpenCodeIntegration::new(
                agents_home.clone(),
                root.join("opencode-config.json"),
                uze_home.clone(),
            ))),
        ],
        Box::new(NoopProcessRunner),
    );

    let package_dir = skill_fixture(&root.join("fixtures"), "acme", "review");
    application
        .add_plugin(PackageSource::local(package_dir), &uze::trust::AlwaysTrust)
        .unwrap();

    let skills_dir = agents_home.join("skills");
    let entries: Vec<String> = fs::read_dir(&skills_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["review".to_owned()],
        "exactly one physical entry must exist for the one skill shared by \
         opencode/codex/gemini in ~/.agents/skills, not one per harness, and \
         it must be OpenCode's preferred bare name even though Codex and \
         Gemini attach first: {entries:?}"
    );

    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(
        inspection.managed_state.matched, 3,
        "all three harnesses must still each have a matched receipt, even \
         though they share one physical artifact"
    );

    fs::remove_dir_all(root).unwrap();
}
