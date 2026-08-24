//! Lifecycle — install — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::collections::BTreeSet;

use uze_core::{
    PackageSource, Result,
    integration::{AttachmentState, receipt_location},
    state,
    trust::{self, TrustAuthority},
};

use crate::bootstrap;

use super::super::*;

impl UzeApplication {
    pub(crate) fn acquire(&self, source: &PackageSource) -> Result<uze_core::MaterializedPackage> {
        match source {
            PackageSource::Embedded { id } => bootstrap::materialize(id),
            _ => uze_core::acquisition::acquire(source),
        }
    }

    pub fn add_plugin(
        &self,
        source: PackageSource,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        // Acquisition brings the bytes to a local directory and owns their
        // cleanup; the Store only ever sees a materialized package.
        let materialized = self.acquire(&source)?;
        self.install_materialized(materialized, authority, &[], false)
    }

    pub(crate) fn install_materialized(
        &self,
        materialized: uze_core::MaterializedPackage,
        authority: &dyn TrustAuthority,
        already_trusted: &[trust::ExecutableCapability],
        replacing_installed: bool,
    ) -> Result<AddPluginReport> {
        // Any installation changes vendor-visible state; cached inspection
        // verdicts must not outlive it (ADR 024).
        self.inspection_cache.invalidate();
        // Trust is decided here — after the package is materialized and can
        // be inspected honestly, and strictly before anything is written to
        // the Store or shown to a harness. Neither the Store nor any
        // integration knows this question exists.
        self.authorize(
            &materialized,
            authority,
            already_trusted,
            replacing_installed,
        )?;

        // `uze add` is deliberately enough for a harness the user already
        // has.  Preparing a detected integration only creates UZE's own
        // prerequisites (such as a user-scope discovery directory) and
        // records its setup state; it never installs, upgrades, or launches
        // the vendor executable.  Do it before ingesting so a preparation
        // failure cannot leave a newly installed package with no reported
        // delivery attempt.
        self.prepare_detected_integrations(None)?;

        let installed = self.store.ingest(&materialized)?;

        // Derived views refresh before attachment: a native package delivery
        // reads the view it was just given. A failure here is recorded, never
        // propagated — the package is installed, and one integration's view
        // being stale does not make the installation invalid.
        let publications = self.republish_all();
        let unpublished: BTreeSet<&str> = publications
            .iter()
            .filter(|outcome| outcome.error.is_some())
            .map(|outcome| outcome.integration.as_str())
            .collect();

        let environment = self.engine().compose(std::slice::from_ref(&installed.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let mut attachments = Vec::new();
        let mut package_plans = Vec::new();
        for integration in &self.integrations {
            // A package must remain installable on a machine that has only a
            // subset of UZE's peer harnesses. `add` prepares and attaches to
            // detected harnesses; an absent executable is neither a package
            // incompatibility nor a reason to invoke its vendor CLI.
            if !self.detect_cached(integration.as_ref()).present {
                continue;
            }
            let mut provided = BTreeSet::new();
            // Native delivery reads the view; attempting it against a view
            // that failed to publish would fail for a reason that has
            // nothing to do with this package.
            if let Some(plan) = integration
                .package_exposure_plan(&installed, &resources)
                .filter(|_| !unpublished.contains(integration.id()))
            {
                package_plans.push((integration.id().to_owned(), plan.clone()));
                // Idempotency: package-level receipt already Matched means the
                // vendor verb already ran — re-running `agy plugin install`
                // would hit preflight ("already has an imported plugin named
                // `git`") even though UZE owns it. Skip attach and keep
                // `provided` so capability-level attach is also skipped.
                let already_attached = state::receipts(&self.home, Some(installed.id.as_str()))?
                    .into_iter()
                    .any(|(_, receipt)| {
                        receipt.integration == integration.id()
                            && receipt.resource_identity.is_none()
                            && integration.inspect_receipt(&receipt).state
                                == AttachmentState::Matched
                    });
                if already_attached {
                    provided = plan.provided_resource_identities.clone();
                    continue;
                }
                // Migration: if this package was previously decomposed, detach
                // covered capability receipts that are now provided, but only
                // if they are safely detachable.
                let existing: Vec<(String, uze_core::integration::AttachmentReceipt)> =
                    state::receipts(&self.home, Some(installed.id.as_str()))?
                        .into_iter()
                        .filter(|(_, r)| {
                            r.integration == integration.id() && r.resource_identity.is_some()
                        })
                        .collect();
                let mut covered_existing = Vec::new();
                for (key, receipt) in &existing {
                    if let Some(identity) = &receipt.resource_identity
                        && plan.provided_resource_identities.contains(identity)
                    {
                        covered_existing.push((key.clone(), receipt.clone()));
                    }
                }
                let mut migration_blocked = false;
                for (_, receipt) in &covered_existing {
                    let inspection = integration.inspect_receipt(receipt);
                    if matches!(
                        inspection.state,
                        AttachmentState::Drifted
                            | AttachmentState::Conflict
                            | AttachmentState::Blocked
                    ) {
                        migration_blocked = true;
                        break;
                    }
                }
                if migration_blocked {
                    // Keep decomposed; do not attach native to avoid duplication.
                } else {
                    for (key, receipt) in covered_existing {
                        let inspection = integration.inspect_receipt(&receipt);
                        if inspection.state == AttachmentState::Matched {
                            let detached = integration.detach_receipt(&receipt)?;
                            if detached.state == AttachmentState::Missing {
                                state::forget_receipt(&self.home, &key)?;
                            }
                        } else if inspection.state == AttachmentState::Missing {
                            state::forget_receipt(&self.home, &key)?;
                        }
                    }
                    if let Some(receipt) = integration.attach_package(&installed, &plan)? {
                        let location = receipt_location(&receipt);
                        state::record_receipt(
                            &self.home,
                            package_receipt_key(installed.id.as_str(), integration.id()),
                            receipt,
                        )?;
                        attachments.push(AttachmentSummary {
                            integration: integration.id().to_owned(),
                            location,
                        });
                        provided = plan.provided_resource_identities;
                    }
                }
            }
            for resource in &resources {
                if !provided.contains(&resource.identity()) {
                    let resolved = self.resolve_exposure_name(resource, integration.as_ref())?;
                    if let Some(receipt) = integration.attach_receipt(&resolved)? {
                        let location = receipt_location(&receipt);
                        state::record_receipt(
                            &self.home,
                            resource_receipt_key(installed.id.as_str(), integration.id(), resource),
                            receipt,
                        )?;
                        attachments.push(AttachmentSummary {
                            integration: integration.id().to_owned(),
                            location,
                        });
                    }
                }
            }
        }
        Ok(AddPluginReport {
            plugin: self.plugin_summary(&installed)?,
            package_plans,
            attachments,
            publications,
        })
    }
}
