# terminal-runtime Specification

## Purpose
What the workspace client does with the terminals the runtime keeps alive:
how their tabs are ordered and moved, and the code surface that shows the
active tab's checkout — what changed in it, what it contains, and the edits
made to it — without disturbing any pane's process.
## Requirements
### Requirement: Drag-to-reorder agent tabs in the sidebar
The workspace TUI client SHALL let a user reorder an agent tab within its
space's sidebar list by dragging its row up or down among the other agent
rows of that same space.

#### Scenario: User drags an agent row to a new position
- **WHEN** a user drags an agent's sidebar row past another agent row of
  the same space and releases
- **THEN** the client SHALL request that the dragged tab move to the
  released position
- **AND THEN** the sidebar SHALL show the dragged agent in its new
  position

#### Scenario: User drags an agent row without crossing another row
- **WHEN** a user presses and releases an agent's sidebar row without the
  pointer moving past the row-reorder threshold
- **THEN** the client SHALL NOT request any reorder
- **AND THEN** the press SHALL still select that agent, exactly as a plain
  click does today

#### Scenario: User drags an agent row outside the sidebar's agent list
- **WHEN** a user drags an agent's sidebar row outside that space's agent
  rows (over another space, over the tab strip, or off any drop target)
  and releases
- **THEN** the client SHALL NOT request any reorder
- **AND THEN** the agent SHALL remain in its original position

### Requirement: Drag-to-reorder shell tabs in the tab strip
The workspace TUI client SHALL let a user reorder a shell tab within the
horizontal tab strip by dragging it left or right among the other tabs
currently shown in that same strip.

#### Scenario: User drags a shell tab to a new position
- **WHEN** a user drags a shell tab past another tab in the same strip and
  releases
- **THEN** the client SHALL request that the dragged tab move to the
  released position
- **AND THEN** the strip SHALL show the dragged tab in its new position

#### Scenario: User drags a shell tab outside the strip
- **WHEN** a user drags a shell tab outside the tab strip and releases
- **THEN** the client SHALL NOT request any reorder
- **AND THEN** the tab SHALL remain in its original position

### Requirement: Reordering is confined to the dragged tab's own group
Reordering SHALL only change a tab's position among the other tabs it
already renders alongside — the other agent tabs of its own space for an
agent tab, or the other tabs of the same strip for a shell tab. It SHALL
NOT move a tab to a different space, change which agent a shell tab is
shown with, or turn a shell tab into an agent tab or vice versa.

#### Scenario: Reordering leaves grouping unchanged
- **WHEN** a reorder is applied to a tab
- **THEN** the tab SHALL remain in the same space
- **AND THEN** a shell tab SHALL remain associated with the same agent tab
  it was associated with before the reorder
- **AND THEN** the reorder SHALL NOT change which tab is selected

### Requirement: Server applies and validates a tab reorder
The server SHALL accept a request to move a tab to a new position among
the tabs of its own space, and SHALL reject a request naming a target tab
that does not belong to the same space as the tab being moved.

#### Scenario: Valid reorder request
- **WHEN** the server receives a request to move a tab to sit before
  another tab of the same space
- **THEN** the server SHALL update that space's tab order accordingly
- **AND THEN** the server SHALL notify attached clients of the new order

#### Scenario: Reorder request naming a tab from a different space
- **WHEN** the server receives a request to move a tab to sit before a tab
  that belongs to a different space
- **THEN** the server SHALL reject the request
- **AND THEN** the space's tab order SHALL remain unchanged

#### Scenario: Reorder request naming a tab that no longer exists
- **WHEN** the server receives a request naming a tab (as the one being
  moved, or as the target to move before) that no longer exists
- **THEN** the server SHALL reject the request
- **AND THEN** the space's tab order SHALL remain unchanged

### Requirement: One code surface for the active checkout
The workspace TUI client SHALL provide one surface, scoped to the
currently active tab's live working directory, that shows what changed in
that checkout and what it contains. The surface SHALL be reachable by two
entry points: one that opens it on the changes, and one that opens it on
the directory tree.

