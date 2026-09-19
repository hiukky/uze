## Purpose

Draws a project's declared architecture artifacts — Mermaid flowcharts, C4
views and sequence diagrams — inside the workspace TUI, in terminal cells
alone, so the shape of a system can be read and walked down to its code
without leaving the terminal.

## ADDED Requirements

### Requirement: A project declares where its architecture artifacts live
A project SHALL declare its architecture artifacts by naming one directory
in `agents.yaml`, as `artifacts.path`, relative to the project root. The
declaration SHALL be optional. A path that is absolute, or that leaves the
project, SHALL be refused rather than followed. The scaffolded
`agents.yaml` SHALL show the key, commented out.

#### Scenario: The project declares a directory
- **WHEN** `agents.yaml` holds `artifacts: { path: docs/architecture }`
- **THEN** the artifacts are read from `docs/architecture` under the
  project root

#### Scenario: The project declares nothing
- **WHEN** the surface is opened in a project whose `agents.yaml` has no
  `artifacts` key
- **THEN** the surface SHALL say the project declares no artifacts yet
- **AND THEN** it SHALL show how to declare them, and SHALL NOT present
  this as an error

#### Scenario: The declared path leaves the project
- **WHEN** `artifacts.path` is `../elsewhere` or `/etc`
- **THEN** nothing SHALL be read from that path
- **AND THEN** the surface SHALL say that `artifacts` in `agents.yaml`
  needs fixing

#### Scenario: The declared directory holds no diagram
- **WHEN** the declared directory exists and holds no Mermaid file
- **THEN** the surface SHALL name the declared path and say it holds no
  Mermaid files yet

#### Scenario: An unknown key under artifacts
- **WHEN** `artifacts` holds a key other than `path`
- **THEN** the manifest SHALL be rejected, as it is for any unknown key

### Requirement: An artifact is a Mermaid file, described by itself
Every file ending in `.mmd` or `.mermaid` under the declared directory,
to a bounded depth and skipping hidden entries, SHALL be one artifact. No
manifest, index or registration SHALL be required beside the files. An
artifact's **area** SHALL be derived from the diagram's first word: the C4
family is one area, `sequenceDiagram` another, `flowchart` and `graph` a
third, and any other Mermaid diagram a fourth that is listed rather than
hidden. An artifact's **name** SHALL be its Mermaid front-matter `title`,
else the diagram's own `title` statement, else its file name made
readable.

#### Scenario: A file is added to the directory
- **WHEN** a `.mmd` file beginning with `C4Container` is added under the
  declared directory and the surface is opened
- **THEN** it SHALL be listed under the C4 area with no other file edited

#### Scenario: A title in the front matter
- **WHEN** a file opens with a front matter whose `title` is `Containers`
- **THEN** the artifact SHALL be listed as `Containers`, and the front
  matter SHALL NOT be drawn as part of the diagram

#### Scenario: A file with no title
- **WHEN** `install-pipeline.mmd` carries no title of either kind
- **THEN** it SHALL be listed by a name derived from `install-pipeline`

#### Scenario: A comment before the diagram's first word
- **WHEN** a file's first lines are blank or `%%` comments
- **THEN** the area SHALL be derived from the first line that is neither

#### Scenario: A Mermaid diagram this surface does not draw
- **WHEN** a file begins with a diagram type outside the three drawn ones
- **THEN** it SHALL still be listed, under its own area
- **AND THEN** opening it SHALL say it could not be drawn rather than
  showing an empty board

### Requirement: Artifacts are listed by area, and C4 from the outside in
Artifacts SHALL be ordered by area, and within an area by name — except
the C4 area, which SHALL be ordered by level first: context, then
container, then component, then dynamic, then any other C4 diagram.

#### Scenario: Three C4 levels whose names sort the other way
- **WHEN** the C4 area holds `Containers`, `Core components` and
  `System context`
- **THEN** they SHALL be listed as `System context`, `Containers`, `Core
  components`

#### Scenario: Stepping to the next artifact
- **WHEN** the next-artifact action is triggered on the last artifact of
  an area
- **THEN** the first artifact of the following area SHALL open
- **AND THEN** on the last artifact of all, the first of all SHALL open

### Requirement: The menu is one row of two selectors
The surface SHALL show, on a single row above the board, a selector for
the area and a selector for the artifact within it, each naming what is
currently open. Opening a selector SHALL list its choices over the board;
the area list SHALL say how many artifacts each area holds. While a list is
open it SHALL take the keyboard: up and down move the highlight, left and
right step between the two lists, activate opens the highlighted choice,
and dismiss closes the list without leaving the surface. A list longer than
the board SHALL keep the highlight in view and say where it is in the
whole.

#### Scenario: Choosing another area
- **WHEN** the area selector is opened and `Sequence` is activated
- **THEN** the first artifact of the Sequence area SHALL open
- **AND THEN** both selectors SHALL name the new area and artifact

#### Scenario: A list longer than the board
- **WHEN** the artifact list holds more entries than the board has rows
  and the highlight is moved past the last visible one
