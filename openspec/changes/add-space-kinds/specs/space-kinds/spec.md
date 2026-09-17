## Purpose

Lets an operator choose, per space, whether the agents in it work in
isolated checkouts of their own or directly in the space's directory on the
branch it is on, and gives each kind its own placement, its own record and
its own presentation.

## ADDED Requirements

### Requirement: A space has a kind chosen at creation
The system SHALL give every space one of two kinds when it is created, a
*worktree* space or a *workspace* space, SHALL keep the kind for the life
of the space, and SHALL treat the kind as part of the space's identity, so
one directory may be the root of a worktree space and of a workspace space
at the same time. The worktree kind SHALL be refused, with the reason, for
a root that is not a Git working tree with a commit; any root MAY be a
workspace space.

#### Scenario: Creating a space asks for its kind
- **WHEN** the operator creates a space over a Git repository
- **THEN** they choose between the worktree and the workspace kind before the space exists, with the worktree kind selected by default

#### Scenario: A directory that is not a repository is a workspace space
- **WHEN** the operator creates a space over a directory that is not a Git working tree
- **THEN** the worktree kind is refused with the reason and only a workspace space can be created there

#### Scenario: One root, two spaces
- **WHEN** a worktree space exists over a root and the operator creates a workspace space over the same root
- **THEN** a second space is created rather than the first reopened

#### Scenario: The kind survives a restart
- **WHEN** the terminal runtime restores its spaces after a restart
- **THEN** each space comes back with the kind it was created with

### Requirement: An agent of a worktree space is placed in a slot, or not at all
The system SHALL place every agent created in a worktree space in an
isolated checkout of its own, exactly as the `worktree-policy` capability
specifies, and SHALL refuse to start the agent when a slot cannot be
acquired — the manifest cannot be read, the primary checkout is on no
branch or has no commit, the checkout cap is reached, the record cannot be
written, or Git refuses — stating the reason. It SHALL NOT start an agent
of a worktree space in the space's own directory.

#### Scenario: A slot cannot be acquired
- **WHEN** an agent is created in a worktree space whose checkout cap is already reached
- **THEN** no agent starts and the operator is told the cap is the reason

#### Scenario: The operator's tree is never a fallback
- **WHEN** placing an agent of a worktree space fails for any reason
- **THEN** no process is started in the space's root

### Requirement: An agent of a workspace space is a tenant of the space's directory
The system SHALL start every agent created in a workspace space in the
space's root, on whatever branch that directory is on, without creating a
checkout or a branch, and SHALL record it as a tenant: an identity, the
harness it runs, the root it works in, and when it started and ended. A
tenant SHALL have no branch, no readiness, no delivery, no preserved work
and no name of its own; its conversation SHALL be recorded and resumed as
any agent's is. Several tenants MAY share one root, and the root MAY be a
directory that is not a repository.

#### Scenario: A tenant starts where the operator is
- **WHEN** an agent is created in a workspace space over a repository on branch `main`
- **THEN** its process starts in the space's root on `main`
- **AND** no directory is created under `.worktrees` and no `agent/` branch exists

#### Scenario: A tenant's commit lands on the branch
- **WHEN** a tenant commits in the space's root
- **THEN** the commit is on the branch the root was on, with nothing for the system to deliver

#### Scenario: Two tenants share the tree
- **WHEN** two agents are created in the same workspace space
- **THEN** both run in the same root and each is recorded and resumed as its own agent

#### Scenario: A tenant outside any repository
- **WHEN** an agent is created in a workspace space over a directory that is not a Git working tree
- **THEN** it starts there, is recorded as a tenant, and is ended and resumed exactly as a tenant of a repository is

#### Scenario: A tenant is never offered delivery or naming
- **WHEN** a tenant is listed
- **THEN** it carries no delivery action, no task mark, no preserved-work entry, and the naming command refuses it as work that is not named

#### Scenario: A tenant comes back after a restart
- **WHEN** the terminal runtime restores a tenant's tab after a restart
- **THEN** the tenant resumes its recorded conversation

### Requirement: A tenant ends when no live pane carries it
The system SHALL mark a tenant ended when the space's panes are reconciled
and none of them carries the tenant's identity, and SHALL never end a
tenant while a live pane carries it. Reconciliation SHALL reach every
space's root, whether or not it is a repository. An ended tenant SHALL
disappear from the space's listing and SHALL NOT be listed as preserved
work.

#### Scenario: Closing the tab ends the tenant
- **WHEN** a tenant's tab is closed and the space's panes are reconciled
- **THEN** the tenant is recorded as ended and no longer listed

#### Scenario: A live tenant is never ended
- **WHEN** panes are reconciled while a tenant's pane is still running
- **THEN** the tenant stays live

#### Scenario: A tenant of a plain directory ends too
- **WHEN** a tenant of a workspace space over a directory that is not a repository has its tab closed and panes are reconciled
- **THEN** the tenant is recorded as ended

### Requirement: Both kinds share one look, shaped differently
The workspace client SHALL draw every agent, in either kind, as the same
two-row item: its status glyph and label, and beneath them the branch it
works on or, outside a repository, its directory; the space's header row
SHALL be drawn lighter than the block of agents beneath it, and the
header's root toggle SHALL swap each agent's caption for the id of the
harness running it in both kinds. A worktree space SHALL hang its items on
a tree, each on a branch of its own. A workspace space SHALL draw its items
flat, with no tree connectors, and SHALL mark the selected agent with a
vertical accent bar down both of its rows. Selection, keyboard movement
between agents, pointer hits and scrolling SHALL behave identically in both
kinds.

#### Scenario: A workspace space lists agents flat
- **WHEN** a workspace space with two agents is drawn
- **THEN** each agent occupies a label row and a caption row naming the root's branch, no tree connector is drawn, and the header does not repeat the branch

#### Scenario: A tenant outside a repository
- **WHEN** a workspace space over a directory that is not a repository is drawn
- **THEN** each agent's caption row names that directory

#### Scenario: Tenants carry the agent status
- **WHEN** a workspace space holds an idle agent and the selected agent
- **THEN** each wears the same status glyph a worktree agent in that state wears

#### Scenario: The selected tenant carries the bar
- **WHEN** an agent of a workspace space is selected
- **THEN** both of its rows carry the accent bar, and no other row in the sidebar carries a bar

#### Scenario: An empty workspace space
- **WHEN** a workspace space has no agent
- **THEN** it draws one caption row with its root, as an empty worktree space does

#### Scenario: A flat list scrolls by its rows
- **WHEN** a workspace space holds more agents than the column shows
- **THEN** the tree scrolls by the same rows a worktree space's items take and the foot sections keep their place

#### Scenario: A tenant whose harness exited is no longer an agent row
- **WHEN** a tenant's harness exits leaving a shell in its pane
- **THEN** its row is no longer drawn as an agent, as in a worktree space

#### Scenario: Navigation is the same in both kinds
- **WHEN** the operator moves to the next agent with the keyboard or clicks an agent row in a workspace space
- **THEN** that agent is selected exactly as it would be in a worktree space

### Requirement: A space can be created from the keyboard
The workspace client SHALL bind the `new-space` action to a default chord
and SHALL open the same space creation the pointer reaches when the chord
is pressed.

#### Scenario: The chord opens creation
- **WHEN** the operator presses the `new-space` chord in the workspace
- **THEN** the space picker opens, exactly as clicking the new-space control does