#### Scenario: User opens the surface on the changes
- **WHEN** a user triggers the changes entry point while a terminal tab is
  active
- **THEN** the client SHALL show the files changed in that tab's working
  directory and the diff of the selected one

#### Scenario: User opens the surface on the files
- **WHEN** a user triggers the files entry point while a terminal tab is
  active
- **THEN** the client SHALL list the entries of that tab's working
  directory
- **AND THEN** the client SHALL list directories before files

#### Scenario: The checkout has nothing uncommitted
- **WHEN** the active tab's working directory has nothing uncommitted
- **THEN** the files entry point SHALL still be present
- **AND THEN** the changes SHALL still be reachable from the surface it
  opens, as an empty list rather than an error

#### Scenario: The working directory is outside a repository
- **WHEN** the surface is opened for a tab whose working directory is not
  inside a git repository
- **THEN** the client SHALL say so where the changes would be
- **AND THEN** the directory's files SHALL still be listable

#### Scenario: User opens a directory
- **WHEN** a user opens a directory in the tree
- **THEN** the client SHALL list that directory's own entries beneath it
- **AND THEN** the client SHALL leave every directory the user has not
  opened unlisted

#### Scenario: User selects a file
- **WHEN** a user selects a file in the tree
- **THEN** the client SHALL show that file's contents with syntax
  highlighting for its language

#### Scenario: Selected file cannot be read as text
- **WHEN** a user selects a file the client cannot read as text
- **THEN** the client SHALL say so where the contents would be, rather
  than showing an empty file or failing to open

#### Scenario: A directory cannot be read
- **WHEN** a directory below the root cannot be listed
- **THEN** the client SHALL report that condition and leave the rest of
  the tree navigable

### Requirement: The surface says which checkout it is about
The code surface SHALL say which checkout it is showing and which branch
that checkout is at, and SHALL distinguish those from each other and from
the surface's own name, rather than presenting them as one run of text.

#### Scenario: The surface is open on a checkout
- **WHEN** the code surface is showing a checkout
- **THEN** it SHALL name that checkout and the branch it is at
- **AND THEN** the checkout's own name and its branch SHALL be
  distinguishable from the directories leading to it

#### Scenario: The checkout is at no branch
- **WHEN** the checkout has no branch to name
- **THEN** the surface SHALL say the rest without leaving an empty part
  where the branch would be

### Requirement: The selection survives a change of mode
The code surface SHALL address one file at a time, and that file SHALL
remain selected when the viewer switches between the diff of it and its
contents. Where the viewer's position within the file is known, that
position SHALL be carried across the switch too.

#### Scenario: User switches from a diff to the file's contents
- **WHEN** a user viewing the diff of a file switches to its contents
- **THEN** the client SHALL show that same file
- **AND THEN** the client SHALL place the caret on the line the viewer was
  reading in the diff

#### Scenario: User switches to the contents of a file the tree has not listed
- **WHEN** the file selected in the changes has not been listed in the
  directory tree yet
- **THEN** the client SHALL open the directories containing it so that the
  file is shown in the tree

#### Scenario: User switches from a file's contents back to its diff
- **WHEN** a user viewing a file's contents switches to its diff
- **THEN** the client SHALL show that same file's diff

### Requirement: The code surface is always reachable
The code surface SHALL be reachable whenever the terminal workspace is
showing, independently of whether the active tab's working directory has
changes, is inside a repository, or is a repository at all. An entry
point that reports how much changed MAY be absent when nothing has, and
the entry point that is always present SHALL NOT move when it appears.

#### Scenario: The checkout acquires changes
- **WHEN** the active tab's working directory acquires changes
- **THEN** an entry point SHALL report how much changed
- **AND THEN** the entry point that was already there SHALL NOT move to
  make room for it

### Requirement: Editing a file
The code surface SHALL let a user change the contents of a file it is
showing and write those changes back to that file. An edit SHALL NOT
reach the filesystem until the user saves it.

#### Scenario: User edits and saves
- **WHEN** a user edits the contents of a file and saves
- **THEN** the client SHALL write the edited contents to that file
- **AND THEN** the client SHALL report that the file was saved

