## ADDED Requirements

### Requirement: A tab carries the environment its launch put there
The terminal runtime SHALL accept, when a tab is created, a set of
environment variables for the pane's first process, SHALL apply them to
that process, SHALL persist them with the tab exactly as it persists the
tab's command, SHALL apply them again when it respawns the tab with that
command, and SHALL report them back to clients as part of the tab. The
runtime SHALL NOT interpret any of them. It SHALL accept an environment
only together with a command, SHALL refuse a request whose environment
exceeds a documented bound on entries or total size, and SHALL refuse a
variable whose name is empty or contains `=`. A tab respawned as a plain
shell SHALL carry and report none of them.

#### Scenario: A shell never carries a launch environment
- **WHEN** a client creates a tab with an environment and no command
- **THEN** the request is refused and no tab is created

#### Scenario: An oversized environment is refused
- **WHEN** a client creates a tab whose environment exceeds the documented bound
- **THEN** the request is refused and no tab is created

#### Scenario: The environment reaches the first process
- **WHEN** a client creates a tab with a command and an environment
- **THEN** the command's process starts with that environment applied

#### Scenario: The environment survives a server restart
- **WHEN** the server is restarted and restores a tab whose launch carried an environment
- **THEN** the respawned command starts with the same environment

#### Scenario: The environment is reported, never interpreted
- **WHEN** a client attaches
- **THEN** each tab in the session carries the environment its launch put there, as data
- **AND** nothing in the server's behaviour depends on any value in it

#### Scenario: A shell respawn carries nothing
- **WHEN** a tab whose agent exited is restored as a plain shell
- **THEN** the shell starts without the launch's environment and the tab reports none

### Requirement: A pane inherits nothing that identifies the server's own launch
The terminal runtime SHALL strip from every pane it spawns the variables
that identify an agent launch, so a server that was itself started from
inside an agent's pane never hands that agent's identity to the panes it
opens. A pane SHALL carry only what its own launch put there.

#### Scenario: A server born inside an agent spawns clean panes
- **WHEN** the server process was started from a shell inside an agent's pane and later opens a plain shell tab
- **THEN** that shell carries no agent identity
