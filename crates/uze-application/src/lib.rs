//! Package-centric product operations over UZE Core and peer integrations.

pub mod application;
pub mod bootstrap;

pub use application::UzeApplication;
pub use application::services::{
    AgentIdentity, AgentNotice, AgentPlacement, DeliveryOutcome, DeliveryPolicyView,
    DeliveryReport, Evaluation, Isolation, ReleasedTask, TaskStateView, TaskView, UpstreamSync,
};

/// Types the read models above are made of. Presentation consumes these
/// through this crate rather than reaching into the domain for them: a
/// status that carries an `AttachmentState` has to name that type, and
/// making the caller find it elsewhere is what put `uze_core::` in the
/// TUI's imports.
pub use uze_core::{
    ExposureMechanism, ExposurePlan, PackageExposurePlan, Result, UzeError, UzeHome,
    capability::CapabilityKind,
    context::PlannedAction,
    hook::{
        CommandHandlerType, CommandHook, DEFAULT_TIMEOUT_SECONDS, HookEffect, HookEvent,
        HookNativeOutput,
    },
    integration::{AttachmentState, PublicationStatus},
    naming::{
        FixedResolution, NameCollisionAuthority, NameCollisionRequest, NameCollisionResolution,
        NoNameCollisionAuthority,
    },
    preference::{Autonomy, ModelPreference, PreferenceApplyOutcome, Preferences, SandboxScope},
    project_lock::parse_plugin_marketplace_spec,
    prompt_history::{PromptEntry, PromptOrigin},
    provisioning::{ProcessOutput, ProcessResult, ProcessRunner, ProcessSpec, SystemProcessRunner},
    router::CompatibilityRoute,
    router::HarnessCapabilities,
    trust::{AlwaysTrust, NoTrustAuthority, TrustAuthority, TrustOutcome, TrustRequest},
    workspace::workspace_root_or_self,
    worktree::{IsolatedCheckout, isolated_checkout},
};

/// The root of the space a directory belongs to.
///
/// A slot is never a space of its own. It is a checkout of the project, so
/// it carries the project's own anchor files, and asking the workspace
/// resolver alone answers the slot itself — opening a second space over one
/// repository, rooted inside `.worktrees`. An agent's checkout belongs to
/// the space its repository already has.
pub fn space_root(cwd: &std::path::Path) -> std::path::PathBuf {
    let base = isolated_checkout(cwd)
        .map(|checkout| checkout.primary.to_path_buf())
        .unwrap_or_else(|| cwd.to_path_buf());
    workspace_root_or_self(&base)
}
