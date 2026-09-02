## ADDED Requirements

### Requirement: Every agent is isolated and the primary checkout belongs to the operator
The system SHALL start every agent it launches inside a Git working tree in
an isolated checkout of its own, created before the agent's process starts,
without asking the operator and without requiring any harness to cooperate.
The primary checkout SHALL never be assigned to an agent. Where isolation is
impossible the system SHALL start the agent in the directory it would
otherwise have used and SHALL state on the agent's tab that it is not
isolated.

#### Scenario: The first agent is isolated
- **WHEN** an operator creates an agent in a repository with no other agent running
- **THEN** it starts in an isolated checkout on its own branch, not in the primary checkout

#### Scenario: Concurrent agents never share a working tree
- **WHEN** three agents are live in one repository
- **THEN** each works in a distinct checkout and none of them is the primary

#### Scenario: The operator's uncommitted work is untouched by agents
- **WHEN** the operator has uncommitted changes in the primary checkout and agents run to completion
- **THEN** the primary checkout's working tree and index are exactly what the operator left

#### Scenario: A terminal that is not an agent runs where the operator is
- **WHEN** the operator opens a shell tab
- **THEN** it starts in the space's root and no checkout is created

#### Scenario: A harness the operator starts by hand is shown as unmanaged
- **WHEN** the operator runs a harness inside a shell tab
- **THEN** the tab is listed with the harness's name and its real directory, and carries no task, checkout, state or delivery action
- **AND** nothing is evaluated for it

#### Scenario: Isolation being impossible does not block the launch
- **WHEN** the space's root is not a Git working tree, or has no commit to branch from, or Git is absent
- **THEN** the agent still starts in that root
- **AND** the agent's tab states that it is not isolated

### Requirement: Isolated checkouts are reusable slots
The system SHALL keep isolated checkouts under the primary checkout's fixed
isolation directory, each named by a generated identifier that never
changes, and SHALL reuse a free checkout for a new agent before creating
another. Reuse SHALL put the working tree at the task's base with no tracked
or untracked file from the previous task, and SHALL preserve ignored files.
A checkout holding uncommitted changes, or commits absent from its base,
SHALL never be reused. The number of checkouts SHALL be bounded by peak
concurrency, and a project MAY declare a cap.

#### Scenario: A free checkout is reused
- **WHEN** a task ends with its checkout clean and a new agent is created
- **THEN** the new agent starts in that checkout, on a new branch from the base
- **AND** ignored files such as build caches remain in place

#### Scenario: A previous task's edits never reach the next
- **WHEN** a checkout whose last task was delivered is reused
- **THEN** the working tree matches the base and carries none of the previous branch's edits

#### Scenario: A checkout holding work is never reused
- **WHEN** a task's agent is gone and its checkout has uncommitted changes or commits absent from its base
- **THEN** the checkout is parked, listed to the operator, and not offered to a new agent

#### Scenario: A new checkout is created only when none is free
- **WHEN** every existing checkout is occupied or parked
- **THEN** a new checkout is created

#### Scenario: A declared cap holds
- **WHEN** a project declares a maximum number of checkouts and all are occupied
- **THEN** no new agent is started until one is free, and the operator is told why

### Requirement: Nothing that can hold work is removed automatically
The system SHALL NOT remove a working tree holding uncommitted changes, nor
a branch holding commits absent from its target, on any automatic path. It
MAY remove a branch whose work is in the target — every commit reachable
from the target, or, for a task delivered as a pull request, the request
reported merged by the forge — and MAY remove the directory of a clean
checkout idle beyond a declared age while keeping its branch. Discarding work SHALL happen only on an explicit operator
action naming the task.

#### Scenario: A dirty orphan is parked, not deleted
- **WHEN** startup finds a checkout with uncommitted changes and no live agent
- **THEN** the checkout is parked and every file in it is preserved

#### Scenario: An unintegrated branch outlives its checkout
- **WHEN** a clean checkout is idle beyond the declared age and its branch has commits absent from the target
- **THEN** the directory may be removed and the branch remains

#### Scenario: An integrated branch is pruned
- **WHEN** every commit of a task's branch is reachable from the target
- **THEN** the branch may be removed without an operator action