- **THEN** the list SHALL scroll so the highlight stays visible
- **AND THEN** it SHALL show the highlight's position out of the total

#### Scenario: Dismiss with a list open
- **WHEN** a list is open and dismiss is triggered
- **THEN** the list SHALL close and the surface SHALL stay open

#### Scenario: A selector is clicked
- **WHEN** a selector is clicked, and then one of its choices
- **THEN** the click on the choice SHALL open it, and SHALL NOT also reach
  the board underneath

### Requirement: Diagrams are drawn in terminal cells alone
A diagram SHALL be drawn with characters in cells, colored by the active
theme's roles, and SHALL NOT depend on any terminal graphics protocol or
on a font the terminal was not already using. The surface SHALL offer the
same drawing in box-drawing characters and in plain ASCII, and the
artifact's own Mermaid source, switched by one action.

#### Scenario: A terminal with no graphics protocol
- **WHEN** the surface is opened over SSH inside a multiplexer
- **THEN** the diagram SHALL be drawn the same as in a local terminal

#### Scenario: The ASCII rendering
- **WHEN** the next-rendering action is triggered from the default
  rendering
- **THEN** every line, corner, junction and arrowhead SHALL be drawn with
  ASCII characters only

#### Scenario: The source rendering
- **WHEN** the next-rendering action is triggered again
- **THEN** the artifact's Mermaid text SHALL be shown in place of the
  drawing

### Requirement: The subset of Mermaid that architecture is written in is drawn
The surface SHALL draw flowcharts (`flowchart` / `graph`, with nested
subgraphs, node shapes, solid, dotted and thick edges, and edge labels),
the C4 family (persons, systems, containers, components, their database
and external variants, boundaries, deployment nodes and relations), and
sequence diagrams (participants and messages in order). Styling
statements, and a C4 macro the surface does not know, SHALL be ignored
without failing the rest of the diagram. A statement that cannot be read
SHALL fail that one artifact, with the reason shown in its place, and
never the surface.

#### Scenario: A flowchart with nested subgraphs
- **WHEN** a flowchart nests one subgraph inside another
- **THEN** each SHALL be drawn as a titled frame around the boxes it
  holds, the inner inside the outer

#### Scenario: Two edges meet
- **WHEN** two edges share a stretch of line or cross
- **THEN** the cell where they meet SHALL be drawn as a junction or a
  crossing, not as whichever edge was drawn last

#### Scenario: An external C4 element
- **WHEN** a C4 view holds a `System_Ext`
- **THEN** it SHALL be drawn visibly fainter than the system's own
  elements

#### Scenario: A label with nowhere to go
- **WHEN** an edge's label fits neither on its line nor beside it
- **THEN** the label SHALL be left off rather than written over a line or
  a box

#### Scenario: A styling statement
- **WHEN** a diagram holds `classDef`, `style`, `linkStyle` or
  `UpdateLayoutConfig`
- **THEN** the statement SHALL be ignored and the diagram still drawn

#### Scenario: A statement that cannot be read
- **WHEN** a C4 relation names only one end
- **THEN** the surface SHALL say the artifact could not be drawn and quote
  the statement
- **AND THEN** every other artifact SHALL still open

#### Scenario: An edge that cannot be routed
- **WHEN** no path exists for an edge
- **THEN** the rest SHALL still be drawn, and the footer SHALL say how
  many edges were not routed

### Requirement: The board moves freely, by pointer and by key
The drawing SHALL sit on a board larger than itself. The board SHALL be
moved by dragging it with the pointer, by the wheel in both axes, by the
four pan actions, and by page up and page down. It SHALL be free to move
until the drawing's edge reaches the middle of the view, so that any box
can be brought to the middle. An artifact SHALL open centred when it fits
the view. The board's empty space SHALL be marked by a faint dot grid that
moves with the drawing and never shows through a box or a label.

#### Scenario: Dragging the board
- **WHEN** the pointer is pressed on the board, moved by ten columns and
  released
- **THEN** the drawing SHALL have moved by ten columns with it
- **AND THEN** the release SHALL NOT be treated as a click on a box

#### Scenario: Bringing a corner box to the middle
- **WHEN** the board is moved as far as it goes toward a corner of the
  drawing
- **THEN** that corner of the drawing SHALL be at the middle of the view

#### Scenario: A small diagram
- **WHEN** an artifact smaller than the view is opened
- **THEN** it SHALL be drawn centred in the view

#### Scenario: The pointer is released outside the surface's content
- **WHEN** a drag ends
- **THEN** no part of the gesture SHALL reach the pane under the surface

### Requirement: A minimap of a fixed size says where the view is
The surface SHALL show a minimap of the whole drawing in a corner of the
board, with the part currently in view marked on it. The minimap SHALL be
the same size for every artifact. A click on it SHALL move the view to the
place clicked. A view too small to spare the room, and the source
rendering, SHALL show none.

#### Scenario: Two artifacts of very different sizes
- **WHEN** a small and a large artifact are each opened in the same view
- **THEN** their minimaps SHALL occupy the same number of cells

