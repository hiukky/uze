## MODIFIED Requirements

### Requirement: The project manifest declares the desired agent environment
The system SHALL record a project's desired agent environment in a
version-checked manifest (`agents.yaml`) covering its marketplaces, the
plugins it requests, and its isolation policy: which branch
finished work targets, what it does with an isolated agent's finished work,
which ignored files a fresh checkout
links from the primary checkout, the command that prepares a checkout, the
command that gates delivery, and an optional cap on concurrent checkouts.
Every policy field SHALL be optional with a safe default, and a manifest with no
policy block SHALL still load. An undeclared target SHALL resolve to the
branch checked out in the primary checkout when a task is created. A linked path SHALL be relative, SHALL stay
inside the repository, and SHALL be ignored by the repository; a path
violating any of these SHALL be rejected when the manifest is read, not when a
checkout is prepared. A manifest naming a field the current schema has replaced,
or a field the policy block does not know, SHALL be reported as malformed
rather than parsed with that field ignored; unknown fields at the top level
SHALL be tolerated so a manifest written by a newer version still loads.

#### Scenario: The policy round-trips with the rest of the manifest
- **WHEN** a manifest declaring marketplaces, plugins, a target branch, a completion behavior, linked files, a setup command, a gate command and a checkout cap is written and read back
- **THEN** every declaration is preserved

#### Scenario: A manifest without a policy block still loads
- **WHEN** a manifest declares marketplaces and plugins and no policy block
- **THEN** the environment loads with the default completion behavior and no linked files, setup, gate or cap

#### Scenario: An undeclared target is the primary's branch
- **WHEN** a manifest declares a policy block without a target and a task is created while the primary checkout is on a branch
- **THEN** that branch is the task's target

#### Scenario: A link escaping the repository is rejected at read time
- **WHEN** a manifest links an absolute path or a path containing a parent segment
- **THEN** the system reports a malformed manifest naming the path
- **AND** no environment is loaded from it

#### Scenario: A link to a tracked file is rejected at read time
- **WHEN** a manifest links a path the repository does not ignore
- **THEN** the system reports a malformed manifest naming the path and the reason

#### Scenario: A replaced field is rejected rather than silently dropped
- **WHEN** a manifest names the superseded bare worktree-directory field
- **THEN** the system reports a malformed manifest naming the replacement shape
- **AND** no environment is loaded from it

#### Scenario: An unknown policy field is rejected by name
- **WHEN** a manifest's policy block carries a field the schema does not define
- **THEN** the system reports a malformed manifest naming that field
