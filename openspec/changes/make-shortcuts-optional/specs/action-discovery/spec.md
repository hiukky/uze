## Purpose

How someone performs an action without knowing a key: what an entity says
can be done to it, the surfaces that render that, the one index of
everything the product can do, and the rule that keeps the keyboard an
accelerator rather than a prerequisite.

## ADDED Requirements

### Requirement: Every chord has a pointer affordance
An action bound to a chord outside the pane scope SHALL be reachable with
the pointer — a button, a row, a menu entry, or the index of all actions.
An action MAY be reachable by pointer and by no chord at all.

#### Scenario: A bound action is reachable without the keyboard
- **WHEN** the keymap is checked against the surfaces
- **THEN** every action bound outside the pane scope names the affordance
  that performs it, or is declared keyboard-only with a written reason

#### Scenario: An affordance needs no chord
- **WHEN** an action is offered by a button and bound to nothing
- **THEN** that is a complete design, and nothing requires a chord to be
  invented for it

### Requirement: An entity carries what can be done to it
The read model for an entity a screen acts on SHALL carry the offers
available for it: the action, its label, whether it can be performed now
and why not, and whether performing it is destructive. Presentation SHALL
NOT decide what is possible.

#### Scenario: One list, every surface
- **WHEN** the same entity's actions are shown in a row menu, in its detail
  drawer, and in the index of all actions
- **THEN** all three show the same set, because all three read the same
  offers

#### Scenario: An unavailable action is explained, never silent
- **WHEN** an action cannot be performed on the selected entity, and other
  actions can
- **THEN** it is absent from the row menu and present in the detail view
  with the reason, and no gesture appears to do nothing

#### Scenario: A row nothing can be done to still answers
- **WHEN** nothing at all can be done to the selected entity
- **THEN** asking for its actions still opens something, listing what
  cannot be done and why, rather than opening nothing — which would read
  exactly like the silent no-op this replaces

#### Scenario: A destructive offer is never the first thing under the pointer
- **WHEN** offers are rendered for an entity
- **THEN** a destructive one is not the default-highlighted entry, and
  performing it still requires its confirmation

### Requirement: Actions are offered where the thing they act on is
A screen that acts on rows SHALL offer that row's actions from the row
itself and from its detail view. Discovering an action SHALL NOT require
knowing a letter.

#### Scenario: A row offers its own actions
- **WHEN** a row's action affordance is used, or the row is right-clicked
- **THEN** the actions available for that row are listed, navigable, and
  confirmed by selection

#### Scenario: A selected entity shows what can be done to it
- **WHEN** an entity's detail view is open
- **THEN** its actions are shown there as well, so a selection always
  states its own possibilities

#### Scenario: The whole flow works with the pointer alone
- **WHEN** an operator installs, updates and removes a package using only
  the mouse
- **THEN** every step of it — finding, filtering, selecting, acting,
  confirming — has a target, including the search field

### Requirement: One index of everything, reachable by key and by button
The system SHALL provide one surface listing every action live in the
current context, with each action's own chord, filterable, and performable
from the list itself. It SHALL be reachable by a chord that works in both
modes and by a persistent on-screen control in both modes.

#### Scenario: The index is the help
- **WHEN** an operator looks for what the product can do, or for what a key
  does
- **THEN** one surface answers both, and there is no second list to
  disagree with it

#### Scenario: The first steps are listed, and what has been taken is marked
- **WHEN** an operator opens either mode
- **THEN** the foot of the sidebar lists a few things worth trying once,
  each with the key that reaches it and each performable from the list
- **AND** a step the operator has already taken — by any route — is marked
  as taken, and the count says how many remain
- **AND** the list folds to its header, and stays folded until it is opened
  again
- **AND** it shares the foot of the column with the history beside it —
  each pushing the other rather than covering it, and one open at a time

#### Scenario: Acting from the index teaches the chord
- **WHEN** an operator performs an action from the index
- **THEN** the row that performed it showed the chord that reaches it
  directly

#### Scenario: Reachable where the keyboard belongs to something else
- **WHEN** the operator is attached to a pane whose program owns ordinary
  keys
- **THEN** the index is still reachable, both by its chord and by an
  on-screen control

#### Scenario: The index reflects the context it was opened from
- **WHEN** the index is opened
- **THEN** it lists what is live now, and does not offer actions that
  belong to a surface that is not open