#### Scenario: A click on the minimap
- **WHEN** the far end of the minimap is clicked
- **THEN** the view SHALL move so the matching part of the drawing is at
  its middle

#### Scenario: A view too small for it
- **WHEN** the surface is drawn in a view smaller than the minimap needs
- **THEN** no minimap SHALL be shown, and the board SHALL still move

### Requirement: A selected box lights what it connects to
Selecting a box SHALL draw it, every edge that touches it and those
edges' labels in the accent role, and the footer SHALL name the box and
say how many edges come into it and go out of it. A box SHALL be selected
by a click, and by the four directional selection actions, which move to
the nearest box in that direction and bring it into view. With nothing
selected the footer SHALL say how many boxes and edges the drawing holds.

#### Scenario: A box is clicked
- **WHEN** a box with two incoming and three outgoing edges is clicked
- **THEN** the box and those five edges SHALL be lit
- **AND THEN** the footer SHALL read its title, `2 in` and `3 out`

#### Scenario: Selecting toward a direction
- **WHEN** a box is selected and the select-right action is triggered
- **THEN** the nearest box to its right SHALL become the selection
- **AND THEN** the board SHALL move if that box was out of view

#### Scenario: Clicking the selection again
- **WHEN** a selected box that leads nowhere is clicked again
- **THEN** the selection SHALL be cleared

### Requirement: C4 views are levels that can be walked, down to the code
A box in a C4 artifact SHALL lead **inside** when another C4 artifact of
the project draws a boundary with the same alias; only C4 artifacts SHALL
be joined this way. A box SHALL lead **to the code** when it carries a
Mermaid `$link` (C4) or a `click … href` (flowchart) naming a path in the
project. A box that leads somewhere SHALL carry a mark in its border
saying which of the two, and the footer SHALL say what activating it does.
Activating a selected box, clicking its mark, or clicking it a second time
SHALL follow it. Going inside SHALL add the box to a trail shown in the
menu; the level-up action and a click on a trail entry SHALL go back, with
the box that was entered selected and in view. Following a box to the code
SHALL open the code surface on that path.

#### Scenario: Going inside a container
- **WHEN** the container view holds a box `core`, the component view holds
  `Container_Boundary(core, …)`, and `core` is selected and activated
- **THEN** the component view SHALL open
- **AND THEN** the menu SHALL show the trail from the container view to it

#### Scenario: Coming back up
- **WHEN** the level-up action is triggered one level inside
- **THEN** the container view SHALL open with `core` selected and in view
- **AND THEN** the trail SHALL be gone

#### Scenario: Down to the code
- **WHEN** a component carries `$link="crates/uze-core/src/store.rs"` and
  is activated
- **THEN** the code surface SHALL open showing that file's contents

#### Scenario: A flowchart that reuses a container's name
- **WHEN** a flowchart holds a subgraph named `core`
- **THEN** the C4 box `core` SHALL NOT lead into that flowchart

#### Scenario: A box that leads nowhere
- **WHEN** a box with no matching boundary and no link is activated
- **THEN** nothing SHALL happen, and the box SHALL carry no mark

#### Scenario: Choosing an artifact from the menu while inside
- **WHEN** an artifact is opened from a selector while a trail is shown
- **THEN** the trail SHALL be cleared

### Requirement: The surface is reachable the way the code surface is
The surface SHALL open by a `toggle-architect` action bound in the
workspace and in the code surface, and by a chip in the tab strip; the
same action SHALL close it. While it is open its scope SHALL seal the
keyboard from the pane underneath. From it, the changes and files entry
points SHALL open the code surface directly. Every key the surface
answers to SHALL be a named, rebindable action listed in the action index,
and the surface's footer SHALL advertise keys read from the keymap.

#### Scenario: Opening from the tab strip
- **WHEN** the architect chip in the tab strip is clicked
- **THEN** the surface SHALL open over the workspace

#### Scenario: A key typed while the surface is open
- **WHEN** a letter bound to nothing in the architect scope is typed
- **THEN** it SHALL NOT reach the pane underneath

#### Scenario: A rebound key
- **WHEN** the user's keymap binds `choose-artifact` to another chord
- **THEN** the footer SHALL show that chord

### Requirement: Reading the artifacts never blocks the workspace
Resolving the project's declaration and reading its artifact files SHALL
happen off the thread that draws. While it is outstanding the surface
SHALL be open and say it is reading. An answer SHALL carry the project it
was asked for, and one that arrives after the surface was closed or
reopened for another project SHALL be dropped.

#### Scenario: A slow filesystem
- **WHEN** the surface is opened and the artifact directory takes seconds
  to read
- **THEN** the workspace SHALL keep drawing and taking input meanwhile
- **AND THEN** the surface SHALL say it is reading the project's artifacts

#### Scenario: The surface was closed before the answer
- **WHEN** the surface is closed while the read is outstanding
- **THEN** the late answer SHALL be dropped and nothing SHALL reopen
