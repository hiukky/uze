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

use std::path::{Path, PathBuf};

use uze_core::{Result, UzeError, hook, prompt_history, workspace, worktree};

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

impl Workspace<'_> {
    /// The workspace root a directory belongs to, or the directory itself.
    ///
    /// One repository is one terminal server, and this is the answer both
    /// the server key and the prompt history are keyed on — resolved once,
    /// here, rather than twice at two call sites.
    pub fn root(&self, cwd: &Path) -> PathBuf {
        workspace::workspace_root_or_self(cwd)
    }

    /// The isolated checkout a path sits in, or `None` when it is not
    /// isolated. Lexical against the fixed layout: a display asks this of
    /// every open tab on every frame.
    pub fn isolated_checkout<'a>(&self, path: &'a Path) -> Option<worktree::IsolatedCheckout<'a>> {
        worktree::isolated_checkout(path)
    }

    /// The harnesses this installation can recognize, as descriptors.
    pub fn agent_identities(&self) -> Vec<AgentIdentity> {
        self.0
            .integrations
            .iter()
            .map(|integration| AgentIdentity {
                binary: integration
                    .aliases()
                    .first()
                    .copied()
                    .unwrap_or(integration.id()),
                integration: integration.id(),
                display_name: integration.display_name(),
            })
            .collect()
    }

    /// Recent prompts submitted into the agent tabs of `root`'s workspace.
    pub fn prompt_history(&self, root: &Path, limit: usize) -> Vec<prompt_history::PromptEntry> {
        prompt_history::list_for_workspace(&self.0.home, root, limit)
    }

    /// Records one prompt submitted into an agent tab of `root`'s
    /// workspace. Best-effort by construction: an empty prompt is ignored
    /// rather than refused.
    pub fn record_prompt(
        &self,
        root: &Path,
        origin: &prompt_history::PromptOrigin,
        prompt: &str,
    ) -> Result<()> {
        prompt_history::record(&self.0.home, root, origin, prompt)
    }

    /// Forgets every prompt recorded for `root`'s workspace.
    pub fn clear_prompt_history(&self, root: &Path) -> Result<()> {
        prompt_history::clear(&self.0.home, root)
    }

    /// Where a newly created agent should start.
    ///
    /// The seat rule: the primary checkout holds one agent at a time. The
    /// first agent in a repository starts there and sees the operator's
    /// uncommitted work; every additional live agent starts in an isolated
    /// checkout of its own, so two can never write to the same files.
    ///
    /// `occupied` is the working directory of every agent pane already
    /// alive — the caller knows which panes are agents, and this decides
    /// what that means. Falls back to the seat whenever isolation is
    /// impossible (not a Git repository, no commit to branch from, Git
    /// absent): launching an agent unisolated beats not launching it.
    pub fn checkout_for_new_agent(
        &self,
        pane_cwd: &Path,
        occupied: &[PathBuf],
        slug: &str,
    ) -> PathBuf {
        let Some(primary) = worktree::primary_checkout(pane_cwd) else {
            return pane_cwd.to_path_buf();
        };
        let seat_taken = occupied
            .iter()
            .any(|cwd| worktree::is_in_primary(&primary, cwd));
        if !seat_taken {
            return primary;
        }
        worktree::isolate(&primary, slug).unwrap_or_else(|_| primary.clone())
    }
}

#[cfg(test)]
mod seat_tests {
    use super::*;
    use uze_core::UzeHome;

    fn repository(label: &str) -> uze_testkit::git::Repository {
        uze_testkit::git::Repository::new(label)
    }

    fn application(label: &str) -> UzeApplication {
        UzeApplication::new(UzeHome::at(uze_testkit::temp::scratch(label)), Vec::new())
    }

    /// The guarantee this rule exists for: the primary checkout holds one
    /// agent, and every additional live agent gets a checkout of its own.
    #[test]
    fn two_agents_never_share_a_checkout() {
        let repository = repository("seat-two-agents");
        let root = repository.root().to_path_buf();
        let app = application("seat-two-agents-home");

        let first = app
            .workspace()
            .checkout_for_new_agent(&root, &[], "agent-1");
        assert_eq!(first, root.canonicalize().unwrap_or(root.clone()));

        let second = app
            .workspace()
            .checkout_for_new_agent(&root, &[first.clone()], "agent-2");
        assert_ne!(second, first, "the seat is taken, so this one isolates");
        assert!(
            second.join("README.md").is_file(),
            "the checkout is populated"
        );
    }

    /// Occupancy is judged by the checkout a pane is in, never by an exact
    /// path — otherwise an agent that `cd`s one level down would free the
    /// seat and let a second agent in beside it.
    #[test]
    fn an_agent_that_moved_inside_the_primary_still_holds_it() {
        let repository = repository("seat-moved");
        let root = repository.root().to_path_buf();
        let app = application("seat-moved-home");
        let inside = root.join("subdirectory");
        std::fs::create_dir_all(&inside).unwrap();

        let taken = app
            .workspace()
            .checkout_for_new_agent(&root, &[inside], "agent-2");
        assert_ne!(taken, root.canonicalize().unwrap_or(root.clone()));
    }

    /// An isolated checkout lives under the same repository but must never
    /// read as occupying the seat, or every agent after the first would be
    /// told the seat is taken by somebody who already left it.
    #[test]
    fn an_isolated_agent_leaves_the_seat_free() {
        let repository = repository("seat-isolated");
        let root = repository.root().to_path_buf();
        let app = application("seat-isolated-home");
        let isolated = root.join(".worktrees").join("agent-1");

        let seat = app
            .workspace()
            .checkout_for_new_agent(&root, &[isolated], "agent-2");
        assert_eq!(seat, root.canonicalize().unwrap_or(root.clone()));
    }

    /// Launching an agent unisolated beats not launching it at all.
    #[test]
    fn a_directory_outside_any_repository_falls_back_to_itself() {
        let outside = uze_testkit::temp::scratch("seat-no-repo");
        let app = application("seat-no-repo-home");
        assert_eq!(
            app.workspace()
                .checkout_for_new_agent(&outside, &[], "agent-1"),
            outside
        );
        std::fs::remove_dir_all(outside).unwrap();
    }
}

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
