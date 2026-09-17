//! Lifecycle — remove — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{
    PackageSource, Result, UzeError,
    integration::AttachmentState,
    manifest::BUILT_IN_MARKETPLACE,
    reconciliation::{PackageRemovalPlan, plan_remove},
    state,
    store::StoredPackage,
};

use crate::bootstrap;

use super::super::services::Plugins;
use super::super::*;

impl Plugins<'_> {
    #[tracing::instrument(name = "plugins.remove", skip_all, fields(id = %id), err)]
    pub fn remove(&self, id: &str) -> Result<RemovePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // Removal changes vendor-visible state; cached inspection verdicts
        // must not outlive it (ADR 018).
        self.0.inspection_cache.invalidate();
        self.detach_and_remove(id, false)
    }

    /// Whether an installed package is protected from removal. Only the
    /// verified official origin is: an embedded provenance, not a
    /// marketplace name — a local/Git package that merely shares the name
    /// `uze` remains removable, so protection cannot be spoofed by naming.
    pub(crate) fn is_protected_package(package: &StoredPackage) -> bool {
        matches!(
            &package.provenance.requested,
            PackageSource::Embedded { id } if is_protected_plugin(BUILT_IN_MARKETPLACE, id)
        )
    }

    pub(crate) fn detach_and_remove(
        &self,
        id: &str,
        allow_protected: bool,
    ) -> Result<RemovePluginReport> {
        let package = match self.0.package_by_name(id) {
            Ok(package) => package,
            Err(UzeError::UnknownPackage(_)) => {
                // There is no tombstone, so UZE cannot claim this package was
                // previously installed. It can still make repeated remove a
                // safe no-op when no ownership evidence remains.
                if state::receipts(&self.0.home, Some(id))?.is_empty() {
                    return Ok(RemovePluginReport::AlreadyAbsent {
                        plugin: id.to_owned(),
                    });
                }
                return Ok(RemovePluginReport::Blocked {
                    report: self.0.reconcile(id),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            }
            Err(error) => return Err(error),
        };
        if !allow_protected && Self::is_protected_package(&package) {
            return Err(UzeError::ProtectedPackage(package.id.as_str().to_owned()));
        }
        let report = self.0.reconcile(package.id.as_str());
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
                .0
                .integrations
                .iter()
                .find(|integration| integration.id() == reconciled.receipt.integration)
            else {
                return Ok(RemovePluginReport::Blocked {
                    report: self.0.reconcile(package.id.as_str()),
                    plan: PackageRemovalPlan::BlockedByInspection,
                });
            };
            let detached = integration.detach_receipt(&reconciled.receipt)?;
            if detached.state != AttachmentState::Missing {
                return Ok(RemovePluginReport::Blocked {
                    report: self.0.reconcile(package.id.as_str()),
                    plan: plan_remove(&self.0.reconcile(package.id.as_str())),
                });
            }
        }
        let final_report = self.0.reconcile(package.id.as_str());
        let final_plan = plan_remove(&final_report);
        if !matches!(final_plan, PackageRemovalPlan::Safe { .. }) {
            return Ok(RemovePluginReport::Blocked {
                report: final_report,
                plan: final_plan,
            });
        }
        for reconciled in &final_report.receipts {
            state::forget_receipt(&self.0.home, &reconciled.ledger_key)?;
        }
        self.0.store.remove_package(&package.id)?;
        // The package set changed, so every derived view is now stale. A
        // failure to rebuild one does not un-remove the package.
        let _ = self.0.republish_all();
        Ok(RemovePluginReport::Removed {
            plugin: package.id.as_str().to_owned(),
            detached_receipts,
            already_missing_receipts,
        })
    }
}

/// Whether `plugin`, offered by `marketplace`, belongs to the official set
/// compiled into this binary — which re-seeds itself, so removing a member
/// of it is not an operation that means anything.
pub(crate) fn is_protected_plugin(marketplace: &str, plugin: &str) -> bool {
    marketplace == BUILT_IN_MARKETPLACE
        && bootstrap::entries()
            .is_ok_and(|official| official.plugins.iter().any(|entry| entry.name == plugin))
}
