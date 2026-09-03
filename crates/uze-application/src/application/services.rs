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
    Result, UzeError, checkout, hook,
    landing::{self, Delivered, DeliveryFailure, Readiness},
    project_lock, prompt_history,
    task::{self, Base, Task, TaskId, TaskState, TaskStore},
    workspace,
    worktree::{self, CompletionBehavior, WorktreePolicy},
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
    /// repository `pane_cwd` belongs to, prepared as the project's policy
    /// says, and the primary checkout is never assigned to an agent. Where
    /// isolation is impossible — no repository, no branch, no commit to
    /// branch from, Git refusing — the agent starts in place and the
    /// placement says why, so the tab can.
    pub fn place_new_agent(&self, pane_cwd: &Path) -> AgentPlacement {
        let Some(primary) = worktree::primary_checkout(pane_cwd) else {
            return AgentPlacement::unisolated(pane_cwd, "not inside a Git working tree");
        };
        let policy = match self.policy(&primary) {
            Ok(policy) => policy,
            Err(error) => return AgentPlacement::unisolated(&primary, &error.to_string()),
        };
        let Some(target) = policy
            .target
            .clone()
            .or_else(|| checkout::current_branch(&primary))
        else {
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
        match checkout::acquire(&primary, &store, &task, &base_tip, policy.slots) {
            Ok(acquired) => {
                task.checkout = Some(acquired.id.clone());
                store.upsert(task.clone());
                let _ = task::save(&self.0.home, &primary, &store);
                let warnings = checkout::materialize(
                    &primary,
                    &acquired.path,
                    &policy.link,
                    policy.setup.as_deref(),
                );
                AgentPlacement {
                    cwd: acquired.path,
                    isolation: Isolation::Slot {
                        task: task.id,
                        checkout: acquired.id,
                        branch: acquired.branch,
                        reused: !acquired.created,
                    },
                    warnings,
                }
            }
            Err(error) => AgentPlacement::unisolated(&primary, &error.to_string()),
        }
    }

    /// The project's declared policy, or the defaults when the lock declares
    /// none. A malformed lock is an error rather than a silent default.
    fn policy(&self, primary: &Path) -> Result<WorktreePolicy> {
        Ok(project_lock::load_lock(primary)?
            .and_then(|lock| lock.worktrees)
            .unwrap_or_default())
    }

    /// The repository `cwd` belongs to, with its policy and recorded tasks.
    fn repository(&self, cwd: &Path) -> Option<Repository> {
        let primary = worktree::primary_checkout(cwd)?;
        let policy = self.policy(&primary).ok()?;
        let store = task::load(&self.0.home, &primary).ok()?;
        Some(Repository {
            primary,
            policy,
            store,
        })
    }

    /// The primary checkout `cwd` belongs to — the key every task view
    /// hangs off — or `None` outside a Git working tree.
    pub fn primary_of(&self, cwd: &Path) -> Option<PathBuf> {
        worktree::primary_checkout(cwd)
    }

    /// Every task recorded for `cwd`'s repository, as last evaluated.
    pub fn tasks(&self, cwd: &Path) -> Vec<TaskView> {
        self.repository(cwd)
            .map(|repository| repository.views())
            .unwrap_or_default()
    }

    /// The project's say in delivery, for a header to name what `deliver`
    /// will do.
    pub fn delivery_policy(&self, cwd: &Path) -> Option<DeliveryPolicyView> {
        let repository = self.repository(cwd)?;
        Some(DeliveryPolicyView {
            completion: repository.policy.completion.abi_name(),
            target: repository
                .policy
                .target
                .clone()
                .or_else(|| checkout::current_branch(&repository.primary)),
            gate: repository.policy.gate.clone(),
        })
    }

    /// Re-reads every live task's state from its checkout — what the
    /// sidebar shows after an agent's pane goes quiet — and lets a clean,
    /// live task follow a target that moved. A conflict that produces
    /// returns to the owning agent as a notice for its pane.
    pub fn evaluate_tasks(&self, cwd: &Path) -> Evaluation {
        let Some(mut repository) = self.repository(cwd) else {
            return Evaluation::default();
        };
        let target = repository.target();
        checkout::reconcile(&repository.primary, &mut repository.store, &target);
        let mut notices = Vec::new();
        let primary = repository.primary.clone();
        let completion = repository.policy.completion;
        for task in &mut repository.store.tasks {
            if !checkout::is_live(&task.state) || task.state == TaskState::Integrating {
                continue;
            }
            match landing::readiness(&primary, task) {
                Readiness::Running => task.state = TaskState::Running,
                Readiness::Uncommitted => task.state = TaskState::Uncommitted,
                Readiness::Rebasing { files } => task.state = TaskState::Conflicted { files },
                Readiness::Ready { base, .. } => {
                    task.base_commit = base;
                    if task.state != TaskState::GateFailed {
                        task.state = TaskState::Ready;
                    }
                }
            }
            // Following a moved target costs a clean task nothing and a
            // dirty one its work in progress, which `refresh` refuses. A
            // published target is followed at delivery, where the fetch
            // is already paid for.
            if completion != CompletionBehavior::Pr
                && matches!(task.state, TaskState::Running | TaskState::Ready)
                && let Err(DeliveryFailure::Conflict {
                    files,
                    target_moved,
                }) = landing::refresh(&primary, task, completion)
                && let Some(slot) = landing::slot_path(&primary, task)
            {
                notices.push(AgentNotice {
                    task: task.id.as_str().to_owned(),
                    checkout: slot,
                    message: landing::conflict_message(task, &files, target_moved),
                });
            }
        }
        let _ = task::save(&self.0.home, &primary, &repository.store);
        Evaluation {
            tasks: repository.views(),
            notices,
        }
    }

    /// Delivers one task the way the project's completion says, one task
    /// at a time under the repository write lock.
    pub fn deliver_task(&self, cwd: &Path, task_id: &str) -> Option<DeliveryReport> {
        let mut repository = self.repository(cwd)?;
        let report = repository.deliver(task_id)?;
        let _ = task::save(&self.0.home, &repository.primary, &repository.store);
        Some(report)
    }

    /// Delivers every ready task, oldest first; the second sees the first.
    pub fn deliver_ready(&self, cwd: &Path) -> Vec<DeliveryReport> {
        let Some(mut repository) = self.repository(cwd) else {
            return Vec::new();
        };
        let mut ready: Vec<(u64, String)> = repository
            .store
            .tasks
            .iter()
            .filter(|task| task.state == TaskState::Ready)
            .map(|task| (task.created_at_unix, task.id.as_str().to_owned()))
            .collect();
        ready.sort();
        let reports = ready
            .into_iter()
            .filter_map(|(_, id)| repository.deliver(&id))
            .collect();
        let _ = task::save(&self.0.home, &repository.primary, &repository.store);
        reports
    }

    /// Ends every task no pane is in front of any more, and says what
    /// became of each slot.
    ///
    /// `occupied` names the checkout directories a live pane still sits in.
    /// A task outside that set has no agent: its slot goes back to the pool
    /// when it holds nothing, and is parked for the operator when it holds
    /// work. Delivery is not the only way a task ends — most end by the
    /// operator closing the tab — and a slot nobody ever released is a slot
    /// no new agent can reuse.
    pub fn release_abandoned_tasks(&self, cwd: &Path, occupied: &[PathBuf]) -> Vec<ReleasedTask> {
        let Some(mut repository) = self.repository(cwd) else {
            return Vec::new();
        };
        let target = repository.target();
        let primary = repository.primary.clone();
        let mut released = Vec::new();
        for task in &mut repository.store.tasks {
            // A delivery in flight owns the task until it answers.
            if !checkout::is_live(&task.state) || task.state == TaskState::Integrating {
                continue;
            }
            if landing::slot_path(&primary, task)
                .is_some_and(|slot| occupied.iter().any(|pane| pane.starts_with(&slot)))
            {
                continue;
            }
            let slot = checkout::release(&primary, task, &target);
            released.push(ReleasedTask {
                id: task.id.as_str().to_owned(),
                label: task.label.clone(),
                parked: slot == checkout::SlotState::Parked,
            });
        }
        if !released.is_empty() {
            let _ = task::save(&self.0.home, &primary, &repository.store);
        }
        released
    }

    /// Takes out the safe removals: an `agent/` branch whose every commit
    /// is already in the target, and the directory of a clean slot nobody
    /// has touched in a fortnight — its branch kept. Nothing holding work
    /// is ever touched here; that is the operator's alone.
    pub fn collect_slot_garbage(&self, cwd: &Path) -> Vec<String> {
        let Some(repository) = self.repository(cwd) else {
            return Vec::new();
        };
        let target = repository.target();
        let collected = checkout::collect(
            &repository.primary,
            &repository.store,
            &target,
            checkout::IDLE_SLOT_AGE,
        );
        collected
            .branches
            .into_iter()
            .chain(collected.slots.into_iter().map(|slot| slot.to_string()))
            .collect()
    }

    /// The operator declares a handed-off task done: its slot is free and
    /// its branch stays.
    pub fn finish_task(&self, cwd: &Path, task_id: &str) -> Result<()> {
        let mut repository = self
            .repository(cwd)
            .ok_or_else(|| UzeError::UnknownTask(task_id.to_owned()))?;
        let task = repository
            .task_mut(task_id)
            .ok_or_else(|| UzeError::UnknownTask(task_id.to_owned()))?;
        task.state = TaskState::Integrated;
        task::save(&self.0.home, &repository.primary, &repository.store)
    }

    /// The one path that deletes work, taken only by the operator on a
    /// named task: the checkout and the branch go, the record goes with them.
    pub fn discard_task(&self, cwd: &Path, task_id: &str) -> Result<()> {
        let mut repository = self
            .repository(cwd)
            .ok_or_else(|| UzeError::UnknownTask(task_id.to_owned()))?;
        let task = repository
            .task_mut(task_id)
            .ok_or_else(|| UzeError::UnknownTask(task_id.to_owned()))?
            .clone();
        checkout::discard(&repository.primary, &task).map_err(UzeError::Discard)?;
        repository
            .store
            .tasks
            .retain(|recorded| recorded.id.as_str() != task_id);
        task::save(&self.0.home, &repository.primary, &repository.store)
    }
}

