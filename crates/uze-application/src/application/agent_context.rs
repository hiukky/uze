//! How one harness actually receives a project's portable context, at one
//! directory, right now.
//!
//! This is the read model behind the workspace's per-agent support popup
//! and the Harnesses drawer. It exists because the older answer was
//! assembled at the call site out of three unrelated pieces — a
//! project-scoped `context_inspect` resolved at whatever root the TUI
//! happened to attach to, a machine-scoped `HarnessHealth`, and a bridge
//! `needed` flag that actually meant "some installed package contributed a
//! managed region". A harness could be receiving `AGENTS.md` perfectly
//! through the runtime shim while every view reported "not loaded" or "not
//! needed", and the answer changed depending on which directory `uze` was
//! launched from.
//!
//! Two properties fix that, and both are structural rather than
//! conventional: the resolution starts from a directory the *caller* names
//! (an agent pane's own cwd, not a session-wide root), and the two portable
//! resources are answered independently, each naming the mechanism that
//! delivers it.

use std::path::{Path, PathBuf};

use serde::Serialize;
use uze_core::{
    Result, UzeError,
    harness_runtime::RuntimeContext,
    integration::{AttachmentState, ContextDelivery, IntegrationPort},
    project_context, text_region,
};

use super::{INSTRUCTION_BRIDGE_CONTENT, INSTRUCTION_BRIDGE_IDENTITY, services::Workspace};

/// The mechanism actually carrying one portable resource into one harness.
/// Every variant names a mechanism or a specific reason there is none —
/// deliberately never a bare boolean, because "false" was what previously
/// let "the project has no `.agents/`" and "this harness cannot receive
/// one" render as the same misleading row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "delivery", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResourceDelivery {
    /// The project does not carry this resource at all. Not a gap: there
    /// is nothing to deliver.
    AbsentFromProject,
    /// The harness's own binary reads it straight out of the project.
    Native,
    /// UZE's runtime PATH shim projects it into the session at launch,
    /// without writing anything into the project.
    Projected,
    /// A persistent bridge file in the project root carries it.
    Bridged,
    /// The project carries it, but nothing currently delivers it here.
    Undelivered(UndeliveredReason),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UndeliveredReason {
    /// The harness is not installed on this machine.
    HarnessAbsent,
    /// A real harness binary resolves ahead of UZE's shim on this
    /// process's `PATH`, so a launch from here bypasses the projection.
    /// An environment fact, not a defect in the harness or the project.
    ShimShadowed,
    /// The harness's persistent bridge file is not in a usable state.
    Bridge(AttachmentState),
    /// No delivery strategy exists for this harness and this resource.
    Unsupported,
}

impl ResourceDelivery {
    /// Whether this is a real gap a reader should act on — the project has
    /// something the harness is not getting.
    pub fn is_gap(&self) -> bool {
        matches!(self, Self::Undelivered(_))
    }
}

/// One harness's context delivery, resolved against one directory.
#[derive(Clone, Debug, Serialize)]
pub struct AgentContextStatus {
    pub integration: String,
    pub display_name: String,
    /// Whether the harness binary is on this machine at all.
    pub present: bool,
    /// The project root every field below was resolved against — one rule
    /// (`uze_core::project_context`), so a caller can show the reader
    /// exactly which project the answer is about.
    pub root: PathBuf,
    /// Delivery of the shared `AGENTS.md`.
    pub instructions: ResourceDelivery,
    /// Delivery of the portable `.agents/` resource directory.
    pub agents_directory: ResourceDelivery,
}

