# action-discovery Specification

## Purpose
How someone performs an action without knowing a key: what an entity says
can be done to it, the surfaces that render that, the one index of
everything the product can do, and the rule that keeps the keyboard an
accelerator rather than a prerequisite.
## Requirements
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
- **WHEN** the same entity's actions are shown in its detail drawer and in
  the index of all actions
- **THEN** both show the same set, because both read the same offers

#### Scenario: Only what can be done now is a button
- **WHEN** an action cannot be performed on the selected entity
- **THEN** its detail view draws no button for it, and the status at the
  foot of the view says where the entity stands

#### Scenario: A destructive offer is drawn as one
- **WHEN** offers are rendered for an entity
- **THEN** a destructive one comes last, in the danger colour, and
  performing it still requires its confirmation

### Requirement: Actions are offered where the thing they act on is
A screen that acts on rows SHALL offer the selected row's actions as
buttons at the foot of its detail view, beneath the row's status — one
place, the same on every screen. Discovering an action SHALL NOT require
knowing a letter.

#### Scenario: A selected entity shows what can be done to it
- **WHEN** an entity's detail view is open
- **THEN** what can be done to it now is a row of buttons at its foot, so a
  selection always states its own possibilities

#### Scenario: A button reads as one
- **WHEN** the pointer rests on a button
- **THEN** it turns from a soft tint of its colour to the full colour, so
  what can be clicked answers the pointer before it is clicked

#### Scenario: The whole flow works with the pointer alone
- **WHEN** an operator installs, updates and removes a package using only
  the mouse
- **THEN** every step of it — finding, filtering, selecting, acting,
  confirming — has a target, including the search field

### Requirement: A surface never hides what it offers
A list taller than its window SHALL be walkable to its end by every gesture
that walks it, and SHALL say where in it the window sits. A surface that
answers for the whole screen SHALL make that visible rather than leaving
the screen behind it looking as live as it was a frame earlier.

#### Scenario: A list longer than the screen can still be reached
- **WHEN** a list has more rows than the column it is drawn in
- **THEN** the window follows the selection, so no row is out of view and
  unreachable at the same time
- **AND** the heading a row belongs under travels with it

#### Scenario: Every gesture that walks a list walks the same list
- **WHEN** the wheel is turned over a list
- **THEN** it moves what the arrow keys move, on every screen that has a
  list at all

#### Scenario: A long list says how long it is
- **WHEN** a list does not fit its window
- **THEN** it shows where in the whole the window sits, and that indicator
  answers a click and a drag rather than being decoration
- **AND** a list that fits shows none

#### Scenario: A modal is visibly modal
- **WHEN** a surface is open that nothing behind it will answer until it is
  dealt with
- **THEN** the screen behind it recedes, so the depth is visible rather
  than asserted by a border

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
  again; once every step has been taken it also offers to leave for good
- **AND** it shares the foot of the column with the history beside it —
  each pushing the other rather than covering it, and one open at a time

#### Scenario: A key that has nothing to do here says so
- **WHEN** an action is offered on a screen where it has nothing to act on
- **THEN** performing it answers with the reason rather than doing nothing,
  and it is not recorded as a step that was taken

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

