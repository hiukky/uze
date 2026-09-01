## ADDED Requirements

### Requirement: Concurrent agents never share a checkout
The system SHALL seat one agent at a time in a project's primary checkout and
SHALL start every additional live agent in an isolated checkout of its own,
without asking the operator and without requiring any harness to cooperate.
Occupancy SHALL be judged by which checkout an agent's pane is in, not by an
exact path. Where isolation is impossible the system SHALL start the agent in
the primary checkout rather than refuse to start it.

#### Scenario: The first agent takes the seat
- **WHEN** an operator creates an agent in a repository with no other agent running
- **THEN** it starts in the primary checkout, with the operator's uncommitted work visible

#### Scenario: A second concurrent agent is isolated
- **WHEN** an operator creates a second agent while the first is still live in the primary checkout
- **THEN** the second starts in an isolated checkout of its own, on its own branch
- **AND** neither agent can write to the other's files

#### Scenario: An agent moving inside the repository keeps its seat
- **WHEN** the agent holding the seat changes directory within the primary checkout
- **THEN** the seat remains occupied

#### Scenario: An isolated agent does not hold the seat
- **WHEN** the only live agent is working in an isolated checkout
- **THEN** the seat reads as free and the next agent starts in the primary checkout

#### Scenario: A terminal that is not an agent never takes the seat
- **WHEN** the operator opens a shell in the primary checkout
- **THEN** the seat remains free for an agent

#### Scenario: Isolation being impossible does not block the launch
- **WHEN** the project has no commit to branch from, or is not a Git working tree
- **THEN** the agent still starts, in the directory it would otherwise have used

### Requirement: Isolation never destroys or silently reuses
The system SHALL create an isolated checkout under the primary checkout's
fixed isolation directory, branching from the primary's current `HEAD`. It
SHALL prune stale worktree registry entries before creating, SHALL suffix a
name already taken by a directory or branch rather than reusing it, and SHALL
ensure the isolation directory is ignored by the repository. It SHALL NOT
remove any checkout.

#### Scenario: A kept checkout's name recurs
- **WHEN** an isolated checkout already exists for a name and a new agent claims the same one
- **THEN** a suffixed name is used and the existing checkout and branch are untouched

#### Scenario: The seat's commits never swallow another agent's checkout
- **WHEN** the first isolated checkout is created
- **THEN** the isolation directory is ignored by the repository, idempotently and preserving existing entries
- **AND** the primary checkout's status does not report the isolated checkout

#### Scenario: A checkout removed outside the system does not block creation
- **WHEN** an isolated checkout's directory was deleted but its registry entry remains
- **THEN** creating a new checkout prunes the stale entry and succeeds

### Requirement: One repository is one runtime
The system SHALL key its terminal runtime on the resolved workspace root
rather than on the directory it was launched from, so a repository has one
server, one set of agent panes, and one seat.

#### Scenario: Launching from a subdirectory reaches the same runtime
- **WHEN** the system is launched from a repository and from a subdirectory of it
- **THEN** both resolve to the same workspace identity

### Requirement: The declaration is projected without triggering foreign isolation
The system SHALL render a project's declaration into a marker-owned managed
region of the shared instruction file, stating the isolation layout, that a
reader inside an isolated checkout is already isolated, how to isolate a
subagent against the primary checkout, and the completion rule. The rendering
SHALL be deterministic, and SHALL NOT instruct any reader to create a
top-level worktree.

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

#### Scenario: The declared completion behavior is what reaches the baseline
- **WHEN** a project declares a completion behavior
- **THEN** the projected text states that behavior and no other

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