#### Scenario: User edits without saving
- **WHEN** a user edits the contents of a file and does not save
- **THEN** the file on disk SHALL be unchanged

#### Scenario: A save fails
- **WHEN** writing the file does not succeed
- **THEN** the client SHALL report the failure and SHALL continue to show
  the edited contents

#### Scenario: User clicks inside the file's contents
- **WHEN** a user clicks a position in the contents of the file being
  shown
- **THEN** the client SHALL place the caret at the character that position
  falls on

#### Scenario: The caret is on a character
- **WHEN** the caret sits on a character of the file
- **THEN** that character SHALL remain visible

#### Scenario: User closes the surface with unsaved changes
- **WHEN** a user asks to close the code surface while a file has unsaved
  changes
- **THEN** the client SHALL ask for confirmation before closing
- **AND THEN** the surface SHALL stay open until the user confirms

### Requirement: Deleting a file
The code surface SHALL let a user delete a file it lists, and SHALL ask
for confirmation before doing so. It SHALL NOT delete a directory.

#### Scenario: User deletes a file
- **WHEN** a user asks to delete a selected file and confirms
- **THEN** the client SHALL remove that file
- **AND THEN** the client SHALL list the containing directory as it now is

#### Scenario: User declines the confirmation
- **WHEN** a user asks to delete a selected file and does not confirm
- **THEN** the file SHALL be unchanged

#### Scenario: User asks to delete a directory
- **WHEN** a user asks to delete a selected directory
- **THEN** the client SHALL refuse and say why
- **AND THEN** the directory and everything under it SHALL be unchanged

### Requirement: The surface creates nothing it was not shown
The code surface SHALL only write to files that already exist. It SHALL
NOT create a file or a directory.

#### Scenario: A save names a path that is not an existing file
- **WHEN** a save would write to a path that is not an existing file
- **THEN** the client SHALL refuse the write

#### Scenario: The surface leaves the repository alone
- **WHEN** a user views a diff, edits a file or deletes one
- **THEN** the client SHALL NOT stage, unstage, commit or discard anything
  in the repository

### Requirement: A document can be read as itself or as its markup
Where the selected file is a document the client can render, the code
surface SHALL offer both ways of showing it — the document, and the
markup that describes it — and SHALL make the choice reachable by
pointing at it as well as by keyboard. Where the file is not such a
document, no such choice SHALL be offered.

#### Scenario: User opens a document
- **WHEN** a user opens a file the client can render as a document
- **THEN** the client SHALL offer both ways of showing it, marking which
  is current
- **AND THEN** it SHALL show the markup, because opening a file is for
  changing it

#### Scenario: User chooses the rendered view
- **WHEN** a user chooses the rendered way of showing the document
- **THEN** the client SHALL show the document rather than its markup
- **AND THEN** what it shows SHALL reflect unsaved edits, not only what
  is on disk

#### Scenario: The file is not a document
- **WHEN** the selected file is not one the client can render
- **THEN** no choice of how to show it SHALL be offered

### Requirement: Either half of the surface can be scrolled with the pointer
Where a list or a file is longer than the room it has, the code surface
SHALL show how much of it is on screen and where, and SHALL let a pointer
move to any part of it by dragging. Where everything fits, no such
control SHALL be shown.

#### Scenario: The content is longer than the frame
- **WHEN** a file or a diff has more lines than the surface can show
- **THEN** the client SHALL draw a scrollbar for it
- **AND THEN** the size of its handle SHALL reflect how much of the whole
  is on screen

#### Scenario: Everything fits
- **WHEN** a list or a file fits entirely in the room it has
- **THEN** the client SHALL draw no scrollbar for it

#### Scenario: User drags a scrollbar
- **WHEN** a user drags a scrollbar's handle
- **THEN** the client SHALL show the part of the list or file that
  position names
- **AND THEN** taking hold of the handle without moving it SHALL scroll
  nothing

#### Scenario: The scrollbar shares a line with the edge that moves it
- **WHEN** a user takes hold of an edge that both scrolls a list and
  moves the boundary beside it
