## Purpose

Lets an operator launch an agent without deciding anything first, and
isolate that one agent into a checkout of its own at the moment isolation
turns out to be what the work needs.

## ADDED Requirements

### Requirement: A space has one kind, and any directory can be one
The system SHALL give a space no kind to choose. A space is a directory
the operator opened, and every directory MAY be one — a Git repository or
not. One directory SHALL be the root of at most one space: opening a
directory that already has a space SHALL select it rather than create a
second.

#### Scenario: Creating a space asks nothing but where
- **WHEN** the operator creates a space
- **THEN** they choose a directory and nothing else, and the space exists

#### Scenario: A directory that is not a repository is a space like any other
- **WHEN** the operator creates a space over a directory that is not a Git working tree
- **THEN** the space is created, and agents can be launched in it

#### Scenario: One root, one space
- **WHEN** a space exists over a root and the operator picks the same root again
- **THEN** the existing space is selected and none is created

### Requirement: An agent starts in the space's own directory
The system SHALL start every agent in the space's root, on whatever branch
that directory is on, without creating a checkout or a branch, and SHALL
record it with its identity, the harness it runs and when it started. An
agent that is not isolated SHALL have no branch of its own, no delivery and
no preserved work; its conversation SHALL be recorded and resumed as any
agent's is. Several agents MAY share one root.

Where the work in its checkout stands SHALL be read for it as for any
other agent — the checkout's answer, which every agent sharing that
checkout reads alike. Only *delivering* it is withheld, because the branch
is the operator's and UZE did not cut it.

#### Scenario: Launching an agent creates nothing on disk
- **WHEN** an agent is launched in a space
- **THEN** no checkout and no branch are created, and the agent runs in the space's root

#### Scenario: The operator's branch is never changed under them
- **WHEN** an agent runs in the space's root
- **THEN** UZE never switches or resets the branch that directory is on

### Requirement: Isolation is an action on one agent
The system SHALL offer, for a single agent, an action that gives it a
checkout of its own: a slot is acquired as the project's policy says, the
agent's record gains its isolation — the checkout, the branch and what the
branch was cut from — and the agent is relaunched there. The agent's
identity SHALL NOT change, and its conversation record SHALL stay that
agent's.

Whether the *harness's* conversation follows is the harness's answer, not
UZE's: one that binds a conversation to the directory it ran in cannot
resume it in the new checkout and starts a fresh one there. UZE SHALL
resume where the harness can and start a new conversation where it
cannot, never refusing the isolation over it.

The action SHALL be offered only where it can be honoured: inside a Git
working tree with a commit to branch from. Where a slot cannot be
acquired, the agent SHALL be left exactly where it is and the reason
stated.

#### Scenario: An agent is isolated
- **WHEN** the operator isolates an agent running in the space's root
- **THEN** a checkout is created for it, its record carries that isolation, and it is relaunched there
- **AND THEN** it is the same agent: the same identity, and the same conversation record

#### Scenario: A harness that binds its conversation to a directory
- **WHEN** an agent whose harness stores its conversation under the directory it ran in is isolated
- **THEN** the agent is relaunched in the checkout with a conversation of its own, and the isolation is not refused

#### Scenario: Isolation is not offered where it cannot be honoured
- **WHEN** the space's root is not a Git working tree, or has no commit to branch from
- **THEN** the action is not offered

#### Scenario: A slot cannot be acquired
- **WHEN** isolating an agent and the checkout cap is reached, or Git refuses
- **THEN** the agent keeps running where it was, and the operator is told why

Isolating SHALL take a copy of the root's uncommitted changes into the
checkout. At the moment an agent is moved, what the tree holds is
usually what that agent was doing, and a checkout without it is one
where the file it was mid-edit on has gone back to its last commit.

Cutting from the last commit instead SHALL be offered as a second
answer, and only where the tree is known to hold uncommitted work. The
knowledge is the evaluation's, never a Git read taken while the surface
draws, so it MAY be up to a refresh old — which is why it gates this
answer and not the carrying one: a stale *clean* reading costs the
operator nothing, where the same staleness on the carrying answer would
take away the very thing they had just edited. Neither answer takes
anything from the operator's own tree.

