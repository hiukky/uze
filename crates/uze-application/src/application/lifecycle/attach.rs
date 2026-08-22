//! Lifecycle — attach — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::collections::BTreeSet;

use uze_core::{
    Result,
    capability::CapabilityKind,
    integration::{AttachmentState, IntegrationPort, managed_artifact_exposure_name},
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
                let resolved = self.resolve_exposure_name(resource, integration);
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
    ) -> Resource {
        if !matches!(
            resource.capability.kind,
            CapabilityKind::AgentSkill | CapabilityKind::Mcp
        ) {
            return resource.clone();
        }
        let mut resolved = resource.clone();
        let Ok(all_receipts) = state::receipts(&self.home, None) else {
            return resolved;
        };
        let resource_id = resource.identity();
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
        if let Some((_, existing)) = all_receipts.iter().find(|(_, receipt)| {
            receipt.resource_identity.as_deref() == Some(resource_id.as_str())
                && (receipt.integration == integration.id() || shares_root(&receipt.integration))
        }) {
            resolved.resolved_exposure_name = managed_artifact_exposure_name(&existing.artifact);
            resolved.resolved_artifact_target = match &existing.artifact {
                uze_core::integration::ManagedArtifact::SymlinkReference { target, .. } => {
                    Some(target.clone())
                }
                _ => None,
            };
            return resolved;
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
        // Codex/Gemini/OpenCode attaches before OpenCode would lock the
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
        resolved.resolved_exposure_name = candidates
            .iter()
            .find(|candidate| !claimed.contains(*candidate))
            .cloned()
            .or_else(|| candidates.last().cloned());
        resolved
    }
}
