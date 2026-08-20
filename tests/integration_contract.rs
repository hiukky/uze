use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{
    UzeEngine, UzeHome, UzeStore,
    capability::{CapabilityKind, Representation},
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{IntegrationPort, assess_environment},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
};

#[path = "../src/integrations/claude.rs"]
mod claude;
#[path = "../src/integrations/codex.rs"]
mod codex;
#[path = "../src/integrations/opencode.rs"]
mod opencode;

use claude::ClaudeIntegration;
use codex::CodexIntegration;
use opencode::OpenCodeIntegration;

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-plugin-package")
}

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

fn stored_environment(label: &str) -> (PathBuf, uze::EffectiveEnvironment) {
    let root = temporary_home(label);
    let store = UzeStore::new(UzeHome::at(&root));
    let package = store.install_agent_plugin(package_fixture()).unwrap();
    let environment = UzeEngine::new(store).compose(&[package.id]).unwrap();
    (root, environment)
}

#[test]
fn peer_integrations_choose_exposure_without_converting_one_standard_skill() {
    let (home_root, environment) = stored_environment("integration-contract");
    let claude = ClaudeIntegration;
    let codex = CodexIntegration;
    let opencode = OpenCodeIntegration;

    let resource = environment.resources.first().unwrap();
    assert_eq!(resource.capability.representation, Representation::Standard);
    assert!(resource.package_root().is_some());

    let claude_skill = assess_environment(&environment, &claude).pop().unwrap();
    assert_eq!(claude_skill.decision.route, CompatibilityRoute::Adaptable);
    assert_eq!(
        claude_skill.decision.verification,
        VerificationStatus::Unverified
    );
    assert!(matches!(
        claude_skill.exposure_plan.mechanism,
        ExposureMechanism::RuntimeBridge { .. }
    ));

    let codex_skill = assess_environment(&environment, &codex).pop().unwrap();
    assert_eq!(codex_skill.decision.route, CompatibilityRoute::Adaptable);
    assert_eq!(
        codex_skill.decision.verification,
        VerificationStatus::Unverified
    );
    assert!(matches!(
        codex_skill.exposure_plan.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));

    let opencode_skill = assess_environment(&environment, &opencode).pop().unwrap();
    assert_eq!(opencode_skill.decision.route, CompatibilityRoute::Adaptable);
    assert!(matches!(
        opencode_skill.exposure_plan.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));

    fs::remove_dir_all(home_root).unwrap();
}

struct FakeIntegration {
    id: &'static str,
}

impl IntegrationPort for FakeIntegration {
    fn id(&self) -> &'static str {
        self.id
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: BTreeSet::from([CapabilityKind::AgentSkill]),
            evidence: "fake contract evidence".to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn exposure_plan(&self, resource: &uze::Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::DirectNative {
                resource_path: resource.capability.path.clone(),
            },
            evidence: "fake direct exposure".to_owned(),
        }
    }
}

#[test]
fn a_new_peer_integration_needs_no_core_change() {
    let (home_root, environment) = stored_environment("fake-integration");
    let cursor = FakeIntegration { id: "cursor" };
    let skill = assess_environment(&environment, &cursor).pop().unwrap();

    assert_eq!(skill.decision.route, CompatibilityRoute::Native);
    assert_eq!(skill.integration_id, "cursor");
    assert!(matches!(
        skill.exposure_plan.mechanism,
        ExposureMechanism::DirectNative { .. }
    ));

    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn package_store_and_effective_environment_preserve_the_same_skill_bytes() {
    let (home_root, environment) = stored_environment("byte-preservation");
    let resource = environment.resources.first().unwrap();
    let packaged_skill = package_fixture().join("skills/uze-e2e/SKILL.md");

    assert_eq!(
        fs::read(&resource.capability.path).unwrap(),
        fs::read(packaged_skill).unwrap()
    );

    fs::remove_dir_all(home_root).unwrap();
}
