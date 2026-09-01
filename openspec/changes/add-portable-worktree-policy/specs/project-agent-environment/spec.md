## MODIFIED Requirements

### Requirement: The project lock declares the desired agent environment
The system SHALL record a project's desired agent environment in a
version-checked, deterministic lock file covering its marketplaces, its
plugins with resolved sources, and what it does with an isolated agent's finished work. A
lock naming a field the current schema has replaced SHALL be reported as
malformed rather than parsed with that field ignored.

#### Scenario: The completion behavior round-trips with the rest of the lock
- **WHEN** a lock declaring marketplaces, plugins, and a completion behavior is
  written and read back
- **THEN** every declaration is preserved, including the completion behavior

#### Scenario: A replaced field is rejected rather than silently dropped
- **WHEN** a lock names the superseded bare worktree-directory field
- **THEN** the system reports a malformed lock naming the replacement shape
- **AND** no environment is loaded from it
