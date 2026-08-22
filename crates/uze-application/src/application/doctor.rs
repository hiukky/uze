//! Doctor/status — extracted without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{Result, integration::AttachmentState, state};

use super::*;

impl UzeApplication {
    pub fn doctor(&self) -> DoctorReport {
        let package_ids = self.store.package_ids();
        let (store, plugins) = match package_ids {
            Ok(ids) => {
                let packages = ids
                    .into_iter()
                    .filter_map(|id| self.store.package(&id).ok())
                    .collect::<Vec<_>>();
                let inconsistencies = packages
                    .iter()
                    .filter_map(package_store_inconsistency)
                    .collect::<Vec<_>>();
                let health = if inconsistencies.is_empty() {
                    StoreHealth::Ready
                } else {
                    StoreHealth::Blocked(inconsistencies.join("; "))
                };
                (
                    health,
                    packages
                        .iter()
                        .filter_map(|package| self.plugin_summary(package).ok())
                        .collect(),
                )
            }
            Err(error) => (StoreHealth::Blocked(error.to_string()), Vec::new()),
        };
        let installed = self.installed_packages();
        let harnesses = self
            .integrations
            .iter()
            .map(|integration| HarnessHealth {
                integration: integration.id().to_owned(),
                detection: integration.detect(),
                setup: integration_status(integration.status(&self.home)),
                strategy: state::get(&self.home, integration.id())
                    .ok()
                    .flatten()
                    .map(|record| record.strategy),
                provisioning: state::provisioning(&self.home, integration.id())
                    .ok()
                    .flatten(),
                // Observed, not remembered. A package can be installed and
                // reconciled while a harness still cannot see it, and that is
                // exactly the state this field exists to surface.
                publication: integration.publication(&installed),
                capabilities: integration.capabilities(),
                native_instructions: NATIVE_INSTRUCTION_INTEGRATIONS.contains(&integration.id()),
            })
            .collect();
        let attachments = plugins
            .iter()
            .map(|plugin: &PluginSummary| PackageManagedState {
                plugin: plugin.id.clone(),
                state: managed_state(&self.reconcile(&plugin.id)),
            })
            .collect();
        let ledger_error = state::receipts(&self.home, None)
            .err()
            .map(|error| error.to_string());
        let integration_state_error = state::load(&self.home).err().map(|error| error.to_string());
        let provisioning_state_error = self
            .integrations
            .iter()
            .find_map(|integration| state::provisioning(&self.home, integration.id()).err())
            .map(|error| error.to_string());
        DoctorReport {
            uze_home: self.home.root().to_path_buf(),
            store,
            plugins,
            harnesses,
            attachments,
            ledger_error,
            integration_state_error,
            provisioning_state_error,
        }
    }

    pub fn status(&self, project_root: &std::path::Path) -> Result<StatusReport> {
        let context = self.context_inspect(project_root)?;
        let installed = self.store.package_ids()?.len();
        let contributing = context.contributions.len();
        let issues: Vec<String> = context
            .contributions
            .iter()
            .filter(|contribution| !matches!(contribution.state, AttachmentState::Matched))
            .map(|contribution| format!("{}: {:?}", contribution.package_id, contribution.state))
            .chain(
                context
                    .harnesses
                    .iter()
                    .filter_map(|harness| match &harness.delivery {
                        HarnessContextDelivery::Bridge {
                            needed: true,
                            state,
                        } if *state != AttachmentState::Matched => {
                            Some(format!("{}: bridge {:?}", harness.integration, state))
                        }
                        _ => None,
                    }),
            )
            .chain(
                context
                    .malformed_regions
                    .iter()
                    .map(|region| format!("{region}: malformed")),
            )
            .collect();
        Ok(StatusReport {
            root: context.canonical.clone(),
            portability: context.portability,
            harnesses: context.harnesses,
            packages_installed: installed,
            packages_contributing_here: contributing,
            issues,
        })
    }
}
