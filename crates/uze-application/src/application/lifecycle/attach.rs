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

impl UzeApplication {
    pub(crate) fn attach_stored_packages_to(
        &self,
        integration: &dyn IntegrationPort,
    ) -> Result<()> {
        for package_id in self.store.package_ids()? {
            let package = self.store.package(&package_id)?;
            self.attach_package_to(&package, integration)?;
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
        let mut provided = BTreeSet::new();
        if let Some(plan) = integration.package_exposure_plan(package, &resources) {
            // Migration: decomposed → native. Detach covered capability receipts
            // that are now provided by the package, but only if they are
            // safely detachable. Any Drifted/Conflict/Blocked blocks migration
            // and also blocks duplicate native attach to avoid duplication.
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
            let mut migration_blocked = false;
            for (_, receipt) in &covered_existing {
                let inspection = integration.inspect_receipt(receipt);
                if matches!(
                    inspection.state,
                    AttachmentState::Drifted | AttachmentState::Conflict | AttachmentState::Blocked
                ) {
                    migration_blocked = true;
                    break;
                }
            }
            if migration_blocked {
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
                    state::record_receipt(
                        &self.home,
                        package_receipt_key(package.id.as_str(), integration.id()),
                        receipt,
                    )?;
                    provided = plan.provided_resource_identities;
                }
            }
        }
        for resource in &resources {
            if !provided.contains(&resource.identity()) {
                let resolved = self.resolve_exposure_name(resource, integration)?;
                if let Some(receipt) = integration.attach_receipt(&resolved)? {
                    state::record_receipt(
                        &self.home,
                        resource_receipt_key(package.id.as_str(), integration.id(), resource),
                        receipt,
                    )?;
                }
            }
        }
        Ok(())
    }

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
        if let Some((key, existing)) = all_receipts.iter().find(|(_, receipt)| {
            receipt.resource_identity.as_deref() == Some(resource_id.as_str())
                && (receipt.integration == integration.id() || shares_root(&receipt.integration))
        }) {
            let existing_name = managed_artifact_exposure_name(&existing.artifact);
            let current_candidate = integration
                .exposure_name_candidates(resource)
                .into_iter()
                .next();
            // Naming-schema migration (ADR-026): an existing receipt whose
            // physical name is no longer the integration's current single
            // deterministic candidate is a legacy artifact from a previous
            // naming policy (bare-first or `pkg-name`). Migrate it to the
            // stable label instead of freezing the legacy name forever:
            //
            // - Matched (UZE owns the artifact exactly) → detach the old
            //   entry and re-attach under the current label;
            // - Missing → forget the stale receipt;
            // - Conflict → a foreign artifact occupies the old name: UZE
            //   surrenders that name (foreign content is never touched) and
            //   forgets the stale receipt;
            // - Drifted / Blocked → leave untouched (the user intervened;
            //   UZE attaches under the label without touching that entry).
            if let (Some(old_name), Some(canonical_name)) = (&existing_name, &current_candidate)
                && old_name != canonical_name
            {
                let inspection = integration.inspect_receipt(existing);
                match inspection.state {
                    AttachmentState::Matched => {
                        if integration.detach_receipt(existing)?.state == AttachmentState::Missing {
                            state::forget_receipt(&self.home, key)?;
                        }
                    }
                    AttachmentState::Missing | AttachmentState::Conflict => {
                        state::forget_receipt(&self.home, key)?;
                    }
                    AttachmentState::Drifted | AttachmentState::Blocked => {}
                }
                // Fall through to the fresh candidate below — do not reuse.
            } else {
                resolved.resolved_exposure_name = existing_name;
                resolved.resolved_artifact_target = match &existing.artifact {
                    uze_core::integration::ManagedArtifact::SymlinkReference { target, .. } => {
                        Some(target.clone())
                    }
                    _ => None,
                };
                return Ok(resolved);
            }
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
        // claimed name here must belong to a DIFFERENT canonical resource —
        // e.g. a legacy Command-era receipt and a Skill both projecting
        // `flow:commit` into the shared `~/.agents/skills` root, or two
        // distinct resources converging on one label: one physical entry,
        // incompatible representations. That is a projection ownership
        // conflict, not drift; report it deterministically before any
        // attach, instead of handing the conflicting name back and failing
        // later with a misleading `ManagedEntryDrift`. (ADR-029; with only
        // one canonical Skill kind, same-name resource collisions are
        // structurally gone — the residual case is legacy receipts.)
        let entry = candidates
            .last()
            .cloned()
            .expect("naming plans always have at least one candidate");
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
                    .map(|root| root.join(&entry))
                    .unwrap_or_else(|| PathBuf::from(&entry)),
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