impl Workspace<'_> {
    /// Resolves how every registered harness receives `cwd`'s project
    /// context. `cwd` is a real working directory — an agent pane's own,
    /// typically — never a pre-resolved root: resolving it here is the
    /// point, so two callers looking at the same directory can never
    /// disagree about which project it belongs to.
    pub fn agent_context(&self, cwd: &Path) -> Vec<AgentContextStatus> {
        let context = project_context::resolve(cwd);
        self.0
            .integrations
            .iter()
            .map(|integration| self.resolve_agent_context(integration.as_ref(), cwd, &context))
            .collect()
    }

    /// The single-harness slice of [`UzeApplication::agent_context`] — what
    /// an agent pane running one known harness needs, without paying for
    /// the others.
    pub fn agent_context_for(
        &self,
        integration_id: &str,
        cwd: &Path,
    ) -> Result<AgentContextStatus> {
        let integration = self
            .0
            .integrations
            .iter()
            .find(|integration| integration.id() == integration_id)
            .ok_or_else(|| {
                UzeError::UnknownPackage(format!("harness `{integration_id}` not found"))
            })?;
        let context = project_context::resolve(cwd);
        Ok(self.resolve_agent_context(integration.as_ref(), cwd, &context))
    }

    fn resolve_agent_context(
        &self,
        integration: &dyn IntegrationPort,
        cwd: &Path,
        context: &project_context::ProjectContext,
    ) -> AgentContextStatus {
        let present = self.0.detect_cached(integration).present;
        // Asked at the caller's `cwd`, not at the resolved root: this is
        // the same question the shim answers when it actually execs the
        // harness from that directory, so the status can never claim a
        // projection the next real launch would not perform.
        let shim_active = self.0.runtime_shim_is_active(integration);
        let projection_active = present
            && shim_active
            && integration.supports_runtime_integration()
            && integration.runtime_projects_project_context()
            && integration.runtime_contribution_would_activate(&RuntimeContext {
                cwd,
                home: &self.0.home,
            });
        // Only a harness that opted into the runtime shim can be shadowed
        // on PATH; for every other one `runtime_shim_is_active` is
        // vacuously true and this reason must never be reachable.
        let shim_shadowed = present && integration.supports_runtime_integration() && !shim_active;

        AgentContextStatus {
            integration: integration.id().to_owned(),
            display_name: integration.display_name().to_owned(),
            present,
            root: context.root.clone(),
            instructions: self.instruction_delivery(
                integration,
                context,
                present,
                projection_active,
                shim_shadowed,
            ),
            agents_directory: agents_directory_delivery(
                integration,
                context,
                present,
                projection_active,
                shim_shadowed,
            ),
        }
    }

    fn instruction_delivery(
        &self,
        integration: &dyn IntegrationPort,
        context: &project_context::ProjectContext,
        present: bool,
        projection_active: bool,
        shim_shadowed: bool,
    ) -> ResourceDelivery {
        if context.agents_md.is_none() {
            return ResourceDelivery::AbsentFromProject;
        }
        if !present {
            return ResourceDelivery::Undelivered(UndeliveredReason::HarnessAbsent);
        }
        match integration.context_delivery() {
            ContextDelivery::Native { .. } => ResourceDelivery::Native,
            ContextDelivery::None => ResourceDelivery::Undelivered(UndeliveredReason::Unsupported),
            ContextDelivery::Bridge { file_name } => {
                // The runtime projection outranks the persistent bridge:
                // when the shim is delivering, a project-root bridge file
                // is redundant, and reporting its absence as a gap is
                // exactly the false alarm this model exists to end.
                if projection_active {
                    return ResourceDelivery::Projected;
                }
                let state = text_region::inspect(
                    &context.root.join(file_name),
                    INSTRUCTION_BRIDGE_IDENTITY,
                    INSTRUCTION_BRIDGE_CONTENT,
                )
                .state;
                if state == AttachmentState::Matched {
                    ResourceDelivery::Bridged
                } else if shim_shadowed {
                    ResourceDelivery::Undelivered(UndeliveredReason::ShimShadowed)
                } else {
                    ResourceDelivery::Undelivered(UndeliveredReason::Bridge(state))
                }
            }
        }
    }
}

fn agents_directory_delivery(
    integration: &dyn IntegrationPort,
    context: &project_context::ProjectContext,
    present: bool,
    projection_active: bool,
    shim_shadowed: bool,
) -> ResourceDelivery {
    if context.agents_directory.is_none() {
        return ResourceDelivery::AbsentFromProject;
    }
    if !present {
        return ResourceDelivery::Undelivered(UndeliveredReason::HarnessAbsent);
    }
    if integration.discovers_project_agents_directory() {
        ResourceDelivery::Native
    } else if projection_active {
        ResourceDelivery::Projected
    } else if shim_shadowed {
        ResourceDelivery::Undelivered(UndeliveredReason::ShimShadowed)
    } else {
        ResourceDelivery::Undelivered(UndeliveredReason::Unsupported)
    }
}