/// A repository as the task operations see it: its primary checkout, the
/// project's policy, and the recorded tasks.
struct Repository {
    primary: PathBuf,
    policy: WorktreePolicy,
    store: TaskStore,
}

impl Repository {
    fn target(&self) -> String {
        self.policy
            .target
            .clone()
            .or_else(|| checkout::current_branch(&self.primary))
            .unwrap_or_else(|| "HEAD".to_owned())
    }

    fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.store
            .tasks
            .iter_mut()
            .find(|task| task.id.as_str() == id)
    }

    fn views(&self) -> Vec<TaskView> {
        self.store
            .tasks
            .iter()
            .map(|task| TaskView::from_task(&self.primary, task))
            .collect()
    }

    fn deliver(&mut self, task_id: &str) -> Option<DeliveryReport> {
        let completion = self.policy.completion;
        let gate = self.policy.gate.clone();
        let policy = landing::Policy {
            completion,
            gate: gate.as_deref(),
        };
        let primary = self.primary.clone();
        let task = self.task_mut(task_id)?;
        let outcome = match landing::deliver(&primary, task, &policy) {
            Ok(Delivered::Handoff) => DeliveryOutcome::Handoff,
            Ok(Delivered::Merged { .. }) => DeliveryOutcome::Merged,
            Ok(Delivered::Published { branch, request }) => {
                DeliveryOutcome::Published { branch, request }
            }
            Err(DeliveryFailure::Conflict {
                files,
                target_moved,
            }) => DeliveryOutcome::ReturnedToAgent(AgentNotice {
                task: task.id.as_str().to_owned(),
                checkout: landing::slot_path(&primary, task).unwrap_or_default(),
                message: landing::conflict_message(task, &files, target_moved),
            }),
            Err(DeliveryFailure::GateFailed { output }) => {
                DeliveryOutcome::ReturnedToAgent(AgentNotice {
                    task: task.id.as_str().to_owned(),
                    checkout: landing::slot_path(&primary, task).unwrap_or_default(),
                    message: landing::gate_failure_message(task, &output),
                })
            }
            Err(other) => DeliveryOutcome::Refused(other.to_string()),
        };
        Some(DeliveryReport {
            task: TaskView::from_task(&primary, task),
            outcome,
        })
    }
}

