//! Package-centric product operations over UZE Core and peer integrations.

pub mod application;
pub mod bootstrap;

pub use application::UzeApplication;
pub use application::services::{
    AgentIdentity, AgentNotice, AgentPlacement, DeliveryOutcome, DeliveryPolicyView,
    DeliveryReport, Evaluation, Isolation, TaskStateView, TaskView,
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
