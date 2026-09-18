## MODIFIED Requirements

### Requirement: Relaunching an agent continues its task's conversation
The system SHALL start an agent whose task already has a conversation for
that harness by resuming that conversation, and SHALL start a fresh one
only when the task has none. The task SHALL be resolved from the identity
the launch carries (see the `agent-identity` capability), never from the
directory the process stands in. This SHALL hold however the agent is
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

#### Scenario: Two agents in one directory keep their own conversations
- **WHEN** two agents run in the same directory and each is relaunched
- **THEN** each resumes its own conversation and neither sees the other's

### Requirement: The operator's own invocation is never rewritten
The system SHALL leave an invocation the operator composed exactly as
typed. Continuity SHALL apply only to a launch that carries an agent's
identity and asks for nothing about sessions itself: an invocation carrying
the harness's own session or resume arguments SHALL be passed through
untouched; an invocation carrying no identity — the operator running a
harness by hand, in any directory, an isolated checkout included — SHALL
behave exactly as it did before this capability existed; and an invocation
made from inside an agent's pane by that agent SHALL be treated as
carrying no identity.

#### Scenario: An explicit session argument wins
- **WHEN** a harness is started inside a managed task with its own resume or session argument
- **THEN** that argument decides the conversation and UZE adds nothing

#### Scenario: Outside a managed task nothing changes
- **WHEN** the operator starts a harness by hand, in a shell, in any directory including an agent's own checkout
- **THEN** it starts exactly as it would without UZE, with no conversation carried over

#### Scenario: A launch from inside an agent is ordinary
- **WHEN** an agent's harness starts another harness process in its own pane
- **THEN** the inner process starts fresh and the agent's recorded conversation is unchanged