#### Scenario: A squash-merged pull request still counts as integrated
- **WHEN** a task delivered as a pull request is merged by squashing, so none of its commits is reachable from the target
- **THEN** the task is reported integrated on the forge's evidence and its branch may be pruned

#### Scenario: Only the operator discards
- **WHEN** a parked task is discarded
- **THEN** the action was taken by the operator on that task, and no automatic path could have taken it

### Requirement: A task's identity is immutable and its name is derived
The system SHALL give every agent launch a generated task identifier that
never changes, and SHALL key the checkout, the branch and the task's
persisted state on it. The visible label SHALL be derived from the initial
prompt when there is one, and from the identifier otherwise. The branch
SHALL be named from the identifier under the `agent/` prefix while the work
stays local; a readable name derived from the label SHALL be produced only
when the branch is first published. Task state SHALL be persisted outside
every checkout and written atomically.

#### Scenario: The label comes from the prompt
- **WHEN** an agent is created with an initial prompt
- **THEN** its tab carries a label derived from that prompt's first line, and its branch carries the identifier

#### Scenario: A published branch carries a readable name
- **WHEN** a task is delivered by opening a pull request
- **THEN** the pushed branch is named from the task's label
- **AND** the task's identifier, checkout and state are unchanged

#### Scenario: State survives the checkout
- **WHEN** a task's checkout directory is removed
- **THEN** the task's state and transcript are still available

#### Scenario: An interrupted write leaves a valid state file
- **WHEN** the process is killed while task state is being written
- **THEN** the state file on disk is either the previous version or the new one, never truncated

### Requirement: Readiness is observed, never declared
The system SHALL decide whether a task is ready to deliver from the state of
its checkout: ready when its branch has commits absent from the base and the
working tree is clean. It SHALL evaluate that state when the agent's pane
goes quiet and whenever the operator asks, and SHALL NOT rely on the agent
announcing completion. An end-of-turn signal from a harness, where one is
delivered, MAY trigger an evaluation but SHALL NOT be required.

#### Scenario: Commits and a clean tree read as ready
- **WHEN** the agent's pane goes quiet with commits on the task's branch and a clean working tree
- **THEN** the task is reported ready and delivery is offered

#### Scenario: Uncommitted changes are surfaced, not delivered
- **WHEN** the agent's pane goes quiet with uncommitted changes in the task's checkout
- **THEN** the tab reports uncommitted work and delivery is not offered

#### Scenario: A quiet pane that resumes is not stuck as ready
- **WHEN** a task was reported ready and its agent produces further changes
- **THEN** the next evaluation reflects the checkout's current state

### Requirement: Delivery follows the declared completion and only the system writes the target
The system SHALL deliver a ready task only on an explicit operator action,
one task at a time, according to the project's declared completion
behavior. `handoff` SHALL leave the branch for the operator. `merge` SHALL
rebase the task's branch onto the target's tip inside the task's checkout,
run the project's declared gate on the rebased commits, and advance the
target by fast-forward only. `pr` SHALL publish the branch under its
readable name and open a pull request against the target. No agent SHALL
write the target branch; the system SHALL write it only in the fast-forward
step. The target's tip SHALL be taken from where the target lives: the
remote-tracking branch after a fetch when delivery publishes a pull
request, the local branch otherwise. The system SHALL NOT update the
operator's checked-out target except in the fast-forward step of `merge`.

#### Scenario: Handoff never touches the target
- **WHEN** the operator delivers a ready task in a project declaring handoff
- **THEN** the branch is reported as ready for the operator and the target is unchanged

#### Scenario: Merge advances the target linearly after the gate
- **WHEN** the operator delivers a ready task in a project declaring merge
- **THEN** the branch is rebased onto the target, the gate passes on the rebased commits, and the target moves to the branch's tip
- **AND** the target's history is linear

#### Scenario: A gate failure leaves the target untouched
- **WHEN** the gate fails on the rebased commits
- **THEN** the task is returned to its agent with the gate's output and the target is unchanged

#### Scenario: A conflict is returned to the agent that owns the task
- **WHEN** the rebase stops on conflicts
- **THEN** the rebase stays paused in the task's checkout, the agent is told which files conflict and how far the target moved, and the target is unchanged
- **AND** the next evaluation reads the task's state from its checkout

