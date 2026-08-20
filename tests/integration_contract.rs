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
#[path = "../src/integrations/opencode.rs"]
mod opencode;

use claude::ClaudeIntegration;
use codex::CodexIntegration;
use opencode::OpenCodeIntegration;

fn fixture_project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-skill-conformance")
}

#[test]
fn peer_integrations_route_one_standard_skill_without_conversion() {
    let environment = resolve_project(fixture_project()).unwrap();
    let claude = ClaudeIntegration;
    let codex = CodexIntegration;
    let opencode = OpenCodeIntegration;

    let claude_assessment = assess_environment(&environment, &claude);
    let claude_skill = claude_assessment
        .iter()
        .find(|item| item.capability_path.ends_with("SKILL.md"))
        .unwrap();
    assert_eq!(claude_skill.decision.route, CompatibilityRoute::Unsupported);
    assert_eq!(claude_skill.decision.exposure, ExposureState::Unverified);

    let codex_assessment = assess_environment(&environment, &codex);
    let codex_skill = codex_assessment
        .iter()
        .find(|item| item.capability_path.ends_with("SKILL.md"))
        .unwrap();
    assert_eq!(codex_skill.decision.route, CompatibilityRoute::Native);
    assert_eq!(codex_skill.decision.exposure, ExposureState::Verified);

    let opencode_assessment = assess_environment(&environment, &opencode);
    let opencode_skill = opencode_assessment
        .iter()
        .find(|item| item.capability_path.ends_with("SKILL.md"))
        .unwrap();
    assert_eq!(opencode_skill.decision.route, CompatibilityRoute::Native);
    assert_eq!(opencode_skill.decision.exposure, ExposureState::Unverified);

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

#[test]
fn the_project_view_reuses_the_agent_plugin_skill_bytes() {
    let root = fixture_project();
    let environment = resolve_project(&root).unwrap();
    let resolved_skill = environment
        .project_resources
        .iter()
        .find(|resource| resource.kind == CapabilityKind::AgentSkill)
        .unwrap()
        .path
        .canonicalize()
        .unwrap();
    let packaged_skill = root.join("skills/uze-e2e/SKILL.md").canonicalize().unwrap();

    assert_eq!(resolved_skill, packaged_skill);
}
