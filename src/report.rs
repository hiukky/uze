use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    capability::{CapabilityKind, Representation},
    integration::{IntegrationPort, assess_environment},
    project::EffectiveEnvironment,
    router::{CompatibilityRoute, VerificationStatus},
    runtime::{RuntimeIntegration, select_runtime_integration},
};

#[derive(Clone, Debug, Serialize)]
pub struct CompatibilityReport {
    pub project_root: String,
    pub effective_resources: Vec<ReportItem>,
    pub integrations: BTreeMap<String, IntegrationReport>,
    pub standards_coverage: Vec<StandardsCoverage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportItem {
    pub name: String,
    pub path: String,
    pub kind: CapabilityKind,
    pub representation: Representation,
}

#[derive(Clone, Debug, Serialize)]
pub struct IntegrationReport {
    pub routes: Vec<CapabilityRouteReport>,
    pub runtime_integration: RuntimeIntegration,
}

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityRouteReport {
    pub capability_path: String,
    pub route: CompatibilityRoute,
    pub verification: VerificationStatus,
    pub rationale: String,
    pub evidence: String,
    pub exposure_plan: crate::exposure::ExposurePlan,
}

#[derive(Clone, Debug, Serialize)]
pub struct StandardsCoverage {
    pub concern: String,
    pub standard: Option<String>,
    pub coverage: String,
    pub remaining_gap: String,
}

pub fn build_report(
    environment: &EffectiveEnvironment,
    integrations: &[&dyn IntegrationPort],
) -> CompatibilityReport {
    let integrations = integrations
        .iter()
        .map(|integration| {
            let assessment = assess_environment(environment, *integration);
            let routes = assessment
                .into_iter()
                .map(|assessment| CapabilityRouteReport {
                    capability_path: assessment.capability_path,
                    route: assessment.decision.route,
                    verification: assessment.decision.verification,
                    rationale: assessment.decision.rationale,
                    evidence: assessment.decision.evidence,
                    exposure_plan: assessment.exposure_plan,
                })
                .collect();
            (
                integration.id().to_owned(),
                IntegrationReport {
                    routes,
                    runtime_integration: select_runtime_integration(&integration.runtime_support()),
                },
            )
        })
        .collect();

    CompatibilityReport {
        project_root: environment.root.to_string_lossy().into_owned(),
        effective_resources: environment
            .resources
            .iter()
            .map(|resource| report_item(environment, resource))
            .collect(),
        integrations,
        standards_coverage: standards_coverage(),
    }
}

pub fn render_text(report: &CompatibilityReport) -> String {
    let mut output = format!(
        "UZE compatibility report\nProject: {}\n\nEffective resources\n",
        report.project_root
    );
    if report.effective_resources.is_empty() {
        output.push_str("- none discovered\n");
    }
    for item in &report.effective_resources {
        output.push_str(&format!(
            "- {} ({:?}, {:?})\n",
            item.path, item.kind, item.representation
        ));
    }

    output.push_str("\nIntegration routes\n");
    for (id, integration) in &report.integrations {
        output.push_str(&format!(
            "- {id}: {}\n",
            runtime_path(&integration.runtime_integration)
        ));
        for route in &integration.routes {
            output.push_str(&format!(
                "  - {}: {:?}, {:?}, {} — {}\n",
                route.capability_path,
                route.route,
                route.verification,
                exposure_mechanism(&route.exposure_plan.mechanism),
                route.rationale
            ));
        }
    }

    output.push_str("\nStandards Coverage / Remaining Gap\n");
    for row in &report.standards_coverage {
        let standard = row.standard.as_deref().unwrap_or("None");
        output.push_str(&format!(
            "- {} | {} | {} | {}\n",
            row.concern, standard, row.coverage, row.remaining_gap
        ));
    }
    output
}

fn exposure_mechanism(mechanism: &crate::exposure::ExposureMechanism) -> &'static str {
    match mechanism {
        crate::exposure::ExposureMechanism::DirectNative { .. } => "DIRECT_NATIVE",
        crate::exposure::ExposureMechanism::RuntimeBridge { .. } => "RUNTIME_BRIDGE",
        crate::exposure::ExposureMechanism::FilesystemProjection { .. } => "FILESYSTEM_PROJECTION",
        crate::exposure::ExposureMechanism::ManagedUserScopeReference { .. } => {
            "MANAGED_USER_SCOPE_REFERENCE"
        }
        crate::exposure::ExposureMechanism::ManagedVendorConfig { .. } => "MANAGED_VENDOR_CONFIG",
        crate::exposure::ExposureMechanism::Unsupported { .. } => "UNSUPPORTED",
    }
}

