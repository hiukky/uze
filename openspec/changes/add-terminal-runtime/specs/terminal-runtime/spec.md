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

### Requirement: One server per user
The system SHALL run one terminal server per user — per `UZE_HOME` — and
every `uze` client SHALL attach to it, whatever directory the client was
started in. The server's spaces, tabs and panes SHALL persist between runs
in one document under the user's UZE state.

#### Scenario: Two launches from two directories share one server
- **WHEN** a user starts `uze` in one directory and then in another
- **THEN** both clients are attached to the same server and see the same spaces

### Requirement: A space has a root
The system SHALL give every space a root directory, chosen when the space
is created, and SHALL derive the space's behaviour from that root: an agent
or a shell created in the space starts from it, and a root that is a Git
repository with a commit gives agents isolated checkouts. Starting `uze`
SHALL ensure a space rooted at the launch directory's workspace root exists
and SHALL select it for that client, creating it only when no space has
that root. Creating a space explicitly SHALL ask for its root, prefilled
with the selected space's.

#### Scenario: The launch directory becomes a space
- **WHEN** a user starts `uze` in a directory no space is rooted at
- **THEN** a space rooted there is created, labelled from the directory, and selected for that client

#### Scenario: A known root is selected, not duplicated
- **WHEN** a user starts `uze` in a directory a space is already rooted at
- **THEN** that space is selected for the client and no space is created

#### Scenario: A new space starts from a chosen root
- **WHEN** a user creates a space and confirms a root
- **THEN** the space's first shell opens in that root and agents created in it start from it

### Requirement: A new space's root is chosen, not typed
Creating a space SHALL offer the directories that exist rather than accept a
path typed blind: the prompt SHALL name a directory to list and a segment to
match inside it, SHALL narrow the offered directories as that segment is
typed, and SHALL create the space at the directory the person lands on.

#### Scenario: The listing narrows as the root is typed
- **WHEN** a user opens the new-space prompt and types part of a directory's name
- **THEN** only the directories of the listed directory whose names match are offered

#### Scenario: The chosen directory becomes the root
- **WHEN** a user confirms one of the offered directories
- **THEN** a space rooted at that directory is created

### Requirement: A tab belongs with the agent it was born from
A shell opened while an agent is in front of the person SHALL belong with
that agent and SHALL start in that agent's own directory. The tab strip
SHALL show one context at a time — that agent followed by the shells that
belong with it, and no other agent's. A shell belonging to no agent SHALL
belong to the space, reachable from the space's own row. Closing an agent
SHALL hand the shells opened alongside it to the space rather than closing
them, and SHALL remain a confirmed action, never a click on the strip.

#### Scenario: Switching agents switches the strip
- **WHEN** a user selects a different agent
- **THEN** the strip shows that agent and the shells that belong with it, and none of the previous agent's

#### Scenario: A shell opens on the agent's work
- **WHEN** a user opens a shell while an agent is selected
- **THEN** the shell starts in that agent's directory and appears with it

#### Scenario: An agent's shells outlive it
- **WHEN** an agent is closed
- **THEN** the shells opened alongside it remain, as shells of the space

### Requirement: Focus is per client
The system SHALL keep which space and which tab each attached client is
looking at per client. Selecting a space or a tab in one client SHALL NOT
move another client's selection, and the session a client receives SHALL
carry that client's own selection.

#### Scenario: Two terminals look at two agents
- **WHEN** two clients are attached and one selects a different space
- **THEN** the other client's selected space is unchanged

### Requirement: A launch inside a pane opens a space
The system SHALL mark every pane it spawns so a `uze` started inside one
can tell, and such a `uze` SHALL NOT open a client inside the client: it
SHALL ask the running server for a space rooted at its directory's
workspace root, created when none is, and exit reporting the space.

#### Scenario: Nested launch opens a space and leaves
- **WHEN** `uze` is started inside one of the server's own panes, in a directory no space is rooted at
- **THEN** a space rooted there appears in the running client and the nested `uze` exits without attaching a client
