//! Capability-scoped views onto [`UzeApplication`].
//!
//! One handle that can do everything is the shape that makes "I want to add
//! one feature and I have to touch the middle" true: every operation lands
//! on the same `impl`, and the type says nothing about what a caller is
//! allowed to reach. It also makes scoping inexpressible — "this caller may
//! read packages but must never write the Store" has no way to be said when
//! the only handle there is can do both.
//!
//! Each service here is a borrowed view: no state of its own, no cost, and
//! the state stays owned in one place. What changes is that a caller now
//! names the capability it wants, and gets only that.
//!
//! Service boundaries follow the module the operations already lived in —
//! those files were drawn deliberately, and redrawing them in the same
//! change would have made the diff argue two things at once.

use uze_core::{Result, UzeError, hook};

use super::UzeApplication;

/// The current directory's workspace, as presentation needs to see it:
/// what kind of workspace it is and how each harness is actually receiving
/// its context.
///
/// Read-only by construction. Nothing reachable from here writes.
pub struct Workspace<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Read models about the workspace a directory sits in.
    pub fn workspace(&self) -> Workspace<'_> {
        Workspace(self)
    }
}

/// Universal user preferences and the profiles that carry them.
pub struct Profiles<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Reading and applying user preference profiles.
    pub fn profiles(&self) -> Profiles<'_> {
        Profiles(self)
    }
}

/// Which theme is active, and which themes this machine has.
pub struct Themes<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Selecting a theme. Only the selection: what a theme *is* belongs to
    /// the design system, which the domain does not name.
    pub fn themes(&self) -> Themes<'_> {
        Themes(self)
    }
}

/// Marketplaces this machine knows, and the plugins they publish.
pub struct Marketplace<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Marketplace registration, catalogue reads, and installs sourced
    /// from one.
    pub fn marketplace(&self) -> Marketplace<'_> {
        Marketplace(self)
    }
}

/// Whether this machine's UZE installation is sound, and the work to make
/// it so.
pub struct Health<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Diagnostics, harness presence, and bounded environment maintenance.
    pub fn health(&self) -> Health<'_> {
        Health(self)
    }
}

/// A project's portable instruction context — `AGENTS.md` and the bridges
/// projected from it.
///
/// Separate from [`Project`] because the CLI grammar already draws that
/// line (ADR-019): `uze context …` is its own command group, scoped to a
/// directory, and it never touches the machine-scoped environment.
pub struct Context<'a>(pub(super) &'a UzeApplication);

/// A project's declared agent environment — `agents.lock` and what it
/// takes to satisfy it.
pub struct Project<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Instruction context: inspect, plan, reconcile.
    pub fn context(&self) -> Context<'_> {
        Context(self)
    }

    /// The project environment a directory declares.
    pub fn project(&self) -> Project<'_> {
        Project(self)
    }
}

/// The plugins this machine has installed, and their lifecycle.
///
/// The only service here that writes the Store.
pub struct Plugins<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Installing, removing, updating and reading installed plugins.
    pub fn plugins(&self) -> Plugins<'_> {
        Plugins(self)
    }
}

/// One agent harness as presentation needs to recognize it: the binary a
/// running process would be called, and the label to show instead of that
/// binary's raw name.
///
/// Built from the integrations this application already holds, so no
/// caller has to reach the registry — naming a concrete harness is what
/// `cli_and_tui_never_name_a_vendor_harness` forbids, and a descriptor is
/// how presentation stays on the right side of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentIdentity {
    pub binary: &'static str,
    pub integration: &'static str,
    pub display_name: &'static str,
}

/// The workspace service's own file — named for what it holds rather
/// than for the handle, since `workspace` is already `uze-core`'s module
/// for resolving a project root.
mod tasks;

pub use tasks::*;

/// Portable hooks, and the translation into a harness's native contract.
pub struct Hooks<'a>(pub(super) &'a UzeApplication);

impl UzeApplication {
    /// Dispatching an authored hook on a harness's behalf.
    pub fn hooks(&self) -> Hooks<'_> {
        Hooks(self)
    }
}

impl Hooks<'_> {
    /// Runs the handlers a harness's native hook payload asks for, and
    /// renders the answer back in that harness's own contract.
    ///
    /// The whole translation — native payload in, native decision out —
    /// belongs here rather than in the CLI: the only part of it that is
    /// presentation is reading stdin and writing stdout, which is exactly
    /// what the caller is left holding.
    pub fn dispatch(
        &self,
        adapter_id: &str,
        event: hook::HookEvent,
        effect: hook::HookEffect,
        plugin_root: &std::path::Path,
        commands: Vec<String>,
        native: &serde_json::Value,
    ) -> Result<hook::HookNativeOutput> {
        let registry = uze_integrations::registry::IntegrationRegistry::builtin(&self.0.home)?;
        let adapter = registry.hook_adapter(adapter_id).ok_or_else(|| {
            UzeError::HookDispatch(format!("unknown hook adapter `{adapter_id}`"))
        })?;
        let input = adapter
            .normalize_input(native, event)
            .map_err(UzeError::HookDispatch)?;
        let authored = hook::PortableHook {
            id: "dispatch".to_owned(),
            event,
            matchers: Vec::new(),
            handlers: commands
                .into_iter()
                .map(|command| hook::CommandHook {
                    handler_type: hook::CommandHandlerType::Command,
                    command,
                    timeout: hook::DEFAULT_TIMEOUT_SECONDS,
                })
                .collect(),
            effect,
            order: 0,
        };
        let outcome = hook::dispatch_handlers(&authored, &input, plugin_root)?;
        adapter
            .render_output(&outcome, event)
            .map_err(UzeError::HookDispatch)
    }
}
