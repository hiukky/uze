## MODIFIED Requirements

### Requirement: A managed agent's conversation is recorded against its task
The system SHALL record, for every agent it launches — into a task in a
worktree space, or as a tenant of a workspace space — the identifier of
the conversation that agent's harness started, together with the harness
it belongs to. The record SHALL live in UZE's own state beside the agent's
record, never inside any checkout, and SHALL be keyed by the agent's
identity so that it survives a checkout being reset, moved or removed. The
system SHALL NOT record any harness credential or conversation content.

#### Scenario: A new agent's conversation is recorded
- **WHEN** an agent is launched into a task for the first time
- **THEN** the conversation the harness started is recorded against that task and that harness

#### Scenario: A tenant's conversation is recorded
- **WHEN** an agent is launched as a tenant of a workspace space for the first time
- **THEN** the conversation the harness started is recorded against that tenant and that harness

#### Scenario: The record survives the checkout
- **WHEN** a task's checkout is reset, or removed and given back to the task later
- **THEN** the recorded conversation is still the one the task started with

#### Scenario: A task carries one conversation per harness
- **WHEN** an agent has been worked in by two different harnesses
- **THEN** each harness's conversation is recorded separately and neither is offered to the other
