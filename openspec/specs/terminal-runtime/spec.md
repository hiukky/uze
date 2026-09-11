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
