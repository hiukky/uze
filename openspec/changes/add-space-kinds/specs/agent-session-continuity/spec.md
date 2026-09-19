## MODIFIED Requirements

### Requirement: A managed agent's conversation is recorded against its task
The system SHALL record, for every agent it launches — whether it works in
the space's own root or in a checkout of its own — the identifier of the
conversation that agent's harness started, together with the harness it
belongs to. The record SHALL live in UZE's own state beside the agent's
record, never inside any checkout, and SHALL be keyed by the agent's
identity so that it survives a checkout being reset, moved or removed, and
so that it survives the agent being isolated. The system SHALL NOT record
any harness credential or conversation content.

#### Scenario: A new agent's conversation is recorded
- **WHEN** an agent is launched for the first time
- **THEN** the conversation the harness started is recorded against that agent and that harness

#### Scenario: The record survives isolation
- **WHEN** an agent working in the space's root is isolated into a checkout of its own
- **THEN** the conversation recorded against it is the one it was already in, and the relaunched agent continues it

#### Scenario: The record survives the checkout
- **WHEN** an isolated agent's checkout is reset, or removed and given back later
- **THEN** the recorded conversation is still the one the agent started with

#### Scenario: A task carries one conversation per harness
- **WHEN** an agent has been worked in by two different harnesses
- **THEN** each harness's conversation is recorded separately and neither is offered to the other
