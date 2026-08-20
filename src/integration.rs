use crate::{
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
}

#[derive(Clone, Debug)]
pub struct IntegrationAssessment {
    pub integration_id: String,
    pub capability_path: String,
    pub decision: RouteDecision,
}

pub fn assess_environment(
    environment: &EffectiveEnvironment,
    integration: &dyn IntegrationPort,
) -> Vec<IntegrationAssessment> {
    let capabilities = integration.capabilities();
    environment
        .project_resources
        .iter()
        .map(|capability| IntegrationAssessment {
            integration_id: integration.id().to_owned(),
            capability_path: capability.display_path(&environment.root),
            decision: route(capability, &capabilities),
        })
        .collect()
}