- **THEN** the client SHALL NOT decide which of the two it is until the
  pointer moves
- **AND THEN** it SHALL decide by the direction of that movement, and
  SHALL keep that decision until the button is released

#### Scenario: User clicks such an edge without moving
- **WHEN** a user presses and releases on that edge without moving
- **THEN** the client SHALL show the part of the list that position
  names

### Requirement: The code surface never blocks the workspace
Reading the changes, reading a directory, reading a file, writing one and
deleting one SHALL NOT block the workspace client's drawing or input. A
background re-read of the changes SHALL NOT alter a file's contents that
the viewer has edited and not saved. The surface SHALL remain dismissible
back to exactly the workspace state the user was in before opening it,
without ending, resizing, or otherwise disrupting any pane's running
process.

#### Scenario: The changes are re-read while a file is being edited
- **WHEN** the surface re-reads what changed in the checkout while a file
  it holds has unsaved edits
- **THEN** those edits SHALL be unaffected

#### Scenario: A slow read is in progress
- **WHEN** the surface is waiting on a directory or file the filesystem
  has not answered for yet
- **THEN** the client SHALL keep drawing and accepting input
- **AND THEN** the client SHALL say that it is working

#### Scenario: User dismisses the surface
- **WHEN** a user closes the code surface with nothing unsaved
- **THEN** the client SHALL return to the terminal workspace exactly as it
  was before it opened
- **AND THEN** every pane's process SHALL be unaffected by the surface
  having been open

### Requirement: A space is a root, and the runtime knows nothing else about it
The terminal runtime SHALL identify a space by its root alone. It SHALL
NOT carry, persist or report a kind for a space, and nothing it does with
panes, tabs or processes SHALL depend on one. When asked to open a space
for a root, the runtime SHALL reuse the existing space for that root, and
create one only when there is none.

A persisted workspace written by a version that recorded a kind per space
SHALL be read by the rule the `upgrade-resilience` capability states for a
document of an older version.

#### Scenario: A root names one space
- **WHEN** a client opens a space for a root that already has one
- **THEN** the existing space is selected and none is created

#### Scenario: The space round-trips through a restart
- **WHEN** a space is created and the server is restarted
- **THEN** the restored space is the same space, at the same root

#### Scenario: A workspace persisted by an older version
- **WHEN** the runtime restores a persisted workspace whose spaces carry a kind
- **THEN** it reads it as the older document it is, and the operator is told what was done about it

### Requirement: The workspace claim names the process holding it

The server serving a workspace SHALL record its own process id in the
claim it holds, so that a client which cannot reach the endpoint can still
say — and end — what holds the workspace. A reader SHALL treat the
recorded id as a lead and corroborate it against the process table before
acting on it; the lock, not the record, remains the proof that a server is
alive.

#### Scenario: A client cannot reach the endpoint

- **WHEN** a client finds the workspace claimed and nothing answering at
  the endpoint this build computes
- **THEN** it SHALL allow the server the time its endpoint watch needs to
  restore a socket that was taken from it
- **AND THEN** failing that, it SHALL end the recorded holder and start a
  server that answers where this build looks
- **AND THEN** the spaces and panes the previous server was serving SHALL
  be restored by the one that replaces it

#### Scenario: The claim names nobody this process can act on

- **WHEN** the claim is held but records no id, or records one the process
  table no longer vouches for
- **THEN** the client SHALL report that the workspace is served by
  something it cannot reach, naming the endpoint it looked at and the
  command that ends a server
- **AND THEN** it SHALL NOT report only the operating system's error about
  the endpoint path

### Requirement: Stopping the runtime stops what holds the workspace

`uze terminal stop` SHALL end the server holding the workspace, whether or
not that server answers at the endpoint this build computes. "Nothing
answers here" SHALL NOT be reported as "nothing is running" while the
workspace is claimed.

#### Scenario: The server recorded itself and is on another endpoint

- **WHEN** `uze terminal stop` runs while a server that recorded its own
  id holds the workspace at an endpoint this build does not name
