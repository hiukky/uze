# agent-session-continuity Specification

## Purpose

Keeps the conversation of an agent UZE launched attached to the task it
belongs to, so relaunching that agent — after a restart, a crash, or the
operator picking a preserved task back up — continues the work instead of
starting a stranger in the same checkout.

## Requirements

### Requirement: A managed agent's conversation is recorded against its task
The system SHALL record, for every agent it launches into a task, the
identifier of the conversation that agent's harness started, together with
the harness it belongs to. The record SHALL live in UZE's own state beside
the task, never inside any checkout, and SHALL be keyed by task so that it
survives the checkout being reset, moved or removed. The system SHALL NOT
record any harness credential or conversation content.

#### Scenario: A new agent's conversation is recorded
- **WHEN** an agent is launched into a task for the first time
- **THEN** the conversation the harness started is recorded against that task and that harness

#### Scenario: The record survives the checkout
- **WHEN** a task's checkout is reset, or removed and given back to the task later
- **THEN** the recorded conversation is still the one the task started with

#### Scenario: A task carries one conversation per harness
- **WHEN** a task has been worked in by two different harnesses
- **THEN** each harness's conversation is recorded separately and neither is offered to the other

### Requirement: A conversation is keyed by identity, and the record follows the agent
The system SHALL key a conversation by the identifier its harness resumes
by, never by a title, label or name a person can change. Where a harness
moves an agent into a different conversation while it runs — cleared,
forked, or switched by the person at the keyboard — the system SHALL keep
the record pointing at the conversation the agent is actually in, so what
resumes is where the work was left, not where it began.

#### Scenario: Renaming a conversation changes nothing
- **WHEN** a person renames, retitles or relabels a conversation inside the harness
- **THEN** the recorded conversation still resolves and the agent resumes it

#### Scenario: The agent is moved to another conversation while it runs
- **WHEN** the person clears, forks or switches the conversation inside a running agent, and the agent is later relaunched
- **THEN** it resumes the conversation it was in when it stopped, not the one it started in

### Requirement: Relaunching an agent continues its task's conversation
The system SHALL start an agent whose task already has a conversation for
that harness by resuming that conversation, and SHALL start a fresh one
only when the task has none. This SHALL hold however the agent is
relaunched — by the operator from the interface, or by the terminal
runtime restoring a workspace after a restart, with no client present.

#### Scenario: The workspace comes back after a restart
- **WHEN** the terminal runtime restores an agent tab after its server was restarted
- **THEN** the agent resumes the conversation its task was left in, not a new one

#### Scenario: A preserved task is picked up again
- **WHEN** the operator resumes a preserved task and chooses a harness that already worked in it
- **THEN** the agent starts in that task's conversation

#### Scenario: A task with no conversation for that harness starts one
- **WHEN** an agent is launched into a task the chosen harness has never worked in
- **THEN** a fresh conversation starts and is recorded against the task

### Requirement: A reused checkout never inherits another task's conversation
The system SHALL bind a conversation to the task rather than to the
directory it ran in. An agent launched into a task SHALL never be given a
conversation belonging to a different task, including a task that
previously occupied the same checkout.

#### Scenario: A recycled slot starts clean
- **WHEN** a checkout that held a finished task is reused for a new task and an agent is launched into it
- **THEN** the agent starts a fresh conversation and never sees the previous task's

### Requirement: The operator's own invocation is never rewritten
The system SHALL leave an invocation the operator composed exactly as
typed. Continuity SHALL apply only where the operator asked for nothing
about sessions themselves: an invocation carrying the harness's own session
or resume arguments SHALL be passed through untouched, and an invocation
made outside a managed task SHALL behave exactly as it did before this
capability existed.

#### Scenario: An explicit session argument wins
- **WHEN** a harness is started inside a managed task with its own resume or session argument
- **THEN** that argument decides the conversation and UZE adds nothing

#### Scenario: Outside a managed task nothing changes
- **WHEN** a harness is started in a directory that belongs to no managed task
- **THEN** it starts exactly as it would without UZE, with no conversation carried over

### Requirement: Continuity is declared by the harness, never assumed
Each harness integration SHALL declare how a conversation may be carried
over for it: named by UZE at launch, named by the harness and read back
afterwards, or not available. The system SHALL launch an agent for a
harness that declares none as a fresh conversation, and SHALL state on that
agent that its conversation is not carried over. The system SHALL NOT
present a conversation as continued when it was not.

#### Scenario: A harness without a mechanism is honest about it
- **WHEN** an agent is relaunched for a harness that declares no continuity
- **THEN** it starts fresh and the agent's tab states that the conversation was not carried over

#### Scenario: A harness that names its own conversation is still recorded
- **WHEN** an agent runs for a harness that does not let UZE name the conversation
- **THEN** the identifier the harness chose is read back from that harness's own records and recorded against the task

### Requirement: A conversation that cannot be resumed never blocks the launch
The system SHALL start the agent whatever continuity fails: a recorded
conversation the harness no longer holds, a harness version that does not
accept the resume argument, or unreadable state SHALL leave the agent
starting fresh, with the reason stated once. Continuity SHALL never be a
precondition for an agent starting.

#### Scenario: The recorded conversation is gone
- **WHEN** an agent is relaunched into a task whose recorded conversation no longer exists in the harness's records
- **THEN** the agent starts fresh and the reason is stated once

#### Scenario: Unreadable continuity state
- **WHEN** the recorded state for a task cannot be read
- **THEN** the agent still starts and nothing about it is guessed at

### Requirement: Continuity does not depend on the operator's environment
The system SHALL carry a conversation over for the agents it launches
without requiring any UZE-provided executable to be on the operator's PATH
and without requiring the operator to change how they start a harness by
hand.

#### Scenario: An untouched PATH still carries the conversation
- **WHEN** the operator's PATH contains only the harness's own executable and an agent is relaunched
- **THEN** its conversation is still carried over

### Requirement: Every harness proves continuity against its real binary
Conformance SHALL state the continuity outcome in vendor-neutral terms and
run it against every supported harness's real binary: a conversation
started in a task, its process ended, an agent launched into that same task
again, and something from the earlier turn present in the new process. A
harness that cannot deliver it SHALL declare it unsupported with a reason
that the run records, never by omitting the check.

#### Scenario: A supported harness carries a turn across a relaunch
- **WHEN** the continuity check runs against a harness that declares the capability
- **THEN** the relaunched process answers from the conversation the first process started

#### Scenario: An unsupported harness is recorded as such
- **WHEN** the continuity check runs against a harness that declares no mechanism
- **THEN** the run records it as unsupported with the reason, and the vertical does not fail for it