#### Scenario: The work follows the agent
- **WHEN** an agent is isolated while the space's root has uncommitted changes
- **THEN** a copy of them is in the checkout — edits to tracked files and new files the repository does not ignore alike
- **AND THEN** the root's own working tree is left exactly as it was, whichever answer is given
- **AND THEN** nothing is discarded

#### Scenario: A clean tree asks nothing
- **WHEN** an agent is isolated while the space's root has no uncommitted changes
- **THEN** the action carries the one answer, and neither tree gains or loses anything

#### Scenario: The work is not this agent's to take
- **WHEN** the root is known to hold uncommitted changes and the operator cuts from the last commit
- **THEN** the checkout holds the commit alone, and the changes stay in the root they were made in

### Requirement: An isolated agent is the subject of delivery
The system SHALL treat an isolated agent exactly as a task is treated
today: its work is delivered by the project's completion behaviour, it can
be named, and its checkout is a slot that is reused when the agent ends.
An agent that is not isolated SHALL be none of those things.

Readiness is not among them. Where the work in a checkout stands is read
for every agent, isolated or not — what an unisolated one lacks is a
branch UZE cut, and therefore a delivery UZE may run.

#### Scenario: Delivery is offered for an isolated agent
- **WHEN** an isolated agent has commits its target does not have
- **THEN** the same delivery the project's policy describes is offered for it

#### Scenario: An agent in the root is never delivered
- **WHEN** an agent that is not isolated has been working
- **THEN** no delivery is offered for it: its commits are already on the operator's branch

### Requirement: An agent ends when no live pane carries it
The system SHALL record an agent as ended when no live pane carries its
identity, and SHALL keep the record. An isolated agent's slot SHALL be
released by the same sweep that releases a slot today; an agent that is
not isolated SHALL leave nothing to release.

#### Scenario: The last pane of an agent closes
- **WHEN** the last pane carrying an agent's identity is closed
- **THEN** the agent is recorded as ended and its row leaves the column

#### Scenario: An isolated agent that ended holding work
- **WHEN** an isolated agent ends with commits its target does not have
- **THEN** its slot is not handed to the next agent, and the work stays reachable

### Requirement: The column groups an agent by where it works
The sidebar SHALL draw a space's agents in two groups: the agents working
in the space's root first, the isolated agents after them, each group in
its own order. A blank row SHALL separate the groups, and only when both
groups have an agent in them. An isolated agent's row SHALL carry the
connector that says it branches off the project; an agent in the root
SHALL not. The two SHALL be drawn in different colours, so which group a
row belongs to is answered without reading it.

Isolating an agent SHALL move its row from the first group to the second,
which is how the operator sees that the action happened.

#### Scenario: A space with both kinds of agent
- **WHEN** a space has agents in its root and isolated agents
- **THEN** the root's agents are drawn first, then a blank row, then the isolated ones

#### Scenario: A space with one kind of agent
- **WHEN** every agent of a space is in the same group
- **THEN** no blank row is drawn

#### Scenario: The row moves when the agent is isolated
- **WHEN** an agent is isolated
- **THEN** its row leaves the first group and appears in the second

### Requirement: A project may declare that its agents start isolated
The system SHALL let a project declare, in its manifest, that an agent
launched in it starts isolated rather than in the root. Where it does, an
agent SHALL be placed in a slot at launch, and the isolation action SHALL
have nothing to offer. Where the manifest declares nothing, an agent
SHALL start in the root.

#### Scenario: A project that isolates by default
- **WHEN** a project's manifest declares that agents start isolated and an agent is launched
- **THEN** the agent is placed in a checkout of its own at launch

#### Scenario: A project that declares nothing
- **WHEN** a project's manifest declares no default and an agent is launched
- **THEN** the agent starts in the space's root

### Requirement: A space can be created from the keyboard
The system SHALL bind the action that creates a space to a default chord,
so a space can be created without the pointer.

#### Scenario: The chord creates a space
- **WHEN** the operator presses the chord bound to creating a space
- **THEN** the prompt that chooses a directory opens