/// A task ended because its agent is gone, and what became of its slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasedTask {
    pub id: String,
    pub label: String,
    /// `true` when the checkout held work and was parked for the operator
    /// instead of going back to the pool.
    pub parked: bool,
}

/// One task as presentation sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskView {
    pub id: String,
    pub label: String,
    pub branch: String,
    pub target: String,
    /// The slot's directory, when the task has one.
    pub checkout: Option<PathBuf>,
    pub state: TaskStateView,
    /// Commits the branch has beyond its base — what a delivery would land.
    pub ahead: usize,
    pub published_as: Option<String>,
    pub created_at_unix: u64,
}

impl TaskView {
    fn from_task(primary: &Path, task: &Task) -> Self {
        Self {
            id: task.id.as_str().to_owned(),
            label: task.label.clone(),
            branch: task.branch.clone(),
            target: task.target.clone(),
            checkout: landing::slot_path(primary, task),
            state: TaskStateView::from(&task.state),
            ahead: checkout::commits_ahead(primary, &task.base_commit, &task.branch),
            published_as: task.published_as.clone(),
            created_at_unix: task.created_at_unix,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskStateView {
    Running,
    Uncommitted,
    Ready,
    Integrating,
    Conflicted { files: Vec<PathBuf> },
    GateFailed,
    Integrated,
    Parked,
}

impl TaskStateView {
    /// Whether delivery may be offered for a task in this state.
    pub fn is_deliverable(&self) -> bool {
        matches!(self, Self::Ready | Self::GateFailed)
    }

    /// Whether the task still has an agent's work in front of it.
    pub fn is_live(&self) -> bool {
        !matches!(self, Self::Integrated | Self::Parked)
    }
}

impl From<&TaskState> for TaskStateView {
    fn from(state: &TaskState) -> Self {
        match state {
            TaskState::Running => Self::Running,
            TaskState::Uncommitted => Self::Uncommitted,
            TaskState::Ready => Self::Ready,
            TaskState::Integrating => Self::Integrating,
            TaskState::Conflicted { files } => Self::Conflicted {
                files: files.clone(),
            },
            TaskState::GateFailed => Self::GateFailed,
            TaskState::Integrated => Self::Integrated,
            TaskState::Parked => Self::Parked,
        }
    }
}

/// A message for the pane of the agent that owns `task`, running in
/// `checkout`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentNotice {
    pub task: String,
    pub checkout: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Evaluation {
    pub tasks: Vec<TaskView>,
    pub notices: Vec<AgentNotice>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryOutcome {
    Handoff,
    Merged,
    Published {
        branch: String,
        request: Option<String>,
    },
    /// Nothing was written; the reason names why.
    Refused(String),
    /// The target is untouched and the owning agent has been told what to do.
    ReturnedToAgent(AgentNotice),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryReport {
    pub task: TaskView,
    pub outcome: DeliveryOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryPolicyView {
    pub completion: &'static str,
    pub target: Option<String>,
    pub gate: Option<String>,
}

/// Where an agent starts, and whether that is a slot of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentPlacement {
    pub cwd: PathBuf,
    pub isolation: Isolation,
    /// What preparing the checkout could not do — a missing link target, a
    /// failed setup — none of which stops the launch.
    pub warnings: Vec<String>,
}

impl AgentPlacement {
    fn unisolated(cwd: &Path, reason: &str) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            isolation: Isolation::Unisolated {
                reason: reason.to_owned(),
            },
            warnings: Vec::new(),
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

    /// The reuse the slot model exists for only ever happens if closing an
    /// agent ends its task: nothing else releases a checkout.
    #[test]
    fn an_agent_whose_pane_is_gone_frees_its_slot_for_the_next_one() {
        let repository = repository("release-free");
        let root = repository.root().to_path_buf();
        let app = application("release-free-home");

        let first = app.workspace().place_new_agent(&root);
        let abandoned_branch = repository.branch_of(&first.cwd);
        let released = app.workspace().release_abandoned_tasks(&root, &[]);
        assert_eq!(released.len(), 1);
        assert!(!released[0].parked, "an empty checkout holds nothing");

        let second = app.workspace().place_new_agent(&root);
        assert_eq!(
            second.cwd, first.cwd,
            "the freed slot is reused instead of a new directory"
        );

        // The branch it left behind carries nothing the target lacks, so
        // the safe collection takes it once the slot has moved off it.
        app.workspace().collect_slot_garbage(&root);
        assert!(
            !repository
                .git(&["branch", "--list", &abandoned_branch])
                .contains(&abandoned_branch),
            "a branch with nothing on it does not outlive its task"
        );

        // While a pane still sits in it, the slot stays that task's.
        let third_panes = [second.cwd.join("src")];
        assert!(
            app.workspace()
                .release_abandoned_tasks(&root, &third_panes)
                .is_empty(),
            "an agent in front of its checkout is not abandoned"
        );
    }

    #[test]
    fn an_agent_that_left_work_behind_parks_its_slot() {
        let repository = repository("release-park");
        let root = repository.root().to_path_buf();
        let app = application("release-park-home");

        let abandoned = app.workspace().place_new_agent(&root);
        std::fs::write(abandoned.cwd.join("draft.rs"), b"unsaved").unwrap();
        let released = app.workspace().release_abandoned_tasks(&root, &[]);
        assert_eq!(released.len(), 1);
        assert!(released[0].parked);

        let next = app.workspace().place_new_agent(&root);
        assert_ne!(
            next.cwd, abandoned.cwd,
            "a parked checkout is never offered to a new agent"
        );
        assert!(
            abandoned.cwd.join("draft.rs").is_file(),
            "the work it holds is preserved"
        );
    }

    /// A slot carries the project's own anchor files, so resolving a space
    /// from inside one used to answer the slot: a second space over one
    /// repository, rooted in `.worktrees`.
    #[test]
    fn a_slot_belongs_to_its_repositorys_space_and_is_never_a_root_of_its_own() {
        let repository = repository("space-root");
        let root = repository.root().to_path_buf();
        let app = application("space-root-home");
        let placement = app.workspace().place_new_agent(&root);
        assert_eq!(
            crate::space_root(&placement.cwd),
            crate::space_root(&root),
            "an agent's checkout lands in the space its repository already has"
        );
        assert_eq!(
            crate::space_root(&placement.cwd.join("crates")),
            crate::space_root(&root),
            "so does a subdirectory of it"
        );
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

#[cfg(test)]
mod task_service_tests {
    use super::*;
    use uze_core::UzeHome;

    fn repository(label: &str) -> uze_testkit::git::Repository {
        let repository = uze_testkit::git::Repository::new(label);
        repository.commit_file(".gitignore", ".env\ntarget/\n");
        repository
    }

    fn application(label: &str) -> UzeApplication {
        UzeApplication::new(UzeHome::at(uze_testkit::temp::scratch(label)), Vec::new())
    }

    fn lock(repository: &uze_testkit::git::Repository, policy: &str) {
        std::fs::write(
            repository.root().join("agents.lock"),
            format!("version: 1\nworktrees:\n{policy}"),
        )
        .unwrap();
    }

    fn launched(app: &UzeApplication, root: &Path) -> (String, PathBuf) {
        let placement = app.workspace().place_new_agent(root);
        match placement.isolation {
            Isolation::Slot { task, .. } => (task.as_str().to_owned(), placement.cwd),
            Isolation::Unisolated { reason } => panic!("{reason}"),
        }
    }

    fn agent_commits(
        repository: &uze_testkit::git::Repository,
        slot: &Path,
        file: &str,
        contents: &str,
    ) {
        std::fs::write(slot.join(file), contents).unwrap();
        repository.git_in(slot, &["add", "--", file]);
        repository.git_in(slot, &["commit", "-qm", file]);
    }

    fn state_of(app: &UzeApplication, root: &Path, id: &str) -> TaskStateView {
        app.workspace()
            .tasks(root)
            .into_iter()
            .find(|task| task.id == id)
            .map(|task| task.state)
            .expect("the task is recorded")
    }

    #[test]
    fn evaluation_reads_the_checkout_and_merge_delivers() {
        let repository = repository("svc-merge");
        lock(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-merge-home");
        let (id, slot) = launched(&app, &root);
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Running);

        std::fs::write(slot.join("draft.rs"), "").unwrap();
        assert_eq!(
            app.workspace().evaluate_tasks(&root).tasks[0].state,
            TaskStateView::Uncommitted
        );
        agent_commits(&repository, &slot, "draft.rs", "fn done() {}");
        let evaluation = app.workspace().evaluate_tasks(&root);
        assert_eq!(evaluation.tasks[0].state, TaskStateView::Ready);
        assert!(evaluation.notices.is_empty());

        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert_eq!(report.outcome, DeliveryOutcome::Merged);
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Integrated);
        assert!(root.join("draft.rs").is_file());
    }

    #[test]
    fn a_conflict_returns_a_notice_addressed_to_the_slot() {
        let repository = repository("svc-conflict");
        lock(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-conflict-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "shared.rs", "agent\n");
        repository.commit_file("shared.rs", "operator\n");

        // The clean task follows the target on evaluation, and the
        // conflict that produces is already the agent's to resolve; a
        // delivery asked for meanwhile is refused, never forced.
        let evaluation = app.workspace().evaluate_tasks(&root);
        assert_eq!(evaluation.notices.len(), 1, "{evaluation:?}");
        let notice = &evaluation.notices[0];
        assert_eq!(notice.checkout, slot);
        assert!(notice.message.contains("shared.rs"));
        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert!(
            matches!(report.outcome, DeliveryOutcome::Refused(_)),
            "{:?}",
            report.outcome
        );
        assert!(matches!(
            state_of(&app, &root, &id),
            TaskStateView::Conflicted { .. }
        ));
    }

    /// A clean live task follows the target on evaluation; a conflict there
    /// is also a notice.
    #[test]
    fn evaluation_lets_a_clean_task_follow_the_target() {
        let repository = repository("svc-follow");
        lock(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-follow-home");
        let (_, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "mine.rs", "agent's mine\n");
        repository.commit_file("theirs.rs", "");
        let evaluation = app.workspace().evaluate_tasks(&root);
        assert!(evaluation.notices.is_empty());
        assert!(
            slot.join("theirs.rs").is_file(),
            "rebased onto the moved target"
        );

        repository.commit_file("mine.rs", "operator's mine\n");
        let evaluation = app.workspace().evaluate_tasks(&root);
        assert_eq!(evaluation.notices.len(), 1);
        assert_eq!(evaluation.notices[0].checkout, slot);
    }

    #[test]
    fn the_locks_gate_refuses_and_a_passing_gate_lets_it_through() {
        let repository = repository("svc-gate");
        lock(
            &repository,
            "  completion: merge\n  gate: test -f must-exist\n",
        );
        let root = repository.root().to_path_buf();
        let app = application("svc-gate-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "a.rs", "");
        app.workspace().evaluate_tasks(&root);

        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert!(
            matches!(report.outcome, DeliveryOutcome::ReturnedToAgent(_)),
            "{:?}",
            report.outcome
        );
        assert_eq!(state_of(&app, &root, &id), TaskStateView::GateFailed);

        agent_commits(&repository, &slot, "must-exist", "");
        app.workspace().evaluate_tasks(&root);
        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert_eq!(report.outcome, DeliveryOutcome::Merged);
    }

    #[test]
    fn deliver_ready_takes_them_in_order_and_the_second_sees_the_first() {
        let repository = repository("svc-ready");
        lock(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-ready-home");
        let (_, first) = launched(&app, &root);
        let (_, second) = launched(&app, &root);
        agent_commits(&repository, &first, "first.rs", "");
        agent_commits(&repository, &second, "second.rs", "");
        app.workspace().evaluate_tasks(&root);

        let reports = app.workspace().deliver_ready(&root);
        assert_eq!(reports.len(), 2);
        assert!(
            reports
                .iter()
                .all(|report| report.outcome == DeliveryOutcome::Merged),
            "{reports:?}"
        );
        assert!(root.join("first.rs").is_file() && root.join("second.rs").is_file());
    }

    #[test]
    fn handoff_is_finished_by_the_operator_and_discard_is_the_only_deletion() {
        let repository = repository("svc-finish-discard");
        let root = repository.root().to_path_buf();
        let app = application("svc-finish-discard-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "a.rs", "");
        app.workspace().evaluate_tasks(&root);
        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert_eq!(report.outcome, DeliveryOutcome::Handoff);
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Ready);
        let branch = report.task.branch.clone();

        app.workspace().finish_task(&root, &id).unwrap();
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Integrated);
        assert!(
            slot.is_dir()
                && repository
                    .git(&["branch", "--list", &branch])
                    .contains(&branch)
        );

        app.workspace().discard_task(&root, &id).unwrap();
        assert!(!slot.exists());
        assert!(repository.git(&["branch", "--list", &branch]).is_empty());
        assert!(
            app.workspace()
                .tasks(&root)
                .iter()
                .all(|task| task.id != id)
        );
    }

    #[test]
    fn the_locks_target_cap_links_and_setup_shape_the_launch() {
        let repository = repository("svc-lock-launch");
        repository.git(&["branch", "develop"]);
        std::fs::write(repository.root().join(".env"), "KEY=1\n").unwrap();
        lock(
            &repository,
            "  target: develop\n  slots: 1\n  link: [.env]\n  setup: touch prepared\n",
        );
        let root = repository.root().to_path_buf();
        let app = application("svc-lock-launch-home");

        let placement = app.workspace().place_new_agent(&root);
        let Isolation::Slot { branch, .. } = &placement.isolation else {
            panic!("{placement:?}");
        };
        assert!(placement.warnings.is_empty(), "{:?}", placement.warnings);
        assert!(
            placement.cwd.join("prepared").is_file(),
            "setup ran in the slot"
        );
        assert!(
            std::fs::symlink_metadata(placement.cwd.join(".env"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            app.workspace().tasks(&root)[0].target,
            "develop",
            "the declared target, not the primary's branch"
        );
        let _ = branch;

        let second = app.workspace().place_new_agent(&root);
        assert!(
            matches!(&second.isolation, Isolation::Unisolated { reason } if reason.contains("1 declared")),
            "{second:?}"
        );
    }
}
