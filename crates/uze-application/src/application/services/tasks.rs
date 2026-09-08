//! The workspace service: slots, tasks, delivery, and the read models
//! presentation sees them through.
//!
//! Split out of `services.rs`, which had grown to carry eight capability
//! views plus every read model the largest of them answers with. This is
//! that largest one — the only service with a domain of its own rather
//! than a thin route into `uze-core`, which is why it is the one that
//! became a file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use uze_core::{
    Result, UzeError, checkout, client_layout, conversation,
    landing::{self, Delivered, DeliveryFailure, Readiness},
    manifest, prompt_history,
    task::{self, Base, Task, TaskId, TaskState, TaskStore},
    workspace,
    worktree::{self, BranchVocabulary, CompletionBehavior, NameRefusal, WorktreePolicy},
};

use super::{AgentIdentity, Workspace};
#[cfg(test)]
use crate::UzeApplication;

impl Workspace<'_> {
    /// The workspace root a directory belongs to, or the directory itself.
    ///
    /// One repository is one terminal server, and this is the answer both
    /// the server key and the prompt history are keyed on — resolved once,
    /// here, rather than twice at two call sites.
    #[tracing::instrument(name = "workspace.root", skip_all, fields(cwd = %cwd.display()))]
    pub fn root(&self, cwd: &Path) -> PathBuf {
        workspace::workspace_root_or_self(cwd)
    }

    /// The isolated checkout a path sits in, or `None` when it is not
    /// isolated. Lexical against the fixed layout: a display asks this of
    /// every open tab on every frame.
    #[tracing::instrument(name = "workspace.isolated_checkout", skip_all)]
    pub fn isolated_checkout<'a>(&self, path: &'a Path) -> Option<worktree::IsolatedCheckout<'a>> {
        worktree::isolated_checkout(path)
    }

    /// The harnesses this installation can recognize, as descriptors.
    #[tracing::instrument(name = "workspace.agent_identities", skip_all)]
    pub fn agent_identities(&self) -> Vec<AgentIdentity> {
        self.0
            .integrations
            .iter()
            .map(|integration| {
                let binary = integration
                    .aliases()
                    .first()
                    .copied()
                    .unwrap_or(integration.id());
                let (launch, continuity_gap) = self.launcher(integration.as_ref(), binary);
                AgentIdentity {
                    binary,
                    integration: integration.id(),
                    display_name: integration.display_name(),
                    launch,
                    continuity_gap,
                }
            })
            .collect()
    }

    /// What to launch an agent of `integration` by, and what that costs.
    ///
    /// UZE's own launcher is what decides, per launch, whether an agent
    /// resumes its task's conversation or starts one, so naming it here by
    /// path is what makes continuity independent of the operator's `PATH`.
    /// It is never created on their behalf: the launcher's presence is the
    /// operator's own opt-in, and resurrecting one they removed would
    /// override a decision they made. Without it the harness still starts —
    /// on its plain name, with no conversation carried over, and the reason
    /// said rather than silently missing.
    fn launcher(
        &self,
        integration: &dyn uze_core::integration::IntegrationPort,
        binary: &str,
    ) -> (PathBuf, Option<String>) {
        let bare = PathBuf::from(binary);
        if integration.session_continuity() == uze_core::integration::SessionContinuity::Unsupported
        {
            return (
                bare,
                Some("this harness offers no way to continue a conversation".to_owned()),
            );
        }
        let launcher = self.0.home.shims_dir().join(integration.shim_name());
        if launcher.exists() {
            return (launcher, None);
        }
        (
            bare,
            Some(
                "UZE's launcher is not installed for this harness, so its conversation is not \
                 carried over"
                    .to_owned(),
            ),
        )
    }

    /// Writes back which conversation the agent in `cwd` is actually in.
    ///
    /// Answers whether anything changed. Runs off whatever thread the
    /// caller gives it — one of these asks a harness about its own
    /// records, which can mean spawning it — and is silent about every
    /// way of having nothing to say.
    #[tracing::instrument(name = "workspace.refresh_conversation", skip_all, fields(integration = %integration, cwd = %cwd.display()))]
    pub fn refresh_conversation(&self, integration: &str, cwd: &Path) -> bool {
        self.0
            .integrations
            .iter()
            .find(|candidate| candidate.id() == integration)
            .is_some_and(|integration| {
                uze_core::continuity::refresh(&self.0.home, cwd, integration.as_ref())
            })
    }

    /// Recent prompts submitted into the agent tabs of `root`'s workspace.
    #[tracing::instrument(name = "workspace.prompt_history", skip_all, fields(root = %root.display(), limit))]
    pub fn prompt_history(&self, root: &Path, limit: usize) -> Vec<prompt_history::PromptEntry> {
        prompt_history::list_for_workspace(&self.0.home, root, limit)
    }

    /// Records one prompt submitted into an agent tab of `root`'s
    /// workspace. Best-effort by construction: an empty prompt is ignored
    /// rather than refused.
    #[tracing::instrument(name = "workspace.record_prompt", skip_all, fields(root = %root.display(), prompt = %prompt), err)]
    pub fn record_prompt(
        &self,
        root: &Path,
        origin: &prompt_history::PromptOrigin,
        prompt: &str,
    ) -> Result<()> {
        prompt_history::record(&self.0.home, root, origin, prompt)
    }

    /// Forgets every prompt recorded for `root`'s workspace.
    #[tracing::instrument(name = "workspace.clear_prompt_history", skip_all, fields(root = %root.display()), err)]
    pub fn clear_prompt_history(&self, root: &Path) -> Result<()> {
        prompt_history::clear(&self.0.home, root)
    }

    /// What the TUI was last left looking like, in both of its modes.
    /// Best-effort: unreadable state answers with the defaults rather
    /// than failing.
    #[tracing::instrument(name = "workspace.client_layout", skip_all)]
    pub fn client_layout(&self) -> client_layout::ClientLayout {
        client_layout::load(&self.0.home)
    }

    /// Remembers the TUI's shape for the next run.
    #[tracing::instrument(name = "workspace.save_client_layout", skip_all, err)]
    pub fn save_client_layout(&self, layout: &client_layout::ClientLayout) -> Result<()> {
        client_layout::save(&self.0.home, layout)
    }

    /// Where a newly created agent starts, decided before its harness does.
    ///
    /// Every agent isolates: a slot is acquired for a new task in the
    /// repository `pane_cwd` belongs to, prepared as the project's policy
    /// says, and the primary checkout is never assigned to an agent.
    /// `occupied` names the checkout directories a live pane still sits
    /// in; none of them is reused, whatever its task record says — a
    /// delivered task's agent is still there until its tab closes. Where
    /// isolation is impossible — no repository, no branch, no commit to
    /// branch from, Git refusing — the agent starts in place and the
    /// placement says why, so the tab can.
    #[tracing::instrument(name = "workspace.place_new_agent", skip_all, fields(pane_cwd = %pane_cwd.display()))]
    pub fn place_new_agent(&self, pane_cwd: &Path, occupied: &[PathBuf]) -> AgentPlacement {
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
        // Before anything is branched from it: an agent placed on a target
        // nobody fetched starts behind every merge of the day, and hears
        // about it as conflicts in a request already opened.
        let sync = landing::sync_target(&primary, &target);
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
        match checkout::acquire(&primary, &store, &task, &base_tip, policy.slots, occupied) {
            Ok(acquired) => {
                task.checkout = Some(acquired.id.clone());
                store.upsert(task.clone());
                let _ = task::save(&self.0.home, &primary, &store);
                let mut warnings = sync.concern(&task.target).into_iter().collect::<Vec<_>>();
                warnings.extend(checkout::materialize(
                    &primary,
                    &acquired.path,
                    &policy.link,
                    &policy.setup,
                ));
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

    /// Puts a checkout back under a task that lost its own — removed
    /// outside UZE, or swept as idle — so a new agent can continue from
    /// where its branch stands. The task is live again in the slot this
    /// acquires; `occupied` is what [`Self::place_new_agent`] takes. A
    /// task that still has its checkout is answered with that checkout.
    #[tracing::instrument(name = "workspace.resume_task", skip_all, fields(cwd = %cwd.display(), task_id = %task_id), err)]
    pub fn resume_task(
        &self,
        cwd: &Path,
        task_id: &str,
        occupied: &[PathBuf],
    ) -> Result<AgentPlacement> {
        let mut repository = self
            .repository(cwd)
            .ok_or_else(|| UzeError::UnknownTask(task_id.to_owned()))?;
        let target = repository.target();
        let primary = repository.primary.clone();
        let policy = repository.policy.clone();
        checkout::reconcile(&primary, &mut repository.store, &target);
        let store = repository.store.clone();
        let task = repository
            .task_mut(task_id)
            .ok_or_else(|| UzeError::UnknownTask(task_id.to_owned()))?;
        if let (Some(existing), Some(checkout)) =
            (landing::slot_path(&primary, task), task.checkout.clone())
        {
            return Ok(AgentPlacement {
                cwd: existing,
                isolation: Isolation::Slot {
                    task: task.id.clone(),
                    checkout,
                    branch: task.branch.clone(),
                    reused: true,
                },
                warnings: Vec::new(),
            });
        }
        let acquired = checkout::resume(&primary, &store, task, policy.slots, occupied)
            .map_err(|error| UzeError::ResumeFailed(error.to_string()))?;
        task.checkout = Some(acquired.id.clone());
        task.state = TaskState::Running;
        let isolation = Isolation::Slot {
            task: task.id.clone(),
            checkout: acquired.id,
            branch: acquired.branch,
            reused: !acquired.created,
        };
        task::save(&self.0.home, &primary, &repository.store)?;
        let warnings = checkout::materialize(&primary, &acquired.path, &policy.link, &policy.setup);
        Ok(AgentPlacement {
            cwd: acquired.path,
            isolation,
            warnings,
        })
    }

    /// Names the work the agent in `cwd` is doing.
    ///
    /// The task is the one owning the checkout `cwd` sits in — resolved
    /// from the directory alone, with no identifier to pass, because an
    /// identifier is exactly what would let one agent rename another's
    /// branch. A slot that served earlier tasks answers for the one that
    /// owns it now, by the rule `checkout::reconcile` already uses.
    ///
    /// First-writer-wins: a task that already carries a chosen name is
    /// refused, so nothing an agent or an operator decided is ever
    /// replaced by a later mechanism.
    #[tracing::instrument(name = "workspace.name_task", skip_all, fields(cwd = %cwd.display(), proposed = %proposed), err)]
    pub fn name_task(&self, cwd: &Path, proposed: &str) -> Result<NamedTask> {
        let mut repository = self
            .repository(cwd)
            .ok_or_else(|| UzeError::TaskNaming("not inside a Git working tree".to_owned()))?;
        let primary = repository.primary.clone();
        let vocabulary = repository.policy.branch.clone();
        let branch = vocabulary
            .accept(proposed)
            .map_err(|refusal| UzeError::TaskNaming(refusal_words(&refusal, &vocabulary)))?;
        let checkout_id = worktree::isolated_checkout(cwd)
            .map(|isolated| checkout::CheckoutId::adopted(isolated.name))
            .ok_or_else(|| {
                UzeError::TaskNaming(
                    "this is not an agent's checkout — the primary belongs to the operator"
                        .to_owned(),
                )
            })?;
        let target = repository.target();
        checkout::reconcile(&primary, &mut repository.store, &target);
        let task = repository
            .store
            .tasks
            .iter_mut()
            .filter(|task| task.checkout.as_ref() == Some(&checkout_id))
            .max_by_key(|task| task.created_at_unix)
            .ok_or_else(|| {
                UzeError::TaskNaming("no task is recorded for this checkout".to_owned())
            })?;
        if task.is_named() {
            return Err(UzeError::TaskNaming(format!(
                "this work is already named `{}`; a name nobody generated is never replaced",
                task.branch
            )));
        }
        if checkout::current_branch(cwd).as_deref() != Some(task.branch.as_str()) {
            return Err(UzeError::TaskNaming(
                "this checkout is not on the task's branch — finish the rebase first".to_owned(),
            ));
        }
        if checkout::branch_exists(&primary, &branch) {
            return Err(UzeError::TaskNaming(format!(
                "`{branch}` already exists in this repository"
            )));
        }
        checkout::rename_branch(&primary, &task.branch.clone(), &branch)?;
        task.take_name(branch.clone());
        let named = NamedTask {
            task: task.id.as_str().to_owned(),
            branch,
            label: task.label.clone(),
        };
        task::save(&self.0.home, &primary, &repository.store)?;
        Ok(named)
    }

    /// The project's declared policy, or the defaults when its manifest
    /// declares none. Read from the primary checkout on purpose: a worktree
    /// never declares a policy of its own, and nothing machine-scoped
    /// participates, so the same repository resolves identically everywhere.
    /// A malformed manifest is an error rather than a silent default.
    fn policy(&self, primary: &Path) -> Result<WorktreePolicy> {
        manifest::worktree_policy(primary)
    }

    /// The repository `cwd` belongs to, with its policy and recorded tasks.
    fn repository(&self, cwd: &Path) -> Option<Repository> {
        self.open(cwd).ok().flatten()
    }

    /// The same, telling "there is no repository here" apart from "there
    /// is one and its recorded tasks could not be read" — the second is
    /// the condition every caller used to render as the first.
    fn open(&self, cwd: &Path) -> std::result::Result<Option<Repository>, String> {
        let Some(primary) = worktree::primary_checkout(cwd) else {
            return Ok(None);
        };
        let Ok(policy) = self.policy(&primary) else {
            return Ok(None);
        };
        let store = task::load(&self.0.home, &primary).map_err(|error| error.to_string())?;
        Ok(Some(Repository {
            primary,
            policy,
            store,
        }))
    }

    /// The primary checkout `cwd` belongs to — the key every task view
    /// hangs off — or `None` outside a Git working tree.
    #[tracing::instrument(name = "workspace.primary_of", skip_all, fields(cwd = %cwd.display()))]
    pub fn primary_of(&self, cwd: &Path) -> Option<PathBuf> {
        worktree::primary_checkout(cwd)
    }

    /// The branch checked out where `cwd` sits — what an agent working
    /// outside any slot is on — or `None` for a detached `HEAD` or no
    /// repository at all.
    #[tracing::instrument(name = "workspace.current_branch", skip_all, fields(cwd = %cwd.display()))]
    pub fn current_branch(&self, cwd: &Path) -> Option<String> {
        checkout::current_branch(cwd)
    }

    /// How the branch checked out at `cwd` stands against its upstream —
    /// what a pull would bring and a push would send — when that branch
    /// is the repository's delivery target. `None` on any other branch,
    /// and without an upstream to measure against: an agent on a branch
    /// of its own reaches the remote through the target, so the target
    /// is the one branch whose sync with it is worth a caption.
    #[tracing::instrument(name = "workspace.target_upstream_sync", skip_all, fields(cwd = %cwd.display()))]
    pub fn target_upstream_sync(&self, cwd: &Path) -> Option<UpstreamSync> {
        let repository = self.repository(cwd)?;
        if checkout::current_branch(cwd)? != repository.target() {
            return None;
        }
        let divergence = checkout::upstream_divergence(cwd)?;
        Some(UpstreamSync {
            pull: divergence.behind,
            push: divergence.ahead,
        })
    }

    /// Every task recorded for `cwd`'s repository, as last evaluated.
    #[tracing::instrument(name = "workspace.tasks", skip_all, fields(cwd = %cwd.display()))]
    pub fn tasks(&self, cwd: &Path) -> Vec<TaskView> {
        self.repository(cwd)
            .map(|repository| repository.views())
            .unwrap_or_default()
    }

    /// The project's say in delivery, for a header to name what `deliver`
    /// will do.
    /// What declaring a completion behavior would do, so a caller can say
    /// it before doing it rather than after. Writing the policy touches a
    /// *tracked* file — the one the whole team reads — and the projected
    /// `AGENTS.md` still needs `uze context reconcile` to follow it, so a
    /// click that silently did both would be a click nobody could predict.
    #[tracing::instrument(name = "workspace.completion_change_consequence", skip_all, fields(cwd = %cwd.display()))]
    pub fn completion_change_consequence(&self, cwd: &Path) -> Option<PolicyWriteConsequence> {
        let repository = self.repository(cwd)?;
        let manifest = manifest::manifest_path_for(&repository.primary);
        Some(PolicyWriteConsequence {
            creates_manifest: !manifest.exists(),
            manifest,
        })
    }

    /// Declares the completion behavior for the repository `cwd` belongs
    /// to, creating `agents.yaml` when the project has none. Reports
    /// whether it created it, so the caller can say which of the two
    /// things just happened.
    ///
    /// The policy is the primary checkout's, always: an isolated checkout
    /// declaring one of its own would be a per-worktree policy, which
    /// there is deliberately none of.
    #[tracing::instrument(name = "workspace.set_completion", skip_all, fields(cwd = %cwd.display()), err)]
    pub fn set_completion(&self, cwd: &Path, behavior: CompletionBehavior) -> Result<bool> {
        // `MissingPath` is what resolving a project root already answers
        // with when there is nothing to resolve; a policy is a repository's,
        // and outside one there is no primary checkout to declare it in.
        let primary = self
            .repository(cwd)
            .map(|repository| repository.primary)
            .ok_or_else(|| UzeError::MissingPath(cwd.to_path_buf()))?;
        manifest::set_completion(&primary, behavior)
    }

    #[tracing::instrument(name = "workspace.delivery_policy", skip_all, fields(cwd = %cwd.display()))]
    pub fn delivery_policy(&self, cwd: &Path) -> Option<DeliveryPolicyView> {
        let repository = self.repository(cwd)?;
        let declared = manifest::load(&repository.primary)
            .ok()
            .flatten()
            .and_then(|manifest| manifest.worktrees)
            .is_some();
        Some(DeliveryPolicyView {
            source: if declared {
                PolicySource::Declared
            } else {
                PolicySource::BuiltInDefault
            },
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
    #[tracing::instrument(name = "workspace.evaluate_tasks", skip_all, fields(cwd = %cwd.display()))]
    pub fn evaluate_tasks(&self, cwd: &Path, occupied: &[PathBuf]) -> Evaluation {
        let mut repository = match self.open(cwd) {
            Ok(Some(repository)) => repository,
            Ok(None) => return Evaluation::default(),
            Err(reason) => {
                return Evaluation {
                    unreadable: Some(reason),
                    ..Evaluation::default()
                };
            }
        };
        let target = repository.target();
        checkout::reconcile(&repository.primary, &mut repository.store, &target);
        let mut notices = Vec::new();
        let primary = repository.primary.clone();
        let vocabulary = repository.policy.branch.clone();
        let names_work = vocabulary.names_work();
        let owners = slot_owners(&repository.store);
        let completion = repository.policy.completion;
        for task in &mut repository.store.tasks {
            // A task that ended is still looked at while it owns its
            // slot: the agent that delivered usually keeps working in the
            // same checkout, and skipping every non-live task froze that
            // row on `delivered` for the rest of the session however much
            // the slot changed. `Closed` is the same story with nothing
            // delivered — the checkout it ended in can be written in
            // again. Only the *current* owner is reconsidered: a freed
            // slot handed to a new agent belongs to that agent's task, not
            // to the one that used to sit there. `Parked` is nobody's turn
            // by definition and stays put — unless a pane sits in its
            // checkout (`occupied`): parked means "no agent left", and an
            // agent that is there makes it a lie, whichever way it got
            // there — a release that raced the tab opening, a resume.
            let ended_owner = matches!(task.state, TaskState::Integrated | TaskState::Closed)
                && owners.contains(task.id.as_str());
            let parked_with_agent = task.state == TaskState::Parked
                && landing::slot_path(&primary, task)
                    .is_some_and(|slot| occupied.iter().any(|pane| pane.starts_with(&slot)));
            let revivable = ended_owner || parked_with_agent;
            if task.state == TaskState::Integrating
                || (!checkout::is_live(&task.state) && !revivable)
            {
                continue;
            }
            // The branch a task is on is a Git fact, and `task.branch` is
            // a cache of it. Re-read before anything is asked *about* the
            // branch: an operator renaming it by hand otherwise leaves
            // every later question pointed at a ref that no longer exists,
            // and `commits_ahead` answers such a question with `0` — which
            // reads as "nothing to deliver" rather than as "wrong branch".
            // A checkout mid-rebase is on no branch and is left alone.
            if let Some(slot) = landing::slot_path(&primary, task)
                && let Some(actual) = checkout::current_branch(&slot)
                && actual != task.branch
            {
                task.take_name(actual);
            }
            match landing::readiness(&primary, task) {
                // Nothing new since it ended leaves the ending standing:
                // the delivery is the last thing that happened to the
                // task, and saying `running` instead would erase it on the
                // next tick.
                Readiness::Running if ended_owner => {}
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
            // The work has a commit and still carries the name UZE
            // generated for it: name it from what the agent wrote. This is
            // the automatic half, and it deliberately runs *late* — until
            // there is a commit there is nothing to name the work after,
            // and the agent naming it deliberately arrives earlier and
            // therefore wins. `Ready` is the safe moment by construction:
            // commits ahead, a clean tree, no rebase in progress.
            //
            // Nothing about this reaches a harness. It is a Git fact read
            // on a pass that already runs, which is why it works on every
            // harness and on the next one.
            if names_work
                && task.state == TaskState::Ready
                && !task.is_named()
                && let Some(derived) = landing::derived_name(&primary, task, &vocabulary)
                && !checkout::branch_exists(&primary, &derived)
                && checkout::rename_branch(&primary, &task.branch.clone(), &derived).is_ok()
            {
                task.take_name(derived);
            }
            // The other half of what a delivery would do, learned the
            // same way readiness is: an agent told to push and open the
            // request itself is the one case UZE's own records can never
            // cover, and until this ran the button went on offering to
            // publish a branch the forge already had a request open for.
            // Only where a request is what completion means — a project
            // that merges or hands off never asks the remote anything.
            if completion == CompletionBehavior::Pr {
                landing::observe_request(&primary, task);
            }
            // Following a moved target costs a clean task nothing and a
            // dirty one its work in progress, which `refresh` refuses.
            // Whatever the completion behaviour: a task that follows the
            // target as it moves meets a conflict while its agent is
            // still holding the change, rather than in a request already
            // opened.
            if matches!(task.state, TaskState::Running | TaskState::Ready)
                && let Err(DeliveryFailure::Conflict {
                    files,
                    target_moved,
                }) = landing::refresh(&primary, task)
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
            unreadable: None,
        }
    }

    /// Delivers one task the way the project's completion says, one task
    /// at a time under the repository write lock.
    #[tracing::instrument(name = "workspace.deliver_task", skip_all, fields(cwd = %cwd.display(), task_id = %task_id))]
    pub fn deliver_task(&self, cwd: &Path, task_id: &str) -> Option<DeliveryReport> {
        let mut repository = self.repository(cwd)?;
        let report = repository.deliver(task_id)?;
        let _ = task::save(&self.0.home, &repository.primary, &repository.store);
        Some(report)
    }

    /// Delivers every ready task, oldest first; the second sees the first.
    #[tracing::instrument(name = "workspace.deliver_ready", skip_all, fields(cwd = %cwd.display()))]
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

    /// One pass of "who is actually sitting in which slot", across every
    /// repository the workspace can see.
    ///
    /// `look_in` names the directories worth reconsidering — the checkout
    /// a pane just left, plus (on a client's first pass, when nothing has
    /// vanished yet because nothing was ever seen) every open space's own
    /// root. `held` is every checkout a live pane still sits in, and it
    /// governs both halves: a task no pane is in front of ends, and a
    /// directory a pane *is* in is never collected, whatever its record
    /// says — the agent that delivered a task is still there until its
    /// tab closes.
    ///
    /// The sequencing is the point, and it is domain rather than
    /// presentation: several directories resolve to one repository and it
    /// must be reconciled once, not once per pane; a release must precede
    /// the collection that acts on it; and only the removals that cannot
    /// lose work are ever taken. A caller that got any of that wrong would
    /// hand one agent's slot to another.
    #[tracing::instrument(name = "workspace.reconcile_occupancy", skip_all)]
    pub fn reconcile_occupancy(&self, look_in: &[PathBuf], held: &[PathBuf]) -> Reconciliation {
        let mut reconciliation = Reconciliation::default();
        let mut seen = BTreeSet::new();
        for cwd in look_in {
            let Some(primary) = self.primary_of(cwd) else {
                continue;
            };
            if !seen.insert(primary) {
                continue;
            }
            let released = self.release_abandoned_tasks(cwd, held);
            if !released.is_empty() {
                reconciliation.changed.push(cwd.clone());
                reconciliation.released.extend(released);
            }
            // Only ever the removals that cannot lose work, and only from
            // the path that just changed what "in use" means.
            self.collect_slot_garbage(cwd, held);
        }
        reconciliation
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
    #[tracing::instrument(name = "workspace.release_abandoned_tasks", skip_all, fields(cwd = %cwd.display()))]
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
    /// is ever touched here, and nothing a live pane sits in (`occupied`);
    /// that is the operator's alone.
    #[tracing::instrument(name = "workspace.collect_slot_garbage", skip_all, fields(cwd = %cwd.display()))]
    pub fn collect_slot_garbage(&self, cwd: &Path, occupied: &[PathBuf]) -> Vec<String> {
        let Some(repository) = self.repository(cwd) else {
            return Vec::new();
        };
        let target = repository.target();
        let collected = checkout::collect(
            &repository.primary,
            &repository.store,
            &target,
            checkout::IDLE_SLOT_AGE,
            occupied,
        );
        collected
            .branches
            .into_iter()
            .chain(collected.slots.into_iter().map(|slot| slot.to_string()))
            .collect()
    }

    /// The operator declares a handed-off task done: its slot is free and
    /// its branch stays.
    #[tracing::instrument(name = "workspace.finish_task", skip_all, fields(cwd = %cwd.display(), task_id = %task_id), err)]
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
    #[tracing::instrument(name = "workspace.discard_task", skip_all, fields(cwd = %cwd.display(), task_id = %task_id), err)]
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
        // The one place a task stops existing, and therefore the one place
        // its conversations stop being reachable. Finishing is deliberately
        // not such a place: an integrated task whose agent kept working is
        // revived by reconciliation, and it would come back without the
        // conversation it never left.
        conversation::forget(&self.0.home, &repository.primary, &task.id);
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
            .map(|task| TaskView::from_task(&self.primary, task, self.policy.completion))
            .collect()
    }

    fn deliver(&mut self, task_id: &str) -> Option<DeliveryReport> {
        let completion = self.policy.completion;
        let gate = self.policy.gate.clone();
        let policy = landing::Policy {
            completion,
            gate: &gate,
        };
        let primary = self.primary.clone();
        let task = self.task_mut(task_id)?;
        let outcome = match landing::deliver(&primary, task, &policy) {
            Ok(Delivered::Handoff) => DeliveryOutcome::Handoff,
            Ok(Delivered::Merged { .. }) => DeliveryOutcome::Merged,
            Ok(Delivered::Published { branch, request }) => {
                DeliveryOutcome::Published { branch, request }
            }
            Ok(Delivered::AwaitingRequest {
                branch: _,
                instruction,
            }) => DeliveryOutcome::AwaitingRequest(AgentNotice {
                task: task.id.as_str().to_owned(),
                checkout: landing::slot_path(&primary, task).unwrap_or_default(),
                message: instruction,
            }),
            Err(DeliveryFailure::Conflict {
                files,
                target_moved,
            }) => DeliveryOutcome::ReturnedToAgent(AgentNotice {
                task: task.id.as_str().to_owned(),
                checkout: landing::slot_path(&primary, task).unwrap_or_default(),
                message: landing::conflict_message(task, &files, target_moved),
            }),
            Err(DeliveryFailure::GateFailed { command, output }) => {
                DeliveryOutcome::ReturnedToAgent(AgentNotice {
                    task: task.id.as_str().to_owned(),
                    checkout: landing::slot_path(&primary, task).unwrap_or_default(),
                    message: landing::gate_failure_message(task, &command, &output),
                })
            }
            Err(other) => DeliveryOutcome::Refused(other.to_string()),
        };
        Some(DeliveryReport {
            task: TaskView::from_task(&primary, task, completion),
            outcome,
        })
    }
}

/// The task currently answering for each occupied slot, by id.
///
/// A checkout id can be named by more than one task over its life — a slot
/// goes back to the pool and the next agent takes it — and the newest one
/// is the owner, which is the rule `checkout::slot_state` already reads
/// slots by. Anything older is history and must not be revived by what the
/// directory now holds, because what it holds is somebody else's work.
fn slot_owners(store: &TaskStore) -> BTreeSet<String> {
    let mut newest: BTreeMap<&str, &Task> = BTreeMap::new();
    for task in &store.tasks {
        let Some(checkout) = &task.checkout else {
            continue;
        };
        newest
            .entry(checkout.as_str())
            .and_modify(|held| {
                if task.created_at_unix >= held.created_at_unix {
                    *held = task;
                }
            })
            .or_insert(task);
    }
    newest
        .into_values()
        .map(|task| task.id.as_str().to_owned())
        .collect()
}

/// What naming a task produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedTask {
    pub task: String,
    pub branch: String,
    pub label: String,
}

/// A refusal, in words an agent can act on: which half was wrong, and what
/// this project would have accepted.
fn refusal_words(refusal: &NameRefusal, vocabulary: &BranchVocabulary) -> String {
    match refusal {
        NameRefusal::NotDeclared => {
            "this project does not name agent work: declare `worktrees.branch` in agents.yaml"
                .to_owned()
        }
        NameRefusal::UnknownType { found, .. } => format!(
            "`{found}` is not a type this project accepts; use one of `{}`",
            vocabulary.spelled()
        ),
        NameRefusal::UnexpectedType { found } => format!(
            "this project takes {}, so drop the `{found}/`",
            vocabulary.spelled()
        ),
        NameRefusal::MissingType { .. } => format!(
            "a name is `<type>/<subject>`; the types this project accepts are `{}`",
            vocabulary.spelled()
        ),
        NameRefusal::MalformedSubject { reason } => format!(
            "the subject is one or two words naming the intention, and {reason} — \
             try something like `fix/branch-naming`"
        ),
    }
}

/// A task ended because its agent is gone, and what became of its slot.
/// What one pass of [`Workspace::reconcile_occupancy`] changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reconciliation {
    /// The directories whose repository actually gave a slot up, so a
    /// caller knows which tasks are worth re-reading. Empty is the
    /// ordinary answer.
    pub changed: Vec<PathBuf>,
    pub released: Vec<ReleasedTask>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasedTask {
    pub id: String,
    pub label: String,
    /// `true` when the checkout held work and was parked for the operator
    /// instead of going back to the pool.
    pub parked: bool,
}

/// The delivery target against its upstream: commits a pull would bring
/// in and a push would send out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpstreamSync {
    pub pull: usize,
    pub push: usize,
}

/// One task as presentation sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskView {
    pub id: String,
    pub label: String,
    pub branch: String,
    pub target: String,
    /// The slot's directory, when the task has one on disk.
    pub checkout: Option<PathBuf>,
    /// The slot the task was given, whether or not its directory still
    /// exists — what ties a pane standing in a removed checkout back to
    /// the task it was running.
    pub checkout_id: Option<String>,
    pub state: TaskStateView,
    /// What delivering this task does — the project's own say, carried on
    /// the task so a surface offering the delivery can name its ending
    /// instead of showing one verb for three different outcomes.
    pub completion: CompletionBehavior,
    /// Commits the branch has beyond its base — what a delivery would land.
    pub ahead: usize,
    /// The name the branch is published under on the remote, once it is
    /// there. Read from the repository's remote-tracking refs, so a
    /// branch its own agent pushed reads as published exactly like one
    /// UZE pushed.
    pub published_as: Option<String>,
    /// The request open on the forge for the published branch, once there
    /// is one: what turns the delivery button from an errand into a sync.
    pub published_request: Option<u32>,
    /// Commits the published branch does not carry yet — what a sync would
    /// send. `None` until the branch has been published at all, and
    /// `Some(0)` once the request is level with the branch: work already
    /// handed over is not work waiting to be handed over, however far the
    /// branch still is from the target, which only a merge closes.
    pub unsynced: Option<usize>,
    pub created_at_unix: u64,
}

impl TaskView {
    fn from_task(primary: &Path, task: &Task, completion: CompletionBehavior) -> Self {
        // What the remote holds, not what UZE remembers having sent: a
        // push the agent made is a push, and a view built from UZE's own
        // record of its own deliveries goes on offering to send commits
        // the request already carries.
        //
        // Only where the completion publishes. A branch on the remote is
        // no part of what a merge or a handoff would do, and counting a
        // merge's commits against the remote would report a task as
        // delivered the moment its agent pushed it.
        let published = (completion == CompletionBehavior::Pr)
            .then(|| landing::publication(primary, task))
            .flatten();
        let unsynced = published
            .as_ref()
            .map(|published| checkout::commits_ahead(primary, &published.tip, &task.branch));
        Self {
            id: task.id.as_str().to_owned(),
            label: task.label.clone(),
            branch: task.branch.clone(),
            target: task.target.clone(),
            checkout: landing::slot_path(primary, task),
            checkout_id: task
                .checkout
                .as_ref()
                .map(|checkout| checkout.as_str().to_owned()),
            state: publication_state(&task.state, unsynced),
            completion,
            ahead: checkout::commits_ahead(primary, &task.base_commit, &task.branch),
            published_as: published.map(|published| published.branch),
            published_request: task.published_request,
            unsynced,
            created_at_unix: task.created_at_unix,
        }
    }
}

/// The state a task reads as once what the remote holds is folded in.
///
/// `Ready` alone answers "the branch holds commits its base lacks", which
/// stops being the interesting question the moment the branch is on the
/// remote: from then on what every surface needs to say is whether
/// anything is still waiting to be handed over. Only where the completion
/// publishes — `published` is `None` everywhere else, and the record's own
/// state stands.
fn publication_state(state: &TaskState, unsynced: Option<usize>) -> TaskStateView {
    let view = TaskStateView::from(state);
    if view == TaskStateView::Ready && unsynced == Some(0) {
        return TaskStateView::Published;
    }
    view
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskStateView {
    Running,
    Uncommitted,
    Ready,
    /// The branch is on the remote and carries nothing the remote lacks:
    /// the work is with whoever reviews it, not with the operator.
    ///
    /// A view of `Ready`, not a record of its own — `Ready` is still what
    /// the branch holds, and pressing deliver still syncs it as the target
    /// moves. It is a state here because "there is work to hand over" and
    /// "the work is handed over" are the two things every surface has to
    /// tell apart, and a surface that reads only `Ready` cannot: the
    /// sidebar went on marking a task deliverable for the whole life of an
    /// open request.
    Published,
    Integrating,
    Conflicted {
        files: Vec<PathBuf>,
    },
    GateFailed,
    Integrated,
    Parked,
    /// The agent is gone and its branch held nothing to deliver.
    Closed,
}

impl TaskStateView {
    /// Whether delivery may be offered for a task in this state.
    ///
    /// `Published` included: the request is level with the branch, but the
    /// target moves, and a re-sync is how the branch follows it.
    pub fn is_deliverable(&self) -> bool {
        matches!(self, Self::Ready | Self::Published | Self::GateFailed)
    }

    /// Why delivery is refused, for a state where it is — `None` for the
    /// states [`is_deliverable`](Self::is_deliverable) accepts.
    ///
    /// Beside the predicate rather than beside whoever shows the answer:
    /// "not yet" and "already done" are the same refusal to a caller that
    /// only sees a boolean, and a second surface asking the same question
    /// would otherwise write its own second version of these words.
    ///
    /// A few words each: these are read in the header's own row, beside
    /// the button that was just pressed, where the state's mark and the
    /// tab already carry everything the sentence would repeat.
    pub fn undeliverable_reason(&self) -> Option<&'static str> {
        match self {
            Self::Ready | Self::Published | Self::GateFailed => None,
            Self::Running => Some("nothing committed"),
            Self::Uncommitted => Some("uncommitted changes"),
            Self::Conflicted { .. } => Some("rebase paused"),
            Self::Integrating => Some("already delivering"),
            Self::Integrated => Some("already delivered"),
            Self::Closed => Some("branch holds nothing"),
            Self::Parked => Some("parked — resume it first"),
        }
    }

    /// Whether the task still has an agent's work in front of it.
    pub fn is_live(&self) -> bool {
        !matches!(self, Self::Integrated | Self::Parked | Self::Closed)
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
            TaskState::Closed => Self::Closed,
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
    /// Why the repository's recorded tasks could not be read, when they
    /// could not be.
    ///
    /// An empty `tasks` says "this repository has no tasks", and a store
    /// that failed to open says something entirely different — every agent
    /// loses its branch, its mark and its delivery button, and the surface
    /// that swallowed the error has no way to say why. `place_new_agent`
    /// already reported this and was the only thing that did, so the
    /// condition surfaced as a single truncated line the one time somebody
    /// happened to add an agent.
    pub unreadable: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryOutcome {
    Handoff,
    Merged,
    /// The branch was pushed and the forge already has this request
    /// open for it: from here delivery is a sync, and Git alone.
    Published {
        branch: String,
        request: u32,
    },
    /// The branch was pushed and has no request yet, so the owning agent
    /// was handed the words to open one. Carries an [`AgentNotice`] like
    /// the two failures do, because it reaches the agent the same way: a
    /// submission into its pane.
    AwaitingRequest(AgentNotice),
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

/// What writing the policy is about to do to the project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyWriteConsequence {
    /// The file does not exist yet, so declaring adds a tracked file to
    /// the repository rather than editing one.
    pub creates_manifest: bool,
    pub manifest: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryPolicyView {
    pub completion: &'static str,
    pub target: Option<String>,
    pub gate: Vec<String>,
    /// Whether this is what the project declared or what UZE falls back to.
    /// A reader who cannot tell the two apart learns nothing from being
    /// shown `handoff`: they cannot know whether anyone chose it.
    pub source: PolicySource,
}

/// Where the policy in force came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicySource {
    /// `agents.yaml` declares it.
    Declared,
    /// Nothing declares it; this is the built-in default, identical on
    /// every machine.
    BuiltInDefault,
}

impl PolicySource {
    /// How the client attributes it, in the words a reader needs rather
    /// than the words the code uses.
    pub fn attribution(self) -> &'static str {
        match self {
            Self::Declared => "agents.yaml",
            Self::BuiltInDefault => "default",
        }
    }
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

    /// An agent is placed on the target's tip, so the target has to be the
    /// one the team is on rather than the one this machine last saw: a
    /// branch cut from a stale tip conflicts in a request already opened.
    #[test]
    fn a_new_agent_starts_from_the_target_as_the_remote_has_it() {
        let repository = repository("place-synced");
        let root = repository.root().to_path_buf();
        let base = uze_testkit::temp::scratch("place-synced-remote");
        let origin = base.join("origin.git");
        repository.git(&[
            "init",
            "--quiet",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ]);
        repository.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
        repository.git(&["push", "--quiet", "-u", "origin", "main"]);
        let other = base.join("other");
        repository.git(&[
            "clone",
            "--quiet",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ]);
        repository.git_in(&other, &["config", "user.name", "Other"]);
        repository.git_in(&other, &["config", "user.email", "other@uze.invalid"]);
        std::fs::write(other.join("merged-while-you-were-away.rs"), "").unwrap();
        repository.git_in(&other, &["add", "."]);
        repository.git_in(&other, &["commit", "-qm", "merged while you were away"]);
        repository.git_in(&other, &["push", "--quiet"]);

        let app = application("place-synced-home");
        let placement = app.workspace().place_new_agent(&root, &[]);

        assert!(
            placement
                .cwd
                .join("merged-while-you-were-away.rs")
                .is_file(),
            "the agent starts from what the remote has, not from the local tip"
        );
        assert!(
            placement.warnings.is_empty(),
            "nothing to report when the target could be moved: {:?}",
            placement.warnings
        );
    }

    #[test]
    fn the_first_agent_is_isolated() {
        let repository = repository("place-first");
        let root = repository.root().to_path_buf();
        let app = application("place-first-home");
        let placement = app.workspace().place_new_agent(&root, &[]);
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

        let first = app.workspace().place_new_agent(&root, &[]);
        let abandoned_branch = repository.branch_of(&first.cwd);
        let released = app.workspace().release_abandoned_tasks(&root, &[]);
        assert_eq!(released.len(), 1);
        assert!(!released[0].parked, "an empty checkout holds nothing");

        let second = app.workspace().place_new_agent(&root, &[]);
        assert_eq!(
            second.cwd, first.cwd,
            "the freed slot is reused instead of a new directory"
        );

        // The branch it left behind carries nothing the target lacks, so
        // the safe collection takes it once the slot has moved off it.
        app.workspace().collect_slot_garbage(&root, &[]);
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

        let abandoned = app.workspace().place_new_agent(&root, &[]);
        std::fs::write(abandoned.cwd.join("draft.rs"), b"unsaved").unwrap();
        let released = app.workspace().release_abandoned_tasks(&root, &[]);
        assert_eq!(released.len(), 1);
        assert!(released[0].parked);

        let next = app.workspace().place_new_agent(&root, &[]);
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
        let placement = app.workspace().place_new_agent(&root, &[]);
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
            .map(|_| app.workspace().place_new_agent(&root, &[]))
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

        app.workspace().place_new_agent(&root, &[]);
        app.workspace().place_new_agent(&root, &[]);

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
        let placement = app.workspace().place_new_agent(&root, &[]);
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
        let placement = app.workspace().place_new_agent(&outside, &[]);
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
        let first = app.workspace().place_new_agent(&root, &[]);
        let primary = root.canonicalize().unwrap();
        let mut store = task::load(&app.home, &primary).unwrap();
        store.get_mut(slot(&first)).unwrap().state = uze_core::task::TaskState::Integrated;
        task::save(&app.home, &primary, &store).unwrap();

        let second = app.workspace().place_new_agent(&root, &[]);
        assert_eq!(second.cwd, first.cwd);
        assert!(matches!(
            second.isolation,
            Isolation::Slot { reused: true, .. }
        ));
    }

    /// The agent that delivered a task is still in its checkout until its
    /// tab closes: the record says done, the pane says occupied, and the
    /// pane wins — the next agent gets a directory of its own.
    #[test]
    fn a_delivered_tasks_slot_stays_its_agents_while_a_pane_sits_in_it() {
        let repository = repository("place-occupied");
        let root = repository.root().to_path_buf();
        let app = application("place-occupied-home");
        let first = app.workspace().place_new_agent(&root, &[]);
        let primary = root.canonicalize().unwrap();
        let mut store = task::load(&app.home, &primary).unwrap();
        store.get_mut(slot(&first)).unwrap().state = uze_core::task::TaskState::Integrated;
        task::save(&app.home, &primary, &store).unwrap();

        let still_inside = vec![first.cwd.clone()];
        let second = app.workspace().place_new_agent(&root, &still_inside);
        assert_ne!(
            second.cwd, first.cwd,
            "never the checkout somebody is still in"
        );
        assert!(matches!(
            second.isolation,
            Isolation::Slot { reused: false, .. }
        ));
    }

    /// A checkout removed by hand orphans its task; resuming the task gives
    /// it a slot again, on the same branch, with its commits in place.
    #[test]
    fn a_task_whose_checkout_was_removed_resumes_into_a_slot_on_its_branch() {
        let repository = repository("place-resume");
        let root = repository.root().to_path_buf();
        let app = application("place-resume-home");
        let first = app.workspace().place_new_agent(&root, &[]);
        let task_id = slot(&first).as_str().to_owned();
        std::fs::write(first.cwd.join("kept.rs"), b"fn kept() {}").unwrap();
        repository.git_in(&first.cwd, &["add", "."]);
        repository.git_in(&first.cwd, &["commit", "-qm", "kept"]);
        std::fs::remove_dir_all(&first.cwd).unwrap();

        // What the TUI does once no pane is in front of the checkout.
        let released = app.workspace().release_abandoned_tasks(&root, &[]);
        assert!(
            released
                .iter()
                .any(|task| task.id == task_id && task.parked)
        );
        let task = app
            .workspace()
            .tasks(&root)
            .into_iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(task.checkout, None, "the directory is gone");
        assert_eq!(task.state, TaskStateView::Parked);

        let resumed = app.workspace().resume_task(&root, &task_id, &[]).unwrap();
        assert!(resumed.cwd.join("kept.rs").is_file(), "the commit is back");
        assert!(matches!(
            &resumed.isolation,
            Isolation::Slot { task, branch, .. }
                if task.as_str() == task_id && *branch == format!("agent/{task_id}")
        ));
        let task = app
            .workspace()
            .tasks(&root)
            .into_iter()
            .find(|task| task.id == task_id)
            .unwrap();
        assert_eq!(task.checkout.as_deref(), Some(resumed.cwd.as_path()));
        assert_eq!(task.state, TaskStateView::Running, "live again");
    }

    /// Parked says "no agent left". A pane sitting in the task's checkout
    /// says otherwise, and wins: the evaluation reads the task as live
    /// again instead of leaving a working agent marked as set aside.
    #[test]
    fn a_parked_task_with_a_pane_in_its_checkout_is_live_again() {
        let repository = repository("place-parked-live");
        let root = repository.root().to_path_buf();
        let app = application("place-parked-live-home");
        let first = app.workspace().place_new_agent(&root, &[]);
        let task_id = slot(&first).as_str().to_owned();
        std::fs::write(first.cwd.join("work.rs"), b"fn work() {}").unwrap();
        // Released as if no pane were there: parked, since it holds work.
        let released = app.workspace().release_abandoned_tasks(&root, &[]);
        assert!(
            released
                .iter()
                .any(|task| task.id == task_id && task.parked)
        );

        let state_of = |occupied: &[PathBuf]| {
            app.workspace()
                .evaluate_tasks(&root, occupied)
                .tasks
                .into_iter()
                .find(|task| task.id == task_id)
                .unwrap()
                .state
        };
        assert_eq!(
            state_of(&[]),
            TaskStateView::Parked,
            "nobody there: stays put"
        );
        assert_eq!(
            state_of(&[first.cwd.join("src")]),
            TaskStateView::Uncommitted,
            "a pane inside makes it that agent's task again"
        );
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

    fn declare(repository: &uze_testkit::git::Repository, policy: &str) {
        std::fs::write(
            repository.root().join("agents.yaml"),
            format!("worktrees:\n{policy}"),
        )
        .unwrap();
    }

    fn launched(app: &UzeApplication, root: &Path) -> (String, PathBuf) {
        let placement = app.workspace().place_new_agent(root, &[]);
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

    fn view_of(app: &UzeApplication, root: &Path, id: &str) -> TaskView {
        app.workspace()
            .tasks(root)
            .into_iter()
            .find(|task| task.id == id)
            .expect("the task is recorded")
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
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-merge-home");
        let (id, slot) = launched(&app, &root);
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Running);

        std::fs::write(slot.join("draft.rs"), "").unwrap();
        assert_eq!(
            app.workspace().evaluate_tasks(&root, &[]).tasks[0].state,
            TaskStateView::Uncommitted
        );
        agent_commits(&repository, &slot, "draft.rs", "fn done() {}");
        let evaluation = app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(evaluation.tasks[0].state, TaskStateView::Ready);
        assert!(evaluation.notices.is_empty());

        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert_eq!(report.outcome, DeliveryOutcome::Merged);
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Integrated);
        assert!(root.join("draft.rs").is_file());
    }

    /// An agent almost never stops at its first delivery: it keeps working
    /// in the same slot. Skipping every task that was not live froze that
    /// row on `delivered` for the rest of the session, however much the
    /// checkout changed underneath it.
    #[test]
    fn a_delivered_task_still_in_its_slot_is_read_again() {
        let repository = repository("svc-redeliver");
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-redeliver-home");
        let (id, slot) = launched(&app, &root);

        agent_commits(&repository, &slot, "first.rs", "fn first() {}");
        app.workspace().deliver_task(&root, &id).unwrap();
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Integrated);

        // Nothing new: the delivery is the last thing that happened, and
        // an evaluation must not talk it back down to `running`.
        app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(state_of(&app, &root, &id), TaskStateView::Integrated);

        // The same agent carries on in the same checkout.
        std::fs::write(slot.join("second.rs"), "fn second() {}").unwrap();
        app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(
            state_of(&app, &root, &id),
            TaskStateView::Uncommitted,
            "changes in the slot are seen after a delivery, not only before one"
        );

        agent_commits(&repository, &slot, "second.rs", "fn second() {}");
        app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(
            state_of(&app, &root, &id),
            TaskStateView::Ready,
            "and it becomes deliverable a second time"
        );
    }

    /// A slot outlives the task that used to sit in it. What the directory
    /// holds now answers for whoever holds it now.
    #[test]
    fn a_delivered_task_whose_slot_moved_on_is_left_alone() {
        let repository = repository("svc-handover");
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-handover-home");

        let (first, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "first.rs", "fn first() {}");
        app.workspace().deliver_task(&root, &first).unwrap();
        assert_eq!(state_of(&app, &root, &first), TaskStateView::Integrated);

        // The freed slot goes to the next agent, who dirties it.
        let (second, reused) = launched(&app, &root);
        assert_eq!(reused, slot, "the delivered slot was free to reuse");
        std::fs::write(reused.join("draft.rs"), "in progress").unwrap();

        app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(
            state_of(&app, &root, &second),
            TaskStateView::Uncommitted,
            "the work in the slot belongs to the agent sitting in it"
        );
        assert_eq!(
            state_of(&app, &root, &first),
            TaskStateView::Integrated,
            "and never revives the task that handed the slot over"
        );
    }

    #[test]
    fn a_conflict_returns_a_notice_addressed_to_the_slot() {
        let repository = repository("svc-conflict");
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-conflict-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "shared.rs", "agent\n");
        repository.commit_file("shared.rs", "operator\n");

        // The clean task follows the target on evaluation, and the
        // conflict that produces is already the agent's to resolve; a
        // delivery asked for meanwhile is refused, never forced.
        let evaluation = app.workspace().evaluate_tasks(&root, &[]);
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
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-follow-home");
        let (_, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "mine.rs", "agent's mine\n");
        repository.commit_file("theirs.rs", "");
        let evaluation = app.workspace().evaluate_tasks(&root, &[]);
        assert!(evaluation.notices.is_empty());
        assert!(
            slot.join("theirs.rs").is_file(),
            "rebased onto the moved target"
        );

        repository.commit_file("mine.rs", "operator's mine\n");
        let evaluation = app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(evaluation.notices.len(), 1);
        assert_eq!(evaluation.notices[0].checkout, slot);
    }

    #[test]
    fn the_locks_gate_refuses_and_a_passing_gate_lets_it_through() {
        let repository = repository("svc-gate");
        declare(
            &repository,
            "  completion: merge\n  gate: test -f must-exist\n",
        );
        let root = repository.root().to_path_buf();
        let app = application("svc-gate-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "a.rs", "");
        app.workspace().evaluate_tasks(&root, &[]);

        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert!(
            matches!(report.outcome, DeliveryOutcome::ReturnedToAgent(_)),
            "{:?}",
            report.outcome
        );
        assert_eq!(state_of(&app, &root, &id), TaskStateView::GateFailed);

        agent_commits(&repository, &slot, "must-exist", "");
        app.workspace().evaluate_tasks(&root, &[]);
        let report = app.workspace().deliver_task(&root, &id).unwrap();
        assert_eq!(report.outcome, DeliveryOutcome::Merged);
    }

    #[test]
    fn deliver_ready_takes_them_in_order_and_the_second_sees_the_first() {
        let repository = repository("svc-ready");
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let app = application("svc-ready-home");
        let (_, first) = launched(&app, &root);
        let (_, second) = launched(&app, &root);
        agent_commits(&repository, &first, "first.rs", "");
        agent_commits(&repository, &second, "second.rs", "");
        app.workspace().evaluate_tasks(&root, &[]);

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
        app.workspace().evaluate_tasks(&root, &[]);
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

    /// The sidebar captions the operator's own tree with what a pull and
    /// a push would move — on the target only. A branch of its own is
    /// delivered through the target, so its upstream is nobody's caption.
    #[test]
    fn the_targets_sync_with_its_upstream_is_read_on_the_target_alone() {
        let repository = repository("svc-upstream-sync");
        let root = repository.root().to_path_buf();
        let app = application("svc-upstream-sync-home");
        assert_eq!(
            app.workspace().target_upstream_sync(&root),
            None,
            "no upstream"
        );

        repository.git(&["branch", "upstream"]);
        repository.git(&["branch", "--set-upstream-to=upstream"]);
        repository.commit_file("mine.txt", "pushable\n");
        assert_eq!(
            app.workspace().target_upstream_sync(&root),
            Some(UpstreamSync { pull: 0, push: 1 })
        );

        declare(&repository, "  target: upstream\n");
        assert_eq!(
            app.workspace().target_upstream_sync(&root),
            None,
            "the checked-out branch is not the target"
        );
    }

    /// The delivery button reads the remote, not UZE's memory of its own
    /// pushes. An operator who asks the agent to commit, push and open the
    /// request itself has done everything a delivery would have done, and
    /// the button has to say so — it used to go on offering to publish a
    /// branch that was already on the remote with a request open for it.
    #[test]
    fn an_agents_own_push_and_request_are_what_the_delivery_view_reports() {
        let repository = repository("svc-agent-publish");
        declare(
            &repository,
            "  completion: pr
",
        );
        let root = repository.root().to_path_buf();
        let remote = uze_testkit::temp::scratch("svc-agent-publish-remote").join("origin.git");
        repository.git(&["init", "--quiet", "--bare", remote.to_str().unwrap()]);
        repository.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        repository.git(&["push", "--quiet", "origin", "HEAD"]);

        let app = application("svc-agent-publish-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "a.rs", "");
        app.workspace().evaluate_tasks(&root, &[]);
        let before = view_of(&app, &root, &id);
        assert_eq!(before.published_as, None);
        assert_eq!(before.unsynced, None, "nothing is on the remote yet");

        // The agent does both halves itself.
        repository.git_in(&slot, &["push", "--quiet", "origin", "HEAD"]);
        let tip = repository.git_in(&slot, &["rev-parse", "HEAD"]);
        repository.git(&[
            "push",
            "--quiet",
            "origin",
            &format!("{}:refs/pull/12/head", tip.trim()),
        ]);
        app.workspace().evaluate_tasks(&root, &[]);

        let synced = view_of(&app, &root, &id);
        assert_eq!(synced.published_as.as_deref(), Some(synced.branch.as_str()));
        assert_eq!(synced.unsynced, Some(0), "nothing left to send");
        assert_eq!(
            synced.state,
            TaskStateView::Published,
            "the work is with its reviewer, not waiting to be handed over"
        );
        assert_eq!(
            synced.state.undeliverable_reason(),
            None,
            "and a re-sync still follows a target that moves"
        );
        assert_eq!(
            synced.published_request,
            Some(12),
            "the request the agent opened is this branch's request"
        );

        agent_commits(&repository, &slot, "b.rs", "");
        app.workspace().evaluate_tasks(&root, &[]);
        let behind = view_of(&app, &root, &id);
        assert_eq!(
            behind.unsynced,
            Some(1),
            "and a commit made after that push is one commit to sync"
        );
        assert_eq!(
            behind.state,
            TaskStateView::Ready,
            "which puts the work back in the operator's hands"
        );
    }

    /// A merge lands on the target, and a branch sitting on the remote is
    /// no part of that. Reading publication for it would have called the
    /// task synced the moment its agent pushed — before the one thing the
    /// completion actually does had happened at all.
    #[test]
    fn a_merge_project_never_measures_its_work_against_the_remote() {
        let repository = repository("svc-merge-remote");
        declare(&repository, "  completion: merge\n");
        let root = repository.root().to_path_buf();
        let remote = uze_testkit::temp::scratch("svc-merge-remote-origin").join("origin.git");
        repository.git(&["init", "--quiet", "--bare", remote.to_str().unwrap()]);
        repository.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        repository.git(&["push", "--quiet", "origin", "HEAD"]);

        let app = application("svc-merge-remote-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "a.rs", "");
        repository.git_in(&slot, &["push", "--quiet", "origin", "HEAD"]);
        app.workspace().evaluate_tasks(&root, &[]);

        let view = view_of(&app, &root, &id);
        assert_eq!(view.published_as, None);
        assert_eq!(view.unsynced, None, "the merge has not happened");
        assert_eq!(view.state, TaskStateView::Ready, "so it is still to do");
        assert_eq!(view.ahead, 1, "and that is what it would land");
    }

    /// "This repository has no tasks" and "this repository's tasks could
    /// not be read" are opposite facts, and the evaluation used to answer
    /// both with an empty list. Every agent then lost its branch, its mark
    /// and its delivery button at once, with nothing said — the condition
    /// surfaced only as a truncated line the next time somebody happened
    /// to add an agent.
    #[test]
    fn an_unreadable_task_store_is_an_answer_not_an_empty_one() {
        let repository = repository("svc-unreadable");
        let root = repository.root().to_path_buf();
        let app = application("svc-unreadable-home");
        let (id, slot) = launched(&app, &root);
        agent_commits(&repository, &slot, "a.rs", "");
        let evaluation = app.workspace().evaluate_tasks(&root, &[]);
        assert_eq!(evaluation.unreadable, None);
        assert!(evaluation.tasks.iter().any(|task| task.id == id));

        std::fs::write(
            task::store_path(&app.home, &root.canonicalize().unwrap()),
            "{ this is not the document",
        )
        .unwrap();
        let evaluation = app.workspace().evaluate_tasks(&root, &[]);
        assert!(
            evaluation.unreadable.is_some(),
            "the reason is carried, not swallowed"
        );
        assert!(
            evaluation.tasks.is_empty(),
            "and nothing is invented to stand in for what could not be read"
        );
    }

    #[test]
    fn the_locks_target_cap_links_and_setup_shape_the_launch() {
        let repository = repository("svc-lock-launch");
        repository.git(&["branch", "develop"]);
        std::fs::write(repository.root().join(".env"), "KEY=1\n").unwrap();
        declare(
            &repository,
            "  target: develop\n  slots: 1\n  link: [.env]\n  setup: touch prepared\n",
        );
        let root = repository.root().to_path_buf();
        let app = application("svc-lock-launch-home");

        let placement = app.workspace().place_new_agent(&root, &[]);
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

        let second = app.workspace().place_new_agent(&root, &[]);
        assert!(
            matches!(&second.isolation, Isolation::Unisolated { reason } if reason.contains("1 declared")),
            "{second:?}"
        );
    }
}

/// Naming the work, and what refuses to overwrite it.
///
/// The whole point of this tier is that these are Git and filesystem
/// facts: the branch a checkout is on, the record UZE keeps, and what a
/// second attempt does to both.
#[cfg(test)]
mod naming_tests {
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

    /// A project that names its work. Declared rather than defaulted,
    /// because an undeclared vocabulary is exactly the project that must
    /// keep its old behaviour.
    fn naming_project(label: &str) -> (UzeApplication, uze_testkit::git::Repository) {
        let repository = repository(label);
        std::fs::write(
            repository.root().join("agents.yaml"),
            "worktrees:\n  branch: conventional\n",
        )
        .unwrap();
        (application(label), repository)
    }

    fn placed(app: &UzeApplication, root: &Path) -> PathBuf {
        match app.workspace().place_new_agent(root, &[]).isolation {
            Isolation::Slot { .. } => {}
            Isolation::Unisolated { reason } => panic!("{reason}"),
        }
        app.workspace()
            .tasks(root)
            .last()
            .and_then(|task| task.checkout.clone())
            .expect("the placed agent has a checkout")
    }

    fn branch_of(checkout: &Path) -> String {
        checkout::current_branch(checkout).expect("the checkout is on a branch")
    }

    #[test]
    fn naming_renames_the_branch_and_records_the_label() {
        let (app, repository) = naming_project("naming-basic");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        assert!(branch_of(&checkout).starts_with("agent/"));

        let named = app
            .workspace()
            .name_task(&checkout, "fix/branch-naming")
            .unwrap();

        assert_eq!(named.branch, "fix/branch-naming");
        assert_eq!(named.label, "branch naming");
        assert_eq!(
            branch_of(&checkout),
            "fix/branch-naming",
            "Git is where the rename actually happened"
        );
        let task = app
            .workspace()
            .tasks(&root)
            .into_iter()
            .find(|task| task.id == named.task)
            .unwrap();
        assert_eq!(task.branch, "fix/branch-naming");
        assert_eq!(task.label, "branch naming");
        assert_eq!(
            task.checkout.as_deref(),
            Some(checkout.as_path()),
            "the slot directory is not renamed with the branch"
        );
    }

    /// Resolved from the directory, so any depth inside the checkout is the
    /// same answer.
    #[test]
    fn a_nested_directory_names_the_checkouts_own_task() {
        let (app, repository) = naming_project("naming-nested");
        let checkout = placed(&app, repository.root());
        let nested = checkout.join("deep/inside");
        std::fs::create_dir_all(&nested).unwrap();

        app.workspace()
            .name_task(&nested, "feat/from-below")
            .unwrap();

        assert_eq!(branch_of(&checkout), "feat/from-below");
    }

    /// The primary belongs to the operator: there is no task there to name,
    /// and no argument that would let one be named from here.
    #[test]
    fn the_primary_checkout_has_nothing_to_name() {
        let (app, repository) = naming_project("naming-primary");
        let root = repository.root().to_path_buf();
        let before = branch_of(&root);
        let error = app
            .workspace()
            .name_task(&root, "fix/not-here")
            .unwrap_err()
            .to_string();
        assert!(error.contains("operator"), "{error}");
        assert_eq!(branch_of(&root), before, "nothing was renamed");
    }

    /// First-writer-wins: the second call is refused and the first name
    /// stands, in Git as well as in the record.
    #[test]
    fn a_second_name_is_refused_and_the_first_one_stands() {
        let (app, repository) = naming_project("naming-twice");
        let checkout = placed(&app, repository.root());
        app.workspace()
            .name_task(&checkout, "fix/first-name")
            .unwrap();

        let error = app
            .workspace()
            .name_task(&checkout, "fix/second-name")
            .unwrap_err()
            .to_string();

        assert!(error.contains("already named"), "{error}");
        assert_eq!(branch_of(&checkout), "fix/first-name");
    }

    #[test]
    fn a_name_outside_the_vocabulary_is_refused_naming_what_is_accepted() {
        let (app, repository) = naming_project("naming-vocabulary");
        let checkout = placed(&app, repository.root());
        let before = branch_of(&checkout);

        let error = app
            .workspace()
            .name_task(&checkout, "ui/dark-mode")
            .unwrap_err()
            .to_string();

        assert!(error.contains("ui"), "{error}");
        assert!(
            error.contains("feat"),
            "the refusal names what is accepted: {error}"
        );
        assert_eq!(branch_of(&checkout), before, "nothing was renamed");
    }

    #[test]
    fn a_name_already_taken_is_refused_rather_than_disambiguated() {
        let (app, repository) = naming_project("naming-collision");
        repository.git(&["branch", "fix/taken"]);
        let checkout = placed(&app, repository.root());
        let before = branch_of(&checkout);

        let error = app
            .workspace()
            .name_task(&checkout, "fix/taken")
            .unwrap_err()
            .to_string();

        assert!(error.contains("already exists"), "{error}");
        assert_eq!(branch_of(&checkout), before);
    }

    /// A project that declares no vocabulary keeps exactly the behaviour it
    /// had before naming existed.
    #[test]
    fn a_project_that_names_nothing_refuses_and_says_why() {
        let repository = repository("naming-undeclared");
        let app = application("naming-undeclared");
        let checkout = placed(&app, repository.root());

        let error = app
            .workspace()
            .name_task(&checkout, "fix/branch-naming")
            .unwrap_err()
            .to_string();

        assert!(error.contains("agents.yaml"), "{error}");
        assert!(branch_of(&checkout).starts_with("agent/"));
    }

    /// The defect this fixes: with the branch renamed by hand, every later
    /// question was asked about a ref that no longer existed, and
    /// `commits_ahead` answered `0` — which reads as "nothing to deliver"
    /// rather than as "wrong branch".
    #[test]
    fn a_branch_renamed_by_hand_is_adopted_and_still_reaches_ready() {
        let (app, repository) = naming_project("naming-manual");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        std::fs::write(checkout.join("work.rs"), "fn work() {}").unwrap();
        repository.git_in(&checkout, &["add", "."]);
        repository.git_in(&checkout, &["commit", "-qm", "feat: work"]);
        repository.git_in(&checkout, &["branch", "--move", "feat/renamed-by-hand"]);

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        let task = evaluation.tasks.last().expect("a task was evaluated");
        assert_eq!(
            task.branch, "feat/renamed-by-hand",
            "the checkout's HEAD is the truth about the branch"
        );
        assert_eq!(task.label, "renamed by hand");
        assert_eq!(task.ahead, 1, "the commit is still counted");
        assert_eq!(
            task.state,
            TaskStateView::Ready,
            "a hand-renamed branch still reaches ready"
        );
    }
}

/// The automatic half: work that nobody named takes its name from its own
/// first commit, on the evaluation pass that already runs.
///
/// No harness is asked anything and no hook is delivered, which is why
/// this works on all four and on the next one.
#[cfg(test)]
mod derived_naming_tests {
    use super::*;
    use uze_core::UzeHome;

    fn project(label: &str, policy: &str) -> (UzeApplication, uze_testkit::git::Repository) {
        let repository = uze_testkit::git::Repository::new(label);
        repository.commit_file(".gitignore", ".env\n");
        std::fs::write(
            repository.root().join("agents.yaml"),
            format!("worktrees:\n{policy}"),
        )
        .unwrap();
        (
            UzeApplication::new(UzeHome::at(uze_testkit::temp::scratch(label)), Vec::new()),
            repository,
        )
    }

    fn placed(app: &UzeApplication, root: &Path) -> PathBuf {
        match app.workspace().place_new_agent(root, &[]).isolation {
            Isolation::Slot { .. } => {}
            Isolation::Unisolated { reason } => panic!("{reason}"),
        }
        app.workspace()
            .tasks(root)
            .last()
            .and_then(|task| task.checkout.clone())
            .expect("the placed agent has a checkout")
    }

    fn commits(repository: &uze_testkit::git::Repository, checkout: &Path, subject: &str) {
        std::fs::write(checkout.join("work.rs"), subject).unwrap();
        repository.git_in(checkout, &["add", "-A"]);
        repository.git_in(checkout, &["commit", "-qm", subject]);
    }

    #[test]
    fn the_first_commit_names_work_nobody_named() {
        let (app, repository) = project("derive-basic", "  branch: conventional\n");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        commits(&repository, &checkout, "feat(api): answer ping with pong");

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        let task = evaluation.tasks.last().unwrap();
        assert_eq!(task.branch, "feat/answer-ping-with-pong");
        assert_eq!(task.label, "answer ping with pong");
        assert_eq!(
            checkout::current_branch(&checkout).as_deref(),
            Some("feat/answer-ping-with-pong"),
            "Git is where the rename happened"
        );
        assert_eq!(
            task.state,
            TaskStateView::Ready,
            "and it is still deliverable"
        );
    }

    /// The agent's own name arrives earlier and therefore wins — the whole
    /// of the precedence rule, with no ladder to fall down.
    #[test]
    fn a_name_the_agent_chose_is_never_replaced_by_the_derivation() {
        let (app, repository) = project("derive-vs-chosen", "  branch: conventional\n");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        app.workspace()
            .name_task(&checkout, "fix/chosen-first")
            .unwrap();
        commits(&repository, &checkout, "feat(api): answer ping with pong");

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        assert_eq!(evaluation.tasks.last().unwrap().branch, "fix/chosen-first");
        assert_eq!(
            checkout::current_branch(&checkout).as_deref(),
            Some("fix/chosen-first")
        );
    }

    /// A derived name the project would have refused from an agent is not
    /// one UZE may write behind its back.
    #[test]
    fn a_commit_outside_the_vocabulary_leaves_the_generated_name() {
        let (app, repository) = project("derive-refused", "  branch: [ui, fix]\n");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        commits(&repository, &checkout, "feat(api): answer ping with pong");

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        assert!(
            evaluation
                .tasks
                .last()
                .unwrap()
                .branch
                .starts_with("agent/"),
            "a type this project does not accept names nothing"
        );
    }

    /// A project that declares no vocabulary keeps exactly the behaviour it
    /// had before any of this existed.
    #[test]
    fn a_project_that_names_nothing_is_left_alone() {
        let (app, repository) = project("derive-undeclared", "  completion: handoff\n");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        commits(&repository, &checkout, "feat(api): answer ping with pong");

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        assert!(
            evaluation
                .tasks
                .last()
                .unwrap()
                .branch
                .starts_with("agent/")
        );
    }

    /// Uncommitted work is not `Ready`, and naming a branch under an agent
    /// mid-edit is exactly what the `Ready` gate exists to avoid.
    #[test]
    fn a_dirty_checkout_is_not_named() {
        let (app, repository) = project("derive-dirty", "  branch: conventional\n");
        let root = repository.root().to_path_buf();
        let checkout = placed(&app, &root);
        commits(&repository, &checkout, "feat(api): answer ping with pong");
        std::fs::write(checkout.join("later.rs"), "in progress").unwrap();

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        let task = evaluation.tasks.last().unwrap();
        assert_eq!(task.state, TaskStateView::Uncommitted);
        assert!(task.branch.starts_with("agent/"));
    }

    /// A name already taken is left alone rather than disambiguated —
    /// silently, because nobody asked for this rename.
    #[test]
    fn a_colliding_derived_name_leaves_the_branch_as_it_was() {
        let (app, repository) = project("derive-collision", "  branch: conventional\n");
        let root = repository.root().to_path_buf();
        repository.git(&["branch", "feat/answer-ping-with-pong"]);
        let checkout = placed(&app, &root);
        commits(&repository, &checkout, "feat(api): answer ping with pong");

        let evaluation = app.workspace().evaluate_tasks(&root, &[checkout.clone()]);

        assert!(
            evaluation
                .tasks
                .last()
                .unwrap()
                .branch
                .starts_with("agent/")
        );
    }
}
