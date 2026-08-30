## Purpose

Provide a local terminal workspace that keeps interactive agent processes alive
while users move between workspace orchestration and UZE management.

## ADDED Requirements

### Requirement: Persistent terminal session
The system SHALL provide a local terminal session whose server owns every pane
PTY and child process independently from an attached UI client.

#### Scenario: Client detaches while an agent is running
- **WHEN** a user detaches from an active terminal session containing a running agent
- **THEN** the session server SHALL keep the agent process and its PTY alive
- **AND THEN** a later client attachment SHALL reconnect to the same session

### Requirement: Native terminal pane behavior
The system SHALL render a pane from terminal-emulation state derived from its
PTY output and SHALL send focused-pane input and resize events back to that
PTY.

#### Scenario: Interactive agent uses terminal controls
- **WHEN** an agent emits cursor movement, styled output, or an alternate-screen transition
- **THEN** the attached client SHALL render the corresponding pane state
- **AND THEN** switching away from and back to the pane SHALL not restart the agent process

### Requirement: Workspace organization
The system SHALL organize a local terminal session as workspaces containing
tabs, with each tab containing at least one pane.

#### Scenario: User creates and selects tabs
- **WHEN** a user creates a terminal tab and selects another tab
- **THEN** each tab SHALL retain its own panes and running processes
- **AND THEN** the sidebar and tab header SHALL identify the selected tab

### Requirement: Management context switching
The system SHALL allow a user to switch between the terminal workspace client
and the existing UZE management TUI without terminating the terminal session.

#### Scenario: User returns from management to a running pane
- **WHEN** a user switches from an active terminal workspace to the management TUI and then returns
- **THEN** the client SHALL reattach to the existing terminal session
- **AND THEN** each running pane SHALL retain its process and terminal state

### Requirement: Explicit session termination
The system SHALL expose an explicit terminal-session stop action that ends the
server and its remaining pane processes.

#### Scenario: User stops a terminal session
- **WHEN** a user requests that a terminal session stop
- **THEN** the server SHALL detach connected clients and terminate its managed panes
- **AND THEN** a subsequent attachment SHALL create a new session rather than reuse the stopped one

### Requirement: Runtime isolation
The terminal runtime SHALL be opt-in and SHALL NOT participate in package
installation, harness projection, or environment-maintenance reconciliation.

#### Scenario: Ordinary management command runs without a terminal session
- **WHEN** a user runs an existing management command without opening a terminal workspace
- **THEN** the command SHALL NOT start a terminal server or create PTYs

### Requirement: Portable runtime boundary
The terminal runtime SHALL define transport and PTY boundaries independently of
the host operating system. The initial release SHALL support Linux and macOS;
adding a Windows backend SHALL NOT require a change to workspace, tab, pane,
or client/server lifecycle semantics.

#### Scenario: Initial supported platform starts a session
- **WHEN** a user on Linux or macOS attaches to a terminal workspace
- **THEN** the system SHALL use the platform's local transport and PTY backend
- **AND THEN** the workspace behavior SHALL conform to this specification
