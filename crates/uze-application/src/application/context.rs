//! Context delivery (AGENTS.md + bridges) — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::{collections::BTreeSet, path::PathBuf};

use uze_core::{
    Result, UzeError,
    context::{self as instruction_context, InstructionContribution},
    integration::{AttachmentState, ContextDelivery},
    text_region,
    worktree::{self, WorktreePolicy},
};

use super::services::Context;
use super::*;
use super::{INSTRUCTION_BRIDGE_CONTENT, INSTRUCTION_BRIDGE_IDENTITY};

impl Context<'_> {
    pub fn inspect(&self, project_root: &std::path::Path) -> Result<ProjectContextStatus> {
        if !project_root.is_dir() {
            return Err(UzeError::NotDirectory(project_root.to_path_buf()));
        }
        // Resolves upward the same way `add_project_plugin`/
        // `install_project_environment` do (nearest `agents.lock`/
        // `AGENTS.md`/`.git`) — a caller pointing at a subdirectory of a
        // project must land on the same root every other project-scoped
        // command finds, not silently inspect the subdirectory itself as
        // if it had no context at all.
        let canonical = uze_core::project_root::resolve_project_root(project_root)?;

        let agents_md_path = canonical.join("AGENTS.md");
        let contributions_input = self.instruction_contributions()?;
        let observation =
            instruction_context::inspect_agents_md(&agents_md_path, &contributions_input);
        let agents_md_exists = agents_md_path.is_file();

        // The observed project files are the shared canonical `AGENTS.md`
        // plus whatever each registered integration declares through
        // `context_delivery` (its bridge file or additional native files) —
        // never an Application-owned list of filenames.
        let mut source_names = vec!["AGENTS.md"];
        for integration in &self.0.integrations {
            match integration.context_delivery() {
                ContextDelivery::Bridge { file_name } => source_names.push(file_name),
                ContextDelivery::Native { files } => source_names.extend(files),
                ContextDelivery::None => {}
            }
        }
        source_names.dedup();
        let sources: Vec<InstructionSourceObservation> = source_names
            .iter()
            .map(|file_name| {
                let path = canonical.join(file_name);
                let exists = path.is_file();
                InstructionSourceObservation {
                    file_name: (*file_name).to_owned(),
                    path: path.clone(),
                    exists,
                    has_user_content: exists
                        && text_region::has_content_outside_managed_regions(&path),
                    managed_region_identities: if exists {
                        let mut identities: Vec<String> =
                            text_region::region_identities_present(&path)
                                .into_iter()
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect();
                        identities.sort();
                        identities
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect();

        let mut harnesses = Vec::new();
        for integration in &self.0.integrations {
            let id = integration.id();
            let delivery = integration.context_delivery();
            if matches!(delivery, ContextDelivery::None) {
                // Not a harness this integration declares Instructions
                // delivery for at all; silently excluded rather than
                // reported as a gap it was never claimed to close.
                continue;
            }
            if !self.0.detect_cached(integration.as_ref()).present {
                harnesses.push(HarnessContextStatus {
                    integration: id.to_owned(),
                    display_name: integration.display_name().to_owned(),
                    delivery: HarnessContextDelivery::NotDetected,
                });
                continue;
            }
            match delivery {
                ContextDelivery::Native { .. } => {
                    harnesses.push(HarnessContextStatus {
                        integration: id.to_owned(),
                        display_name: integration.display_name().to_owned(),
                        delivery: HarnessContextDelivery::Native,
                    });
                }
                ContextDelivery::Bridge { file_name } => {
                    let bridge_file = canonical.join(file_name);
                    let state = text_region::inspect(
                        &bridge_file,
                        INSTRUCTION_BRIDGE_IDENTITY,
                        INSTRUCTION_BRIDGE_CONTENT,
                    )
                    .state;
                    harnesses.push(HarnessContextStatus {
                        integration: id.to_owned(),
                        display_name: integration.display_name().to_owned(),
                        delivery: HarnessContextDelivery::Bridge {
                            needed: observation.has_any_matched_contribution(),
                            state,
                        },
                    });
                }
                ContextDelivery::None => unreachable!("excluded above"),
            }
        }

        let worktrees = self
            .worktree_policy(&canonical)?
            .map(|policy| self.worktree_policy_status(&canonical, &agents_md_path, &policy));

        let portability = derive_portability(agents_md_exists, &sources, &harnesses);
        let warnings = derive_warnings(agents_md_exists, &sources);

        Ok(ProjectContextStatus {
            root: project_root.to_path_buf(),
            canonical,
            sources,
            contributions: observation
                .packages
                .into_iter()
                .map(|(package_id, inspection)| PackageInstructionStatus {
                    package_id: package_id.as_str().to_owned(),
                    state: inspection.state,
                    reason: inspection.reason,
                })
                .collect(),
            orphaned_regions: observation.orphaned_regions,
            malformed_regions: observation.malformed_regions,
            harnesses,
            worktrees,
            portability,
            warnings,
        })
    }

    pub fn plan(&self, project_root: &std::path::Path) -> Result<ContextPlan> {
        if !project_root.is_dir() {
            return Err(UzeError::NotDirectory(project_root.to_path_buf()));
        }
        let canonical = uze_core::project_root::resolve_project_root(project_root)?;
        let agents_md = canonical.join("AGENTS.md");
        let contributions = self.instruction_contributions()?;
        let agents_md_plan = instruction_context::plan_agents_md(&agents_md, &contributions);

        // Bridge planning needs the same "would AGENTS.md end up with a
        // matched contribution" question `context_reconcile` asks, computed
        // the same read-only way: attach-or-not never actually ran here.
        let observation = instruction_context::inspect_agents_md(&agents_md, &contributions);
        let would_have_contribution = agents_md_plan.contributions.iter().any(|plan| {
            matches!(
                plan.action,
                instruction_context::PlannedAction::Attach
                    | instruction_context::PlannedAction::NoChange
            )
        }) || observation.has_any_matched_contribution();

        let bridges = self
            .0
            .integrations
            .iter()
            .filter_map(|integration| {
                let ContextDelivery::Bridge { file_name } = integration.context_delivery() else {
                    return None;
                };
                if !self.0.detect_cached(integration.as_ref()).present {
                    return None;
                }
                let bridge_file = canonical.join(file_name);
                let state = text_region::inspect(
                    &bridge_file,
                    INSTRUCTION_BRIDGE_IDENTITY,
                    INSTRUCTION_BRIDGE_CONTENT,
                )
                .state;
                Some(BridgePlan {
                    integration: integration.id().to_owned(),
                    file: bridge_file,
                    action: plan_action_for_region(would_have_contribution, state, "bridge"),
                })
            })
            .collect();

        let worktree_region = self.worktree_policy(&canonical)?.map(|policy| {
            let state = text_region::inspect(
                &agents_md,
                &policy.region_identity(),
                &policy.instructions(),
            )
            .state;
            WorktreeRegionPlan {
                file: agents_md.clone(),
                action: plan_action_for_region(true, state, "worktree policy"),
                superseded: superseded_policy_regions(&agents_md, &policy),
            }
        });

        Ok(ContextPlan {
            agents_md,
            agents_md_plan,
            worktree_region,
            bridges,
        })
    }

    pub fn reconcile(&self, project_root: &std::path::Path) -> Result<ContextReconciliationReport> {
        if !project_root.is_dir() {
            return Err(UzeError::NotDirectory(project_root.to_path_buf()));
        }
        let canonical = uze_core::project_root::resolve_project_root(project_root)?;
        let agents_md = canonical.join("AGENTS.md");
        let contributions = self.instruction_contributions()?;
        let agents_md_report = instruction_context::reconcile_agents_md(&agents_md, &contributions);

        // The policy region is reconciled against the shared file before the
        // bridges are, for the same reason package contributions are: a
        // bridge's own desired-state question is answered by what `AGENTS.md`
        // ends up carrying, not by what it carried on entry.
        let declared_policy = self.worktree_policy(&canonical)?;
        let worktree_region = declared_policy.as_ref().map(|policy| {
            // A superseded region goes before the current one is written, so
            // the file never briefly carries two statements of the same
            // policy. Removal is structural (`remove_unconditionally`): the
            // region's content was UZE's to render, and the authored source
            // it came from is the lock, not the file.
            let mut removed = Vec::new();
            let mut blocked = Vec::new();
            for identity in superseded_policy_regions(&agents_md, policy) {
                match text_region::remove_unconditionally(&agents_md, &identity) {
                    Ok(inspection) if inspection.state == AttachmentState::Missing => {
                        removed.push(identity);
                    }
                    Ok(inspection) => blocked.push((identity, inspection.reason)),
                    Err(error) => blocked.push((identity, error.to_string())),
                }
            }
            let inspection = text_region::reconcile(
                &agents_md,
                &policy.region_identity(),
                &policy.instructions(),
                true,
            );
            WorktreeRegionStatus {
                file: agents_md.clone(),
                state: inspection.state,
                reason: inspection.reason,
                removed_superseded: removed,
                blocked_superseded: blocked,
            }
        });

        let bridges = self
            .0
            .integrations
            .iter()
            .filter_map(|integration| {
                let ContextDelivery::Bridge { file_name } = integration.context_delivery() else {
                    return None;
                };
                if !self.0.detect_cached(integration.as_ref()).present {
                    return None;
                }
                let bridge_file = canonical.join(file_name);
                let inspection = text_region::reconcile(
                    &bridge_file,
                    INSTRUCTION_BRIDGE_IDENTITY,
                    INSTRUCTION_BRIDGE_CONTENT,
                    agents_md_report.has_any_matched_contribution(),
                );
                Some(BridgeStatus {
                    integration: integration.id().to_owned(),
                    file: bridge_file,
                    state: inspection.state,
                    reason: inspection.reason,
                })
            })
            .collect();

        Ok(ContextReconciliationReport {
            agents_md,
            packages: agents_md_report
                .packages
                .into_iter()
                .map(|(package_id, inspection)| PackageInstructionStatus {
                    package_id: package_id.as_str().to_owned(),
                    state: inspection.state,
                    reason: inspection.reason,
                })
                .collect(),
            removed_orphans: agents_md_report.removed_orphans,
            blocked_orphans: agents_md_report.blocked_orphans,
            worktree_region,
            bridges,
        })
    }

    /// This project's declared isolation policy, or `None` when `agents.lock`
    /// declares none. A malformed lock is an error here rather than a silent
    /// "no policy": dropping a declared policy without saying so is exactly
    /// the failure that left `worktrees_dir` unprojected for so long.
    fn worktree_policy(&self, canonical: &std::path::Path) -> Result<Option<WorktreePolicy>> {
        Ok(uze_core::project_lock::load_lock(canonical)?.and_then(|lock| lock.worktrees))
    }

    /// Composes the policy's current standing: its managed region in the
    /// shared file, each harness's honest route, and the checkouts on disk
    /// the policy does not account for.
    fn worktree_policy_status(
        &self,
        canonical: &std::path::Path,
        agents_md: &std::path::Path,
        policy: &WorktreePolicy,
    ) -> WorktreePolicyStatus {
        let inspection =
            text_region::inspect(agents_md, &policy.region_identity(), &policy.instructions());
        WorktreePolicyStatus {
            directory: canonical.join(worktree::WORKTREES_DIRECTORY),
            completion: policy.completion,
            state: inspection.state,
            reason: inspection.reason,
            superseded_regions: superseded_policy_regions(agents_md, policy),
        }
    }

    fn instruction_contributions(&self) -> Result<Vec<InstructionContribution>> {
        let mut contributions = Vec::new();
        for package_id in self.0.store.package_ids()? {
            let package = self.0.store.package(&package_id)?;
            let resources = uze_core::engine::package_resources_at(&package_id, &package.root)?;
            for resource in resources {
                if resource.capability.kind != CapabilityKind::Instruction {
                    continue;
                }
                contributions.push(InstructionContribution {
                    package_id: package_id.clone(),
                    content: String::from_utf8_lossy(&resource.capability.payload).into_owned(),
                });
            }
        }
        Ok(contributions)
    }
}

fn derive_portability(
    agents_md_exists: bool,
    sources: &[InstructionSourceObservation],
    harnesses: &[HarnessContextStatus],
) -> Portability {
    if !agents_md_exists {
        let vendor_files: Vec<PathBuf> = sources
            .iter()
            .filter(|source| {
                source.file_name != "AGENTS.md" && source.exists && source.has_user_content
            })
            .map(|source| source.path.clone())
            .collect();
        return if vendor_files.is_empty() {
            Portability::NoContext
        } else {
            Portability::VendorLocked {
                files: vendor_files,
            }
        };
    }
    let gaps: Vec<String> = harnesses
        .iter()
        .filter_map(|harness| match &harness.delivery {
            HarnessContextDelivery::Bridge {
                needed: true,
                state,
            } if *state != AttachmentState::Matched => {
                Some(format!("{}: bridge {:?}", harness.integration, state))
            }
            _ => None,
        })
        .collect();
    if gaps.is_empty() {
        Portability::Portable
    } else {
        Portability::PartiallyPortable { gaps }
    }
}

fn derive_warnings(
    agents_md_exists: bool,
    sources: &[InstructionSourceObservation],
) -> Vec<String> {
    let mut warnings = Vec::new();
    let vendor_specific_with_content: Vec<&InstructionSourceObservation> = sources
        .iter()
        .filter(|source| {
            source.file_name != "AGENTS.md" && source.exists && source.has_user_content
        })
        .collect();
    if !agents_md_exists && vendor_specific_with_content.len() >= 2 {
        let names: Vec<&str> = vendor_specific_with_content
            .iter()
            .map(|source| source.file_name.as_str())
            .collect();
        warnings.push(format!(
            "{} each carry their own content with no shared AGENTS.md — these are observed as \
             independent, potentially divergent vendor-specific sources; UZE does not compare or \
             consolidate them.",
            names.join(" and ")
        ));
    }
    if agents_md_exists {
        for source in &vendor_specific_with_content {
            warnings.push(format!(
                "{} carries content beyond the shared bridge — this is expected and supported \
                 (vendor-specific instructions alongside portable ones), not a gap.",
                source.file_name
            ));
        }
    }
    warnings
}

/// Regions in `agents_md` that this module's own naming shape claims but
/// that the current policy does not — what a previous policy left behind.
/// Structural, exactly like `uze_core::context`'s orphan detection: an
/// identity is claimed by shape, never by comparing rendered content.
fn superseded_policy_regions(agents_md: &std::path::Path, policy: &WorktreePolicy) -> Vec<String> {
    let current = policy.region_identity();
    let mut stale: Vec<String> = text_region::region_identities_present(agents_md)
        .into_iter()
        .filter(|identity| WorktreePolicy::owns_region(identity) && *identity != current)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    stale.sort();
    stale
}

/// The action a managed region needs to reach its desired presence. Shared
/// by every region this module owns — a bridge's import line and the
/// worktree policy alike — because the safety rules are `text_region`'s, not
/// each caller's; `subject` only names the region in a blocked explanation.
fn plan_action_for_region(
    needed: bool,
    state: AttachmentState,
    subject: &str,
) -> instruction_context::PlannedAction {
    use instruction_context::PlannedAction;
    match (needed, state) {
        (true, AttachmentState::Matched) | (false, AttachmentState::Missing) => {
            PlannedAction::NoChange
        }
        (true, AttachmentState::Missing) => PlannedAction::Attach,
        (false, AttachmentState::Matched) => PlannedAction::Remove,
        (_, AttachmentState::Drifted) => PlannedAction::Blocked(format!(
            "{subject} content differs from what UZE would write"
        )),
        (_, AttachmentState::Blocked) => {
            PlannedAction::Blocked(format!("{subject} region markers are malformed"))
        }
        (_, AttachmentState::Conflict) => {
            PlannedAction::Blocked(format!("{subject} region ownership is ambiguous"))
        }
    }
}
