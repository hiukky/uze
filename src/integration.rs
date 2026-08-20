use crate::{
    exposure::ExposurePlan,
    project::EffectiveEnvironment,
    router::{HarnessCapabilities, RouteDecision, route},
    runtime::RuntimeSupport,
};

pub trait IntegrationPort {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> HarnessCapabilities;

    fn runtime_support(&self) -> RuntimeSupport {
        RuntimeSupport::default()
    }

    /// The integration, not the resource representation, selects how the
    /// harness receives a capability from a composed UZE environment.
    fn exposure_plan(&self, resource: &crate::project::Resource) -> ExposurePlan;
}

#[derive(Clone, Debug)]
pub struct IntegrationAssessment {
    pub integration_id: String,
    pub capability_path: String,
    pub decision: RouteDecision,
    pub exposure_plan: ExposurePlan,
}

pub fn assess_environment(
    environment: &EffectiveEnvironment,
    integration: &dyn IntegrationPort,
) -> Vec<IntegrationAssessment> {
    let capabilities = integration.capabilities();
    environment
        .resources
        .iter()
        .map(|resource| {
            let exposure_plan = integration.exposure_plan(resource);
            let mut decision = route(&resource.capability, &capabilities);
            decision.route = exposure_plan.route;
            decision.exposure = exposure_plan.exposure;
            decision.rationale = exposure_plan.evidence.clone();
            decision.evidence = exposure_plan.evidence.clone();
            IntegrationAssessment {
                integration_id: integration.id().to_owned(),
                capability_path: resource.display_path(&environment.root),
                decision,
                exposure_plan,
            }
        })
        .collect()
}
