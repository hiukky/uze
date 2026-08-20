use std::{collections::BTreeSet, path::PathBuf};

use uze::{
    capability::{CapabilityKind, Representation},
    integration::{IntegrationPort, assess_environment},
    project::resolve_project,
    router::{CompatibilityRoute, ExposureState, HarnessCapabilities},
};

#[path = "../src/integrations/claude.rs"]
mod claude;
#[path = "../src/integrations/codex.rs"]
mod codex;

use claude::ClaudeIntegration;
use codex::CodexIntegration;

fn fixture_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/portable-project")
}

#[test]
fn peer_integrations_route_one_standard_skill_without_conversion() {
    let environment = resolve_project(fixture_project()).unwrap();
    let claude = ClaudeIntegration;
    let codex = CodexIntegration;

    for integration in [&claude as &dyn IntegrationPort, &codex] {
        let assessment = assess_environment(&environment, integration);
        let skill = assessment
            .iter()
            .find(|item| item.capability_path.ends_with("SKILL.md"))
            .unwrap();
        assert_eq!(skill.decision.route, CompatibilityRoute::Native);
        assert_eq!(skill.decision.exposure, ExposureState::Unverified);
    }

    assert_eq!(
        environment
            .project_resources
            .iter()
            .filter(|resource| resource.kind == CapabilityKind::AgentSkill)
            .count(),
        1
    );
    assert_eq!(
        environment
            .project_resources
            .iter()
            .find(|resource| resource.kind == CapabilityKind::AgentSkill)
            .unwrap()
            .representation,
        Representation::Standard
    );
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
}

#[test]
fn a_new_peer_integration_needs_no_core_change() {
    let environment = resolve_project(fixture_project()).unwrap();
    let cursor = FakeIntegration { id: "cursor" };
    let assessment = assess_environment(&environment, &cursor);
    let skill = assessment
        .iter()
        .find(|item| item.capability_path.ends_with("SKILL.md"))
        .unwrap();

    assert_eq!(skill.decision.route, CompatibilityRoute::Native);
    assert_eq!(skill.integration_id, "cursor");
}
