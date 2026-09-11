## ADDED Requirements

### Requirement: Contextual git changes view
The workspace TUI client SHALL provide a read-only git changes view,
reachable from any point in the terminal workspace, that lists files
changed (relative to `HEAD`, including untracked files) in the currently
active tab's live working directory and shows the diff of a selected file.

#### Scenario: User opens the git changes view
- **WHEN** a user triggers the git changes view while a terminal tab is
  active
- **THEN** the client SHALL list files changed in that tab's current
  working directory
- **AND THEN** the client SHALL show the diff of the first listed file

#### Scenario: User selects a different changed file
- **WHEN** a user selects another file in the changed-files list
- **THEN** the client SHALL show that file's diff in place of the previous
  one

#### Scenario: Selected working directory has no changes
- **WHEN** a user opens the git changes view for a tab whose working
  directory has no uncommitted or untracked changes
- **THEN** the client SHALL show an empty changed-files list rather than an
  error

#### Scenario: Working directory is outside a git repository
- **WHEN** a user opens the git changes view for a tab whose working
  directory is not inside a git repository
- **THEN** the client SHALL show that condition instead of a changed-files
  list

### Requirement: Non-disruptive dismissal
The git changes view SHALL be dismissible back to exactly the workspace
state the user was in before opening it, without ending, resizing, or
otherwise disrupting any pane's running process.

#### Scenario: User dismisses the git changes view
- **WHEN** a user closes the git changes view
- **THEN** the client SHALL return to the terminal workspace exactly as it
  was before the view opened
- **AND THEN** every pane's process SHALL be unaffected by the view having
  been open

### Requirement: Read-only git changes view
The git changes view SHALL only display changed files and diffs. It SHALL
NOT stage, unstage, commit, or discard any change.

#### Scenario: User views a diff
- **WHEN** a user views a file's diff in the git changes view
- **THEN** the client SHALL make no modification to the working directory,
  the index, or the git repository
