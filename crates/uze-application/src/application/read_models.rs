//! Doctor/status — extracted without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::Result;

use super::*;

impl UzeApplication {
    pub fn list_plugins(&self) -> Result<Vec<PluginSummary>> {
        self.store
            .package_ids()?
            .into_iter()
            .map(|id| self.plugin_summary(&self.store.package(&id)?))
            .collect()
    }

    pub fn inspect_plugin(&self, id: &str) -> Result<PluginInspection> {
        let package = self.package_by_name(id)?;
        let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let deliveries = self
            .integrations
            .iter()
            .map(|integration| {
                let package_plan = integration.package_exposure_plan(&package, &resources);
                let provided = package_plan
                    .as_ref()
                    .map(|plan| plan.provided_resource_identities.clone())
                    .unwrap_or_default();
                let capabilities = resources
                    .iter()
                    .map(|resource| CapabilityDelivery {
                        identity: resource.identity(),
                        kind: resource.capability.kind,
                        plan: (!provided.contains(&resource.identity()))
                            .then(|| integration.exposure_plan(resource)),
                        provided_by_package: provided.contains(&resource.identity()),
                    })
                    .collect();
                HarnessDelivery {
                    integration: integration.id().to_owned(),
                    package_plan,
                    capabilities,
                }
            })
            .collect();
        let reconciliation = self.reconcile(package.id.as_str());
        Ok(PluginInspection {
            plugin: self.plugin_summary(&package)?,
            capabilities: resources
                .iter()
                .map(|resource| PluginCapability {
                    identity: resource.identity(),
                    name: resource.name(),
                    kind: resource.capability.kind,
                })
                .collect(),
            deliveries,
            managed_state: managed_state(&reconciliation),
            reconciliation,
        })
    }
}