fn report_item(
    environment: &EffectiveEnvironment,
    resource: &crate::project::Resource,
) -> ReportItem {
    let capability = &resource.capability;
    ReportItem {
        name: capability.name(),
        path: resource.display_path(&environment.root),
        kind: capability.kind,
        representation: capability.representation,
    }
}

fn runtime_path(integration: &RuntimeIntegration) -> String {
    match integration {
        RuntimeIntegration::NativeAcp => "native ACP".to_owned(),
        RuntimeIntegration::ReliableAcpAdapter { adapter } => {
            format!("reliable ACP adapter ({adapter})")
        }
        RuntimeIntegration::MinimalExplicitAdapter { adapter } => {
            format!("minimal explicit adapter ({adapter})")
        }
        RuntimeIntegration::Unavailable { rationale } => format!("unavailable — {rationale}"),
    }
}

fn standards_coverage() -> Vec<StandardsCoverage> {
    vec![
        coverage(
            "Project instructions",
            Some("AGENTS.md"),
            "Portable instructions where a runtime supports discovery.",
            "Discovery and proprietary rules vary.",
        ),
        coverage(
            "Reusable capabilities",
            Some("Agent Skills"),
            "Portable SKILL.md capabilities and assets.",
            "Locations and vendor extensions vary.",
        ),
        coverage(
            "Tools/resources",
            Some("MCP"),
            "Portable tool, resource, and prompt/context interoperability.",
            "Configuration and authorization vary.",
        ),
        coverage(
            "Client ↔ Agent",
            Some("ACP"),
            "Negotiated runtime interaction when both endpoints support ACP.",
            "Target adoption and adapters vary.",
        ),
        coverage(
            "Agent ↔ Agent",
            Some("A2A"),
            "Future candidate only.",
            "No MVP orchestration need.",
        ),
        coverage(
            "Project composition",
            None,
            "No current composition standard.",
            "UZE resolves and explains the effective environment without creating a format.",
        ),
        coverage(
            "Harness-specific capabilities",
            None,
            "Optional native enhancements may be used.",
            "Hooks, commands, subagents, permissions, and memory have no safe universal equivalence.",
        ),
    ]
}

fn coverage(
    concern: &str,
    standard: Option<&str>,
    coverage: &str,
    remaining_gap: &str,
) -> StandardsCoverage {
    StandardsCoverage {
        concern: concern.to_owned(),
        standard: standard.map(str::to_owned),
        coverage: coverage.to_owned(),
        remaining_gap: remaining_gap.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        capability::CapabilityKind,
        exposure::{ExposureMechanism, ExposurePlan},
        integration::IntegrationPort,
        project::resolve_project,
        router::{CompatibilityRoute, HarnessCapabilities},
    };
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn report_is_reproducible_for_unchanged_project() {
        let root = temp_project("report");
        fs::create_dir_all(root.join(".agents/skills/review")).unwrap();
        fs::write(root.join("AGENTS.md"), "# Instructions\n").unwrap();
        fs::write(
            root.join(".agents/skills/review/SKILL.md"),
            "---\nname: review\n---\n",
        )
        .unwrap();
        fs::write(root.join("mcp.json"), "{\"mcpServers\":{}}\n").unwrap();

        let environment = resolve_project(&root).unwrap();
        let first = TestIntegration { id: "first" };
        let second = TestIntegration { id: "second" };
        let integrations: [&dyn IntegrationPort; 2] = [&first, &second];
        let first = serde_json::to_vec(&build_report(&environment, &integrations)).unwrap();
        let second = serde_json::to_vec(&build_report(&environment, &integrations)).unwrap();
        assert_eq!(first, second);
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_project(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
    }

    struct TestIntegration {
        id: &'static str,
    }

    impl IntegrationPort for TestIntegration {
        fn id(&self) -> &'static str {
            self.id
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities {
                direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
                evidence: "test evidence".to_owned(),
                ..HarnessCapabilities::default()
            }
        }

        fn exposure_plan(&self, resource: &crate::project::Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Native,
                verification: crate::router::VerificationStatus::Unverified,
                mechanism: ExposureMechanism::DirectNative {
                    resource_path: resource.capability.path.clone(),
                },
                evidence: "test exposure".to_owned(),
            }
        }
    }
}
