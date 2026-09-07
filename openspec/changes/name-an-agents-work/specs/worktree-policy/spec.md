## MODIFIED Requirements

### Requirement: A task's identity is immutable and its name is derived
The system SHALL give every agent launch a generated task identifier that
never changes, and SHALL key the checkout and the task's persisted state on
it. The branch SHALL start from that identifier under the `agent/` prefix,
and both the branch and the visible label SHALL then be names rather than
derivations: replaced once while they are still generated, never overwritten
afterwards (see the `agent-work-naming` capability). A readable name derived
at publish time SHALL remain as the fallback for a task nobody named, not as
the mechanism by which work is named. Task state SHALL be persisted outside
every checkout and written atomically.

#### Scenario: The branch starts from the identifier
- **WHEN** an agent is created
- **THEN** its branch is the identifier under the `agent/` prefix, and its
  label is the generated one until the work is named

#### Scenario: A published branch carries a readable name
- **WHEN** a task nobody named is delivered by opening a pull request
- **THEN** the pushed branch is named from the task's first commit
- **AND** the task's identifier, checkout and state are unchanged

#### Scenario: State survives the checkout
- **WHEN** a task's checkout directory is removed
- **THEN** the task's state and transcript are still available

#### Scenario: An interrupted write leaves a valid state file
- **WHEN** the process is killed while task state is being written
- **THEN** the state file on disk is either the previous version or the new one, never truncated
