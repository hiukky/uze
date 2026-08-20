use std::path::PathBuf;

use serde::Serialize;

use crate::{
    error::Result,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    project::EffectiveEnvironment,
    router::{HarnessCapabilities, RouteDecision, route},
    runtime::RuntimeSupport,
    state,
};

/// Read-only detection of a harness binary. No side effects.
#[derive(Clone, Debug, Default, Serialize)]
pub struct HarnessDetection {
    pub present: bool,
    pub version: Option<String>,
}

/// Diagnosable, machine-level integration status for `uze doctor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationStatus {
    NotConfigured,
    InstalledUnverified,
    InstalledVerified,
}

pub trait IntegrationPort {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> HarnessCapabilities;

    fn runtime_support(&self) -> RuntimeSupport {
        RuntimeSupport::default()
    }

    /// The integration, not the resource representation, selects how the
    /// harness receives a capability from a composed UZE environment.
    fn exposure_plan(&self, resource: &crate::project::Resource) -> ExposurePlan;

    /// Detects whether the harness binary is present and, if cheaply
    /// obtainable, its version. Read-only; performs no filesystem writes.
    fn detect(&self) -> HarnessDetection {
        HarnessDetection::default()
    }

    /// Idempotently ensures this integration's machine-level prerequisites
    /// exist (e.g. its user-scope discovery directory) and records setup
    /// state. Safe to call more than once; a second call refreshes recorded
    /// facts rather than duplicating state or artifacts.
    fn install(&self, home: &UzeHome) -> Result<()> {
        let _ = home;
        Ok(())
    }

    /// Current installed/managed status, for `uze doctor`. The default
    /// reads whatever `install` recorded through the shared `state` module.
    fn status(&self, home: &UzeHome) -> IntegrationStatus {
        match state::get(home, self.id()).ok().flatten() {
            Some(record) if record.installed => IntegrationStatus::InstalledUnverified,
            _ => IntegrationStatus::NotConfigured,
        }
    }

    /// Idempotently creates or refreshes this harness's managed attachment
    /// for one resource. `None` when the currently selected exposure
    /// mechanism does not support persistent attachment (e.g. setup has not
    /// completed yet and the integration is still on a conformance-probe
    /// fallback).
    fn attach(&self, resource: &crate::project::Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                Ok(Some(plan.mechanism.attach()?))
            }
            _ => Ok(None),
        }
    }
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
            decision.verification = exposure_plan.verification.clone();
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
