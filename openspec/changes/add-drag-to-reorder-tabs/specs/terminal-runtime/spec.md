## ADDED Requirements

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
