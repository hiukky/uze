//! Lifecycle — attach — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::collections::BTreeSet;

use uze_core::{
    Result,
    capability::CapabilityKind,
    exposure::ExposureMechanism,
    integration::{
        AttachmentReceipt, AttachmentState, IntegrationPort, ManagedArtifact,
        managed_artifact_exposure_name, receipt_location,
    },
    project::Resource,
    state,
    store::StoredPackage,
};

use super::super::*;

/// Whether an integration's own package-level plan may be used for this
/// delivery. `Skipped` when the derived view a native package reads failed
/// to refresh — attempting it would fail for a reason that has nothing to
/// do with the package, so delivery falls back to capability level.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeDelivery {
    Allowed,
    Skipped,
}

/// What one package's delivery to one integration produced: the native plan
/// it was delivered under, if any, and where each recorded artifact landed.
#[derive(Default)]
pub(crate) struct PackageDelivery {
    pub plan: Option<uze_core::exposure::PackageExposurePlan>,
    pub attachments: Vec<AttachmentSummary>,
}

impl UzeApplication {
    pub(crate) fn attach_stored_packages_to(
        &self,
        integration: &dyn IntegrationPort,
    ) -> Result<()> {
        let mut first_error: Option<uze_core::UzeError> = None;
        for package_id in self.store.package_ids()? {
            let package = match self.store.package(&package_id) {
                Ok(pkg) => pkg,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    continue;
                }
            };
            if let Err(error) = self.attach_package_to(&package, integration)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn attach_package_to(
        &self,
        package: &StoredPackage,
        integration: &dyn IntegrationPort,
    ) -> Result<()> {
        let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        self.deliver_package_to(package, &resources, integration, NativeDelivery::Allowed)?;
        Ok(())
    }

    /// The one receipt-safe delivery of a package to a single integration:
    /// native package plan first when the integration offers one, then a
    /// capability-level attachment for every resource the plan does not
    /// provide. Returns what it produced so an install can report it —
    /// `attach` and `install` must never grow two copies of this.
    pub(crate) fn deliver_package_to(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
        integration: &dyn IntegrationPort,
        native_delivery: NativeDelivery,
    ) -> Result<PackageDelivery> {
        let _span = tracing::info_span!(
            "integration.attach",
            integration = integration.id(),
            package = %package.id.as_str()
        )
        .entered();
        let mut delivery = PackageDelivery::default();
        let mut provided = BTreeSet::new();
        if let Some(plan) = integration
            .package_exposure_plan(package, resources)
            .filter(|_| native_delivery == NativeDelivery::Allowed)
        {
            delivery.plan = Some(plan.clone());
            // Idempotency guard: a package-level receipt already recorded
            // for this integration that still inspects Matched means the
            // vendor install verb already ran. Re-running it is not safely
            // idempotent — Antigravity's preflight refuses an existing
            // same-name import UZE has to prove it owns again, and a
            // truthful `agy plugin list` would turn every second
            // `setup`/attach into a hard failure (found by the acceptance
            // suite with a truthful fake CLI).
            let already_attached = state::receipts(&self.home, Some(package.id.as_str()))?
                .into_iter()
                .any(|(_, receipt)| {
                    receipt.integration == integration.id()
                        && receipt.resource_identity.is_none()
                        && integration.inspect_receipt(&receipt).state == AttachmentState::Matched
                });
            if already_attached {
                provided = plan.provided_resource_identities;
            } else {
                // The package was delivered capability-by-capability before
                // this integration had a native plan for it. Detach the
                // capability receipts the plan now covers, but only while
                // every one of them is safely detachable: a single
                // Drifted/Conflict/Blocked leaves decomposed delivery in
                // place rather than adding a native copy beside it.
                let existing: Vec<(String, uze_core::integration::AttachmentReceipt)> =
                    state::receipts(&self.home, Some(package.id.as_str()))?
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
                let native_blocked = covered_existing.iter().any(|(_, receipt)| {
                    matches!(
                        integration.inspect_receipt(receipt).state,
                        AttachmentState::Drifted
                            | AttachmentState::Conflict
                            | AttachmentState::Blocked
                    )
                });
                if native_blocked {
                    // Keep decomposed delivery; do not attach native to avoid duplication.
                    provided = BTreeSet::new();
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
                    if let Some(receipt) = integration.attach_package(package, &plan)? {
                        let location = receipt_location(&receipt);
                        state::record_receipt(
                            &self.home,
                            package_receipt_key(package.id.as_str(), integration.id()),
                            receipt,
                        )?;
                        delivery.attachments.push(AttachmentSummary {
                            integration: integration.id().to_owned(),
                            location,
                        });
                    }
                    // A native plan that returns no receipt explicitly
                    // declined UZE ownership (for example, a same-name
                    // plugin already imported by the harness). Its native
                    // delivery remains authoritative, so avoid adding
                    // duplicate capability-level fallbacks beside it.
                    provided = plan.provided_resource_identities;
                }
            }
        }
        for resource in resources {
            if !provided.contains(&resource.identity()) {
                let resolved = self.resolve_exposure_name(resource, integration)?;
                if let Some(receipt) = integration.attach_receipt(&resolved)? {
                    let location = receipt_location(&receipt);
                    state::record_receipt(
                        &self.home,
                        resource_receipt_key(package.id.as_str(), integration.id(), resource),
                        receipt,
                    )?;
                    delivery.attachments.push(AttachmentSummary {
                        integration: integration.id().to_owned(),
                        location,
                    });
                }
            }
        }
        Ok(delivery)
    }

    /// Resolves `resource`'s physical exposure name for `integration`,
    /// immediately before an attach call — the one place a naming decision
    /// happens. Returns a clone of `resource` with `resolved_exposure_name`
    /// set; `resource` itself is never mutated.
    ///
    /// "Existing receipt wins": a receipt for this exact
    /// `resource.identity()` hands back its already-recorded physical name
    /// verbatim, which is what makes re-add/setup idempotent. The same
    /// reuse extends to a *different* integration's receipt for the same
    /// resource when both report the same `shared_agent_skill_root` (Codex
    /// and OpenCode both read `~/.agents/skills`), so the second attach
    /// writes the very same entry instead of a second one beside it.
    ///
    /// Only a resource with no reusable receipt anywhere asks the
    /// integration for ordered candidates (`exposure_name_candidates`) and
    /// takes the first not already claimed — by this integration or by one
    /// sharing its skill root. It resolves purely from the ledger, never
    /// the filesystem, so it never decides a foreign-artifact conflict;
    /// attach's own structural check remains the last word on that.
    pub(crate) fn resolve_exposure_name(
        &self,
        resource: &Resource,
        integration: &dyn IntegrationPort,
    ) -> Result<Resource> {
        if !matches!(
            resource.capability.kind,
            CapabilityKind::AgentSkill | CapabilityKind::Mcp
        ) {
            return Ok(resource.clone());
        }
        let mut resolved = resource.clone();
        let Ok(all_receipts) = state::receipts(&self.home, None) else {
            return Ok(resolved);
        };
        let resource_id = resource.identity();
        // Only Agent Skills live in a directory shared across integrations
        // (Codex, OpenCode all read `~/.agents/skills`).
        let shared_root = (resource.capability.kind == CapabilityKind::AgentSkill)
            .then(|| integration.shared_agent_skill_root())
            .flatten();
        let shares_root = |other_id: &str| -> bool {
            let Some(root) = &shared_root else {
                return false;
            };
            self.integrations.iter().any(|other| {
                other.id() == other_id && other.shared_agent_skill_root().as_ref() == Some(root)
            })
        };
        // Existing receipt wins: a resource already attached keeps the
        // physical entry it was given, so re-running attach never renames
        // or duplicates it.
        if let Some((_, existing)) = all_receipts.iter().find(|(_, receipt)| {
            receipt.resource_identity.as_deref() == Some(resource_id.as_str())
                && (receipt.integration == integration.id() || shares_root(&receipt.integration))
        }) {
            resolved.resolved_exposure_name = managed_artifact_exposure_name(&existing.artifact);
            resolved.resolved_artifact_target = match &existing.artifact {
                ManagedArtifact::SymlinkReference { target, .. } => Some(target.clone()),
                _ => None,
            };
            return Ok(resolved);
        }
        let claimed: BTreeSet<String> = all_receipts
            .iter()
            .filter(|(_, receipt)| {
                receipt.integration == integration.id() || shares_root(&receipt.integration)
            })
            .filter_map(|(_, receipt)| managed_artifact_exposure_name(&receipt.artifact))
            .collect();
        // A shared root must converge on the same physical name no matter
        // which member happens to attach first. If any integration sharing
        // `shared_root` prefers the resource's bare logical name first (only
        // OpenCode does today, for its V2 slash-command UX), that preference
        // governs for the whole group — otherwise whichever of
        // Codex/OpenCode attaches before OpenCode would lock the
        // group onto the always-qualified fallback via the reuse check
        // above, even though the bare name was free.
        let candidates = shared_root
            .as_ref()
            .and_then(|root| {
                self.integrations
                    .iter()
                    .filter(|other| other.shared_agent_skill_root().as_ref() == Some(root))
                    .map(|other| other.exposure_name_candidates(resource))
                    .find(|list| {
                        list.first().map(String::as_str)
                            == resource.logical_capability_name().as_deref()
                    })
            })
            .unwrap_or_else(|| integration.exposure_name_candidates(resource));
        if let Some(free) = candidates
            .iter()
            .find(|candidate| !claimed.contains(*candidate))
            .cloned()
        {
            resolved.resolved_exposure_name = Some(free);
            return Ok(resolved);
        }
        // Every candidate is already claimed. The reuse path above already
        // returned for the same-resource case (identical canonical identity
        // sharing one physical entry across shared-root harnesses), so a
        // claimed name here belongs to a DIFFERENT canonical resource: two
        // distinct resources converging on one label — one physical entry,
        // incompatible representations. That is a projection ownership
        // conflict, not drift; report it deterministically before any
        // attach, instead of handing the conflicting name back and failing
        // later with a misleading `ManagedEntryDrift` (ADR-029).
        // Nothing in `IntegrationPort` forbids an empty candidate list, and a
        // lifecycle operation is the wrong place to discover that: report the
        // integration that could not name the resource, the way the ledger
        // drift a few lines below degrades rather than panics.
        let Some(entry) = candidates.last().cloned() else {
            return Err(UzeError::ExposureUnavailable(format!(
                "`{}` offers no name to expose `{}` under",
                integration.id(),
                resource.name()
            )));
        };
        let claimant = all_receipts
            .iter()
            .filter(|(_, receipt)| {
                receipt.integration == integration.id() || shares_root(&receipt.integration)
            })
            .find_map(|(_, receipt)| {
                (managed_artifact_exposure_name(&receipt.artifact).as_deref()
                    == Some(entry.as_str()))
                .then_some(receipt)
            });
        let Some(claimant) = claimant else {
            // Defensive fallback (should be unreachable): retain the
            // previous behavior rather than panicking on ledger drift.
            resolved.resolved_exposure_name = Some(entry);
            return Ok(resolved);
        };
        let requested_target = match integration.exposure_plan(resource).mechanism {
            ExposureMechanism::ManagedUserScopeReference { source, .. } => source,
            _ => resource.capability.path.clone(),
        };
        Err(UzeError::ProjectionConflict(Box::new(
            uze_core::error::ProjectionConflictDetails {
                entry: integration
                    .shared_agent_skill_root()
                    .map_or_else(|| PathBuf::from(&entry), |root| root.join(&entry)),
                requested: resource.identity(),
                requested_integration: integration.id().to_owned(),
                requested_target,
                existing: claimant
                    .resource_identity
                    .clone()
                    .unwrap_or_else(|| claimant.package_id.clone()),
                existing_integration: claimant.integration.clone(),
                existing_target: artifact_owned_target(claimant),
            },
        )))
    }
}

/// The physical artifact a receipt's entry points at — the symlink target
/// for a reference, the file itself for a managed file, and the receipt
/// location for anything else (diagnostic-only, never owned data).
fn artifact_owned_target(receipt: &AttachmentReceipt) -> PathBuf {
    match &receipt.artifact {
        ManagedArtifact::SymlinkReference { target, .. } => target.clone(),

        _ => receipt_location(receipt),
    }
}
