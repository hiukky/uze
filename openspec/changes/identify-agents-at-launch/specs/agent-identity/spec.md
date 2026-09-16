## Purpose

Identifies an agent UZE launched for the life of its process, so that every
reader — the launch boundary, the agent's own commands, the workspace
client — answers "which agent is this?" from what the launch carried
rather than from the directory the process stands in.

## ADDED Requirements

### Requirement: A launch carries the agent's identity
The system SHALL give every agent it launches an identifier before the
agent's process starts, SHALL record that identifier in the project's
state, and SHALL place it in the environment of the pane the agent runs
in. Every process descending from that launch SHALL inherit it. A pane the
system opens for anything other than an agent it launched — a shell, a
tab opened beside an agent — SHALL carry no identity, whatever the
environment of the process that spawned it.

#### Scenario: The identity is present in the agent's own environment
- **WHEN** an agent UZE launched starts a child process
- **THEN** that child can read the agent's identifier from its environment

#### Scenario: A shell beside an agent carries no identity
- **WHEN** the operator opens a shell tab beside a running agent
- **THEN** no agent identifier is present in that shell's environment

#### Scenario: A pane never inherits an identity from the server
- **WHEN** the terminal server was itself started from inside an agent's pane and later spawns a plain shell
- **THEN** the shell carries no agent identifier

### Requirement: Identity survives what the launch survives
The identifier SHALL be persisted with the tab it was launched into, SHALL
be applied again when the terminal runtime respawns that tab after a
restart, and SHALL be reported back to clients as part of the tab, so a
client learns which agent a pane belongs to from the session it is handed
and never by inspecting the process. The reported identity says what the
tab was launched for; whether that process still runs is reported
separately, as it is today. A tab respawned as a plain shell SHALL report
no identity.

#### Scenario: A restart keeps the identity
- **WHEN** the terminal runtime restores an agent's tab after its server was restarted
- **THEN** the respawned process carries the same identifier the original launch did

#### Scenario: The client reads the identity from the session
- **WHEN** a workspace client attaches while an agent is running
- **THEN** the session names that agent's identifier on its tab, without the client probing the process

#### Scenario: A finished agent's tab reports no identity
- **WHEN** an agent has exited and its tab is respawned as a plain shell
- **THEN** the tab reports no agent identifier

### Requirement: An identity is verified before it is acted on
A reader SHALL act on an identifier only when both of these hold: the
project's records name an agent with that identifier, and the directory
the process stands in is inside the directory that agent's record names
as its own. An identifier failing either check SHALL be treated as
absent, so a process that alters its own environment cannot reach an
agent whose directory it does not stand in. Where two records name the
same directory, the identifier alone tells them apart, and a process
there that alters its identifier is trusted as the agent it names; the
system SHALL therefore never grant, by identity alone, an action that
reaches beyond the agent's own directory — a branch is named only for an
agent whose record gives it a directory of its own.

#### Scenario: Two records over one directory are told apart by the identifier alone
- **WHEN** two agents are recorded over the same directory and a process there alters its identifier to the other's
- **THEN** it is treated as the other agent, and nothing it can do by that identity reaches outside the shared directory

#### Scenario: An identifier no record names is ignored
- **WHEN** a process carries an identifier the project's records do not contain
- **THEN** every reader behaves as if no identifier were present

#### Scenario: An identifier from the wrong directory is ignored
- **WHEN** a process carries an isolated agent's identifier but stands outside that agent's checkout
- **THEN** every reader behaves as if no identifier were present

#### Scenario: Two agents in one directory are told apart
- **WHEN** two agents are running in the same directory, each launched with its own identifier
- **THEN** each reader resolves each process to its own agent

### Requirement: A launch made by an agent is not an agent
The system SHALL treat a harness process started from inside an agent's
pane by that agent or by a person typing in it as an ordinary invocation:
it SHALL NOT resume the enclosing agent's conversation and SHALL NOT be
bound to the enclosing agent's record, even though it inherits the
identifier.

#### Scenario: A harness started inside a harness
- **WHEN** an agent's harness starts another harness process in its pane
- **THEN** the inner process starts an ordinary conversation and the enclosing agent's conversation is untouched
