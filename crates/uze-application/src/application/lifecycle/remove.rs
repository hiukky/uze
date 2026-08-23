//! Lifecycle — remove — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{
    PackageSource, Result, UzeError,
    integration::AttachmentState,
    reconciliation::{PackageRemovalPlan, plan_remove},
    state,
    store::StoredPackage,
};

use crate::bootstrap;

use super::super::*;

impl UzeApplication {
    pub fn remove_plugin(&self, id: &str) -> Result<RemovePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        // Removal changes vendor-visible state; cached inspection verdicts
        // must not outlive it (ADR 024).
        self.inspection_cache.invalidate();
        let report = self.detach_and_remove(id, false)?;
        if matches!(report, RemovePluginReport::Removed { .. }) {
            let _ = uze_core::state::plugin_marketplace_remove(&self.home, id);
        }
        Ok(report)
    }

    pub(crate) fn is_protected_package(package: &StoredPackage) -> bool {
        // Only the verified official origin is protected: an embedded
        // provenance whose id appears in the compiled marketplace snapshot.
        // A local/git package that merely shares the name `uze` is not
        // official and remains removable — prevents spoofing by name alone.
        let embedded_id = match &package.provenance.requested {
            PackageSource::Embedded { id } => id,
            _ => return false,
        };
        if bootstrap::DEFAULT_PLUGIN_IDS.contains(&embedded_id.as_str()) {
            return true;
        }
        if let Ok((_, entries)) = bootstrap::entries()
            && entries.iter().any(|entry| entry.name == *embedded_id)
        {
            return true;
        }
        false
    }

    pub(crate) fn detach_and_remove(
        &self,
        id: &str,
        allow_protected: bool,
    ) -> Result<RemovePluginReport> {
        let package = match self.package_by_name(id) {
            Ok(package) => package,
            Err(UzeError::UnknownPackage(_)) => {
                // There is no tombstone, so UZE cannot claim this package was
                // previously installed. It can still make repeated remove a
                // safe no-op when no ownership evidence remains.
                if state::receipts(&self.home, Some(id))?.is_empty() {
                    return Ok(RemovePluginReport::AlreadyAbsent {
                        plugin: id.to_owned(),
                    });
                }
                return Ok(RemovePluginReport::Blocked {
                    report: self.reconcile(id),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            }
            Err(error) => return Err(error),
        };
        if !allow_protected && Self::is_protected_package(&package) {
            return Err(UzeError::ExposureUnavailable(format!(
                "official marketplace plugin `{}` is protected and cannot be removed",
                package.id.as_str()
            )));
        }
        let report = self.reconcile(package.id.as_str());
        let plan = plan_remove(&report);
        let (detached_receipts, already_missing_receipts) = match &plan {
            PackageRemovalPlan::Safe {
                detachable_receipts,
                already_missing_receipts,
            } => (
                detachable_receipts.clone(),
                already_missing_receipts.clone(),
            ),
            _ => (Vec::new(), Vec::new()),
        };
        if !matches!(plan, PackageRemovalPlan::Safe { .. }) {
            return Ok(RemovePluginReport::Blocked { report, plan });
        }
        for reconciled in &report.receipts {
            if reconciled.inspection.state != AttachmentState::Matched {
                continue;
            }
            let Some(integration) = self
                .integrations
                .iter()
                .find(|integration| integration.id() == reconciled.receipt.integration)
            else {
                return Ok(RemovePluginReport::Blocked {
                    report: self.reconcile(package.id.as_str()),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            };
            let detached = integration.detach_receipt(&reconciled.receipt)?;
            if detached.state != AttachmentState::Missing {
                return Ok(RemovePluginReport::Blocked {
                    report: self.reconcile(package.id.as_str()),
                    plan: plan_remove(&self.reconcile(package.id.as_str())),
                });
            }
        }
        let final_report = self.reconcile(package.id.as_str());
        let final_plan = plan_remove(&final_report);
        if !matches!(final_plan, PackageRemovalPlan::Safe { .. }) {
            return Ok(RemovePluginReport::Blocked {
                report: final_report,
                plan: final_plan,
            });
        }
        for reconciled in &final_report.receipts {
            state::forget_receipt(&self.home, &reconciled.ledger_key)?;
        }
        self.store.remove_package(&package.id)?;
        // The package set changed, so every derived view is now stale. A
        // failure to rebuild one does not un-remove the package.
        let _ = self.republish_all();
        Ok(RemovePluginReport::Removed {
            plugin: package.id.as_str().to_owned(),
            detached_receipts,
            already_missing_receipts,
        })
    }
}