- **THEN** that server SHALL be ended
- **AND THEN** the workspace SHALL be free for the next server

#### Scenario: The server predates the record

- **WHEN** the workspace is claimed, nothing answers at this build's
  endpoint, and the claim records nobody — a server from a release older
  than the record itself
- **THEN** `uze terminal stop` SHALL report that, naming the endpoint it
  looked at and how to find the process
- **AND THEN** it SHALL NOT report success, and SHALL signal nothing

#### Scenario: Nothing is running at all

- **WHEN** `uze terminal stop` runs with no server holding the workspace
  and no endpoint present
- **THEN** it SHALL succeed, having nothing to do

### Requirement: The endpoint lives beside the workspace it serves

The endpoint SHALL be named from the workspace's own location, so that
every terminal of one machine computes the same endpoint for one
`UZE_HOME` whatever their session environment says, and no directory a
system cleaner owns can take it while the workspace itself survives. Where
that path cannot hold a socket, UZE SHALL fall back to the session's
runtime directory and the system temporary directories in turn.

#### Scenario: Two terminals with different session environments

- **WHEN** two terminals of one machine have different values for the
  session's runtime directory, or one has none
- **THEN** both SHALL compute the same endpoint for the same `UZE_HOME`
- **AND THEN** the second SHALL attach to the server the first started

#### Scenario: A home too long for a socket path

- **WHEN** the workspace's own directory would exceed the length a socket
  path allows
- **THEN** UZE SHALL use the first fallback directory that can hold one
- **AND THEN** the endpoint SHALL still be one per `UZE_HOME`

### Requirement: A server is never started from a binary that is gone

Starting a server SHALL use an executable that exists. Where the running
image has been replaced on disk — an install over a live session — UZE
SHALL start the server from `uze` as the path resolves it, and where
neither exists it SHALL say so rather than reporting a missing file.

#### Scenario: The binary was replaced while the client ran

- **WHEN** a client needs to start a server after its own binary has been
  replaced on disk
- **THEN** the server SHALL be started from the installed `uze`
- **AND THEN** no error naming a deleted path SHALL reach the operator

### Requirement: A workspace written by an earlier build opens on this one

The persisted workspace SHALL be carried across to the shape this build
reads, keeping every space, its root, its tabs and each tab's directory
and launch. A field an earlier shape carried that this one dropped SHALL
be the only thing lost. Spaces SHALL NOT be lost because the workspace was
written by an earlier build whose shape this one knows.

#### Scenario: Upgrading with spaces open

- **WHEN** UZE is upgraded and the persisted workspace was written by an
  earlier shape this build knows
- **THEN** the same spaces SHALL open, with the same roots, tabs and
  directories
- **AND THEN** nothing SHALL be reported

#### Scenario: The shape that carried a kind per space

- **WHEN** the workspace was written when a space carried a kind of its
  own
- **THEN** every space SHALL open without it
- **AND THEN** nothing else about the workspace SHALL change

### Requirement: The runtime says what it could not carry across

When the terminal runtime cannot carry the persisted workspace across and
starts from nothing, it SHALL tell the client, and the client SHALL show
the operator what happened and where the previous workspace was kept.

Reporting it only where a log would have to be turned on SHALL NOT satisfy
this: the runtime and the screen are different processes, and the operator
is at the screen.

#### Scenario: A workspace that could not be read

- **WHEN** the runtime starts from nothing because the persisted workspace
  could not be read or carried across
- **THEN** the operator SHALL be told on screen
- **AND THEN** they SHALL be told where the previous workspace is kept

#### Scenario: A first run

- **WHEN** the runtime starts with no workspace persisted at all
- **THEN** nothing SHALL be reported: there was nothing to lose

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

#### Scenario: A launch after the binary was replaced keeps the running panes
- **WHEN** the `uze` binary is replaced while a server is running agents, and a user starts `uze` again
- **THEN** the new client attaches to the running server whenever it answers this build's handshake, and every pane keeps its process
- **AND THEN** a server that cannot answer it is ended instead, and the spaces, tabs and panes it held are restored by the server that replaces it

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