#### Scenario: The second task sees the first
- **WHEN** two ready tasks are delivered in sequence
- **THEN** the second is rebased onto a target that already contains the first

#### Scenario: Overlapping uncommitted work in the primary refuses delivery
- **WHEN** the operator has uncommitted changes in the primary checkout to files the task changed
- **THEN** delivery in merge mode is refused and reported, and nothing is written

#### Scenario: In pr mode the target is the remote's
- **WHEN** a task is rebased or delivered in a project declaring pr and the remote target has moved
- **THEN** the branch is rebased onto the remote target's tip as fetched
- **AND** the operator's local target branch and primary checkout are not modified

#### Scenario: Sibling tasks share work only through the target
- **WHEN** one task has been delivered and another live task asks for the target
- **THEN** the second task receives the first's work by rebasing onto the target
- **AND** no task's branch ever carries another task's commits directly

#### Scenario: A live task follows the target automatically
- **WHEN** the target has moved and a live task's pane goes quiet with a clean working tree
- **THEN** the task's branch is rebased onto the target's tip inside its checkout, under the same rules as delivery
- **AND** a conflict is returned to the agent and the target is unchanged

#### Scenario: A task mid-edit is not rebased under its agent
- **WHEN** the target has moved while a live task's working tree is dirty
- **THEN** the task is left as it is until its tree is clean and its pane quiet

#### Scenario: A pull request targets the declared branch
- **WHEN** the operator delivers a ready task in a project declaring pr
- **THEN** the branch is pushed under its readable name and a pull request against the target is opened

### Requirement: Existing checkouts are adopted at startup
The system SHALL reconcile the isolation directory, the `agent/` branches
and its persisted task state when a repository's space starts. A checkout
without a task SHALL be adopted: parked when it holds work, free otherwise.
A task without a checkout SHALL be marked from where its branch stands.
Stale worktree registry entries SHALL be pruned only after reconciliation.

#### Scenario: A legacy checkout is adopted
- **WHEN** the isolation directory holds checkouts created before task state existed
- **THEN** each is adopted as a task labelled from its branch, and no branch is renamed

#### Scenario: Prune never runs before adoption
- **WHEN** startup finds a registry entry whose directory is gone
- **THEN** the entry is pruned only after every remaining checkout has been adopted

### Requirement: The declaration is projected without triggering foreign isolation
The system SHALL render a project's declaration into a marker-owned managed
region of the shared instruction file, stating the isolation layout, that a
reader inside an isolated checkout is already isolated, that finished work
is committed on the reader's own branch and never on the target, that
delivery is performed by the system, and how to isolate a subagent against
the primary checkout. The rendering SHALL be deterministic, and SHALL NOT
instruct any reader to create a top-level worktree.

#### Scenario: Reconciling a declaration projects it
- **WHEN** the operator reconciles project context for a project that declares an isolation policy
- **THEN** the shared instruction file carries the declaration in a managed region
- **AND** a second reconciliation changes no bytes

#### Scenario: A project declaring nothing carries no region
- **WHEN** the operator reconciles project context for a project that declares no isolation policy
- **THEN** no region is written and none is reported

#### Scenario: The projection does not activate a harness's own worktree mechanism
- **WHEN** the projected text is read by a harness that creates worktrees when instructed to
- **THEN** it finds no instruction to create a top-level worktree
- **AND** it is told it is already isolated

#### Scenario: The projected text keeps the target for the system
- **WHEN** a project declares any completion behavior
- **THEN** the projected text states that behavior, that the reader commits on its own branch, and that the target is written by the system

### Requirement: A declaration stays editable
The system SHALL key the projected region's identity on the rendered
content, so changing the declaration supersedes one region and creates
another rather than drifting the existing one. A region edited by hand SHALL
be reported as drifted and left untouched.

#### Scenario: Editing the declaration replaces its region
- **WHEN** a project's declaration changes and context is reconciled
- **THEN** the superseded region is removed and the new one attached
- **AND** exactly one region remains

#### Scenario: An edited region is refused, not rewritten
- **WHEN** the region's content has been changed by hand
- **THEN** reconciliation reports drift and leaves the file's bytes untouched
