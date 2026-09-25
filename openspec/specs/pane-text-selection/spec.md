# pane-text-selection Specification

## Purpose

Lets the operator select a pane's text with the pointer and have it copied on
release, with the same gesture and the same result in every pane whatever
program runs in it.

## Requirements

### Requirement: Dragging over a pane selects its text

The workspace client SHALL start a selection when the left button is pressed
over a pane and SHALL extend it, as the pointer moves with the button held,
from the cell the press landed on to the cell under the pointer, in reading
order: whole rows between the first and the last, partial rows at either end.
A drag that leaves the pane SHALL select up to the pane's edge. The selection
SHALL be drawn over the pane as it grows, in a way that stays legible over any
colour the pane's content uses. A press that is released without having moved
to another cell SHALL select nothing.

#### Scenario: A drag across rows

- **WHEN** the operator presses on the seventh cell of a pane's first row and
  releases on the third cell of its third row
- **THEN** the selection covers the rest of the first row, the whole second
  row, and the first three cells of the third

#### Scenario: A drag backwards

- **WHEN** the operator drags from a later cell to an earlier one
- **THEN** the same cells are selected as a drag from the earlier to the later

#### Scenario: A click selects nothing

- **WHEN** the operator presses and releases on the same cell
- **THEN** nothing is selected and nothing is copied

### Requirement: Releasing a selection copies it

When the button is released over a non-empty selection, the client SHALL put
the selected text on the system clipboard, with no further key or command,
and SHALL say so in a toast that names how much was copied. The text SHALL
carry one line per selected row, SHALL NOT carry the blanks a terminal pads
each row with nor blank rows at its end, and SHALL NOT carry the blank cell a
wide character spills into. A selection that covered only blanks SHALL copy
nothing and SHALL NOT be delivered to the pane's program as a click. The clipboard SHALL be reached through the terminal the operator is
using, so the copy lands on the machine the operator sits at when UZE runs on
another one or under WSL.

#### Scenario: Copying a word

- **WHEN** the operator drags across `hello` on a row reading `hello world`
  and releases
- **THEN** the clipboard holds `hello`
- **AND** a toast says five characters were copied

#### Scenario: A drag over blanks copies nothing

- **WHEN** the operator drags across cells that hold only blanks
- **THEN** nothing is copied and no toast is raised
- **AND** a program that asked for mouse reports receives nothing

#### Scenario: Trailing padding is not copied

- **WHEN** a selection covers the ends of rows shorter than the pane
- **THEN** the copied lines end at their last non-blank character

### Requirement: A selection stays drawn until the next gesture

A copied selection SHALL stay drawn after the release, so the operator sees
what was taken, and SHALL be put away by the next press anywhere or the next
key.

#### Scenario: The next key puts it away

- **WHEN** the operator types after copying a selection
- **THEN** the selection is no longer drawn
- **AND** the key reaches the pane as it would have without a selection

### Requirement: Selection takes precedence over the pane's program

A drag over a pane SHALL be a selection even when the pane's program asked
for mouse reports, and the program SHALL NOT be told about any part of it.
The program SHALL keep its clicks: a press over a pane whose program asked for
mouse reports SHALL be held until the button comes up, and if it never moved
the press and the release SHALL both be delivered to the program then. The
wheel SHALL reach the program as before.

#### Scenario: A drag over a program that owns the mouse

- **WHEN** a pane's program has asked for mouse reports and the operator
  drags across its text
- **THEN** the text is copied
- **AND** the program receives no mouse report for the press, the drag or the
  release

#### Scenario: A click still reaches the program

- **WHEN** a pane's program has asked for mouse reports and the operator
  presses and releases on one cell
- **THEN** the program receives nothing while the button is down
- **AND** on release it receives the press and the release at that cell
- **AND** nothing is copied

### Requirement: Shift hands the drag to the pane's program

With Shift held, a press over a pane whose program asked for mouse reports
SHALL go to the program as it did before selection existed — press, drag and
release forwarded as they happen — and SHALL NOT start a selection. Over a
pane whose program did not ask for the mouse, Shift SHALL change nothing.

#### Scenario: Shift-dragging over a program that owns the mouse

- **WHEN** the operator holds Shift and drags over a pane whose program asked
  for mouse reports
- **THEN** the program receives the press, the drag and the release
- **AND** nothing is selected or copied
