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

use uze_core::{
    Result, UzeError, checkout, hook, prompt_history,
    task::{self, Base, Task, TaskId},
    workspace, worktree,
};

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

    /// Where a newly created agent starts, decided before its harness does.
    ///
    /// Every agent isolates: a slot is acquired for a new task in the
    /// repository `pane_cwd` belongs to, and the primary checkout is never
    /// assigned to an agent. Where isolation is impossible — no
    /// repository, no branch, no commit to branch from, Git refusing — the
    /// agent starts in place and the placement says why, so the tab can.
    pub fn place_new_agent(&self, pane_cwd: &Path) -> AgentPlacement {
        let Some(primary) = worktree::primary_checkout(pane_cwd) else {
            return AgentPlacement::unisolated(pane_cwd, "not inside a Git working tree");
        };
        let Some(target) = checkout::current_branch(&primary) else {
            return AgentPlacement::unisolated(&primary, "the primary checkout is not on a branch");
        };
        let base_tip = checkout::tip_of(&primary, &target);
        if base_tip.is_empty() {
            return AgentPlacement::unisolated(&primary, "no commit to branch from");
        }
        let mut store = match task::load(&self.0.home, &primary) {
            Ok(store) => store,
            Err(error) => {
                return AgentPlacement::unisolated(
                    &primary,
                    &format!("task state could not be read: {error}"),
                );
            }
        };
        checkout::reconcile(&primary, &mut store, &target);
        let mut task = Task::new(None, Base::Ref(target.clone()), base_tip.clone(), target);
        match checkout::acquire(&primary, &store, &task, &base_tip, None) {
            Ok(acquired) => {
                task.checkout = Some(acquired.id.clone());
                store.upsert(task.clone());
                let _ = task::save(&self.0.home, &primary, &store);
                AgentPlacement {
                    cwd: acquired.path,
                    isolation: Isolation::Slot {
                        task: task.id,
                        checkout: acquired.id,
                        branch: acquired.branch,
                        reused: !acquired.created,
                    },
                }
            }
            Err(error) => AgentPlacement::unisolated(&primary, &error.to_string()),
        }
    }
}

/// Where an agent starts, and whether that is a slot of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPlacement {
    pub cwd: PathBuf,
    pub isolation: Isolation,
}

impl AgentPlacement {
    fn unisolated(cwd: &Path, reason: &str) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            isolation: Isolation::Unisolated {
                reason: reason.to_owned(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Isolation {
    /// The agent runs in a slot of its own, on the task's branch.
    Slot {
        task: TaskId,
        checkout: checkout::CheckoutId,
        branch: String,
        reused: bool,
    },
    /// The agent runs where it was created, and the tab must say so.
    Unisolated { reason: String },
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

#[cfg(test)]
mod placement_tests {
    use super::*;
    use uze_core::UzeHome;

    fn repository(label: &str) -> uze_testkit::git::Repository {
        uze_testkit::git::Repository::new(label)
    }

    fn application(label: &str) -> UzeApplication {
        UzeApplication::new(UzeHome::at(uze_testkit::temp::scratch(label)), Vec::new())
    }

    fn slot(placement: &AgentPlacement) -> &TaskId {
        match &placement.isolation {
            Isolation::Slot { task, .. } => task,
            Isolation::Unisolated { reason } => panic!("expected a slot, got: {reason}"),
        }
    }

    #[test]
    fn the_first_agent_is_isolated() {
        let repository = repository("place-first");
        let root = repository.root().to_path_buf();
        let app = application("place-first-home");
        let placement = app.workspace().place_new_agent(&root);
        let primary = root.canonicalize().unwrap();
        assert_ne!(
            placement.cwd, primary,
            "the primary belongs to the operator"
        );
        assert!(placement.cwd.starts_with(primary.join(".worktrees")));
        assert!(
            placement.cwd.join("README.md").is_file(),
            "the slot is populated"
        );
        let task = slot(&placement);
        assert_eq!(repository.branch_of(&placement.cwd), task.branch());
    }

    #[test]
    fn three_agents_get_three_distinct_checkouts_and_none_is_the_primary() {
        let repository = repository("place-three");
        let root = repository.root().to_path_buf();
        let app = application("place-three-home");
        let primary = root.canonicalize().unwrap();
        let placements: Vec<AgentPlacement> = (0..3)
            .map(|_| app.workspace().place_new_agent(&root))
            .collect();
        let mut cwds: Vec<&PathBuf> = placements.iter().map(|p| &p.cwd).collect();
        cwds.sort();
        cwds.dedup();
        assert_eq!(cwds.len(), 3);
        assert!(cwds.iter().all(|cwd| **cwd != primary));
        for placement in &placements {
            slot(placement);
        }
    }

    /// The property the seat rule broke: agents come and go, and the
    /// operator's tree is exactly what they left.
    #[test]
    fn the_operators_uncommitted_work_survives_agents_launching() {
        let repository = repository("place-untouched");
        let root = repository.root().to_path_buf();
        let app = application("place-untouched-home");
        std::fs::write(root.join("README.md"), "edited by the operator\n").unwrap();
        std::fs::write(root.join("scratch.txt"), "untracked\n").unwrap();

        app.workspace().place_new_agent(&root);
        app.workspace().place_new_agent(&root);

        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "edited by the operator\n"
        );
        assert!(root.join("scratch.txt").is_file());
        let status = repository.git(&["status", "--porcelain"]);
        assert_eq!(
            status.lines().count(),
            2,
            "only the operator's own two changes, no slot swept in: {status}"
        );
    }

    /// Launching an agent unisolated beats not launching it, and the tab
    /// is told.
    #[test]
    fn a_repository_without_a_commit_launches_in_place_with_the_reason() {
        let repository = uze_testkit::git::Repository::empty("place-unborn");
        let root = repository.root().to_path_buf();
        let app = application("place-unborn-home");
        let placement = app.workspace().place_new_agent(&root);
        assert_eq!(placement.cwd, root.canonicalize().unwrap());
        assert!(
            matches!(&placement.isolation, Isolation::Unisolated { reason } if reason.contains("commit")),
            "{placement:?}"
        );
    }

    #[test]
    fn a_directory_outside_any_repository_launches_in_place() {
        let outside = uze_testkit::temp::scratch("place-no-repo");
        let app = application("place-no-repo-home");
        let placement = app.workspace().place_new_agent(&outside);
        assert_eq!(placement.cwd, outside);
        assert!(matches!(placement.isolation, Isolation::Unisolated { .. }));
        std::fs::remove_dir_all(outside).unwrap();
    }

    /// A slot freed by a delivered task is taken before a new directory
    /// appears — the reuse the whole model rests on, seen from the launch.
    #[test]
    fn a_delivered_tasks_slot_is_reused_by_the_next_agent() {
        let repository = repository("place-reuse");
        let root = repository.root().to_path_buf();
        let app = application("place-reuse-home");
        let first = app.workspace().place_new_agent(&root);
        let primary = root.canonicalize().unwrap();
        let mut store = task::load(&app.home, &primary).unwrap();
        store.get_mut(slot(&first)).unwrap().state = uze_core::task::TaskState::Integrated;
        task::save(&app.home, &primary, &store).unwrap();

        let second = app.workspace().place_new_agent(&root);
        assert_eq!(second.cwd, first.cwd);
        assert!(matches!(
            second.isolation,
            Isolation::Slot { reused: true, .. }
        ));
    }
}
