//! Package-centric product operations over UZE Core and peer integrations.

pub mod application;
pub mod bootstrap;

pub use application::UzeApplication;
pub use application::services::AgentIdentity;

/// Types the read models above are made of. Presentation consumes these
/// through this crate rather than reaching into the domain for them: a
/// status that carries an `AttachmentState` has to name that type, and
/// making the caller find it elsewhere is what put `uze_core::` in the
/// TUI's imports.
pub use uze_core::{
    capability::CapabilityKind,
    integration::AttachmentState,
    preference::{Autonomy, ModelPreference, PreferenceApplyOutcome, Preferences, SandboxScope},
    project_lock::parse_plugin_marketplace_spec,
    prompt_history::{PromptEntry, PromptOrigin},
    router::HarnessCapabilities,
    workspace::workspace_root_or_self,
    worktree::{IsolatedCheckout, isolated_checkout},
};
