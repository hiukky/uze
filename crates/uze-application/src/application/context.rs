//! Context delivery (AGENTS.md + bridges) — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::{collections::BTreeSet, path::PathBuf};

use uze_core::{
    Result, UzeError,
    context::{self as instruction_context, InstructionContribution},
    integration::AttachmentState,
    text_region,
};

use super::*;
use super::{
    BRIDGE_INTEGRATIONS, INSTRUCTION_BRIDGE_CONTENT, INSTRUCTION_BRIDGE_IDENTITY,
    NATIVE_INSTRUCTION_INTEGRATIONS, UzeApplication,
};

impl UzeApplication {
    pub fn context_inspect(&self, project_root: &std::path::Path) -> Result<ProjectContextStatus> {
        if !project_root.is_dir() {
            return Err(UzeError::NotDirectory(project_root.to_path_buf()));
        }
        let canonical = project_root
            .canonicalize()
            .map_err(|source| UzeError::Read {
                path: project_root.to_path_buf(),
                source,
            })?;

        let agents_md_path = canonical.join("AGENTS.md");
        let contributions_input = self.instruction_contributions()?;
        let observation =
            instruction_context::inspect_agents_md(&agents_md_path, &contributions_input);
        let agents_md_exists = agents_md_path.is_file();

        let sources: Vec<InstructionSourceObservation> = ["AGENTS.md", "CLAUDE.md", "GEMINI.md"]
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
        for integration in &self.integrations {
            let id = integration.id();
            let is_native = NATIVE_INSTRUCTION_INTEGRATIONS.contains(&id);
            let bridge_file_name = BRIDGE_INTEGRATIONS
                .iter()
                .find(|(bridge_id, _)| *bridge_id == id)
                .map(|(_, file_name)| *file_name);
            if !is_native && bridge_file_name.is_none() {
                // Not a harness this milestone models Instructions delivery
                // for at all; silently excluded rather than reported as a
                // gap it was never claimed to close.
                continue;
            }
            if !self.detect_cached(integration.as_ref()).present {
                harnesses.push(HarnessContextStatus {
                    integration: id.to_owned(),
                    display_name: integration.display_name().to_owned(),
                    delivery: HarnessContextDelivery::NotDetected,
                });
                continue;
            }
            if is_native {
                harnesses.push(HarnessContextStatus {
                    integration: id.to_owned(),
                    display_name: integration.display_name().to_owned(),
                    delivery: HarnessContextDelivery::Native,
                });
                continue;
            }
            let bridge_file = canonical.join(bridge_file_name.expect("checked above"));
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
            portability,
            warnings,
        })
    }

    pub fn context_plan(&self, project_root: &std::path::Path) -> Result<ContextPlan> {
        if !project_root.is_dir() {
            return Err(UzeError::NotDirectory(project_root.to_path_buf()));
        }
        let agents_md = project_root.join("AGENTS.md");
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

        let bridges = BRIDGE_INTEGRATIONS
            .iter()
            .filter_map(|(integration_id, file_name)| {
                let integration = self
                    .integrations
                    .iter()
                    .find(|integration| integration.id() == *integration_id)?;
                if !self.detect_cached(integration.as_ref()).present {
                    return None;
                }
                let bridge_file = project_root.join(file_name);
                let state = text_region::inspect(
                    &bridge_file,
                    INSTRUCTION_BRIDGE_IDENTITY,
                    INSTRUCTION_BRIDGE_CONTENT,
                )
                .state;
                Some(BridgePlan {
                    integration: (*integration_id).to_owned(),
                    file: bridge_file,
                    action: plan_action_for_bridge(would_have_contribution, state),
                })
            })
            .collect();

        Ok(ContextPlan {
            agents_md,
            agents_md_plan,
            bridges,
        })
    }

    pub fn context_reconcile(
        &self,
        project_root: &std::path::Path,
    ) -> Result<ContextReconciliationReport> {
        if !project_root.is_dir() {
            return Err(UzeError::NotDirectory(project_root.to_path_buf()));
        }
        let agents_md = project_root.join("AGENTS.md");
        let contributions = self.instruction_contributions()?;
        let agents_md_report = instruction_context::reconcile_agents_md(&agents_md, &contributions);

        let bridges = BRIDGE_INTEGRATIONS
            .iter()
            .filter_map(|(integration_id, file_name)| {
                let integration = self
                    .integrations
                    .iter()
                    .find(|integration| integration.id() == *integration_id)?;
                if !self.detect_cached(integration.as_ref()).present {
                    return None;
                }
                let bridge_file = project_root.join(file_name);
                let inspection = text_region::reconcile(
                    &bridge_file,
                    INSTRUCTION_BRIDGE_IDENTITY,
                    INSTRUCTION_BRIDGE_CONTENT,
                    agents_md_report.has_any_matched_contribution(),
                );
                Some(BridgeStatus {
                    integration: (*integration_id).to_owned(),
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
            bridges,
        })
    }

    fn instruction_contributions(&self) -> Result<Vec<InstructionContribution>> {
        let mut contributions = Vec::new();
        for package_id in self.store.package_ids()? {
            let package = self.store.package(&package_id)?;
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

fn plan_action_for_bridge(
    needed: bool,
    state: AttachmentState,
) -> instruction_context::PlannedAction {
    use instruction_context::PlannedAction;
    match (needed, state) {
        (true, AttachmentState::Matched) | (false, AttachmentState::Missing) => {
            PlannedAction::NoChange
        }
        (true, AttachmentState::Missing) => PlannedAction::Attach,
        (false, AttachmentState::Matched) => PlannedAction::Remove,
        (_, AttachmentState::Drifted) => PlannedAction::Blocked(
            "bridge content differs from the expected import line".to_owned(),
        ),
        (_, AttachmentState::Blocked) => {
            PlannedAction::Blocked("bridge region markers are malformed".to_owned())
        }
        (_, AttachmentState::Conflict) => {
            PlannedAction::Blocked("bridge region ownership is ambiguous".to_owned())
        }
    }
}
