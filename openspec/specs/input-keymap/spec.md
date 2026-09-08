# input-keymap Specification

## Purpose
How a keystroke becomes an action — the vocabulary the product names, the
scope that decides which action is live, the file an operator writes, what
a terminal can actually deliver, and the reverse lookup that makes every
key uze advertises true by construction.
## Requirements
### Requirement: An action is named once, and the key that reaches it is a separate question
The system SHALL name every keyboard-reachable action in one vocabulary,
independent of any physical key. Dispatch SHALL resolve an input event to
an action and act on the action; no dispatcher SHALL branch on a physical
key directly.

#### Scenario: One vocabulary serves every surface
- **WHEN** the same meaning exists in more than one surface — quit, help,
  refresh, close
- **THEN** it is one action with one name, and each surface differs only in
  which chord reaches it

#### Scenario: A surface cannot invent a key
- **WHEN** presentation code is inspected
- **THEN** no module outside the single terminal adapter names a physical
  key, except the encoder that translates a key into the bytes a pane's
  program expects, which binds nothing

#### Scenario: An action exists whether or not it has a chord
- **WHEN** an action is offered by a button and bound to no chord
- **THEN** it is still a member of the vocabulary, still listed where
  actions are listed, and still bindable by the operator

### Requirement: Scope decides where an action is live, never what a chord means
Actions SHALL resolve against a stack of scopes derived from what is
currently open, innermost first. Within one mode a *mnemonic* — a chord
whose key is a character, a digit or a function key — SHALL name exactly
one action, whatever the scopes involved. A structural key (Enter, Escape,
Tab, an arrow) is contextual by nature and is exempt: its meaning is read
from the surface in front of the reader, and no one memorizes it.

#### Scenario: The innermost open surface answers first
- **WHEN** a modal surface is open and a chord bound outside it is pressed
- **THEN** the modal surface's binding wins if it has one, and the outer
  binding does not fire

#### Scenario: Modal precedence is uniform
- **WHEN** any modal surface is open
- **THEN** every binding outside it and outside the global scope behaves
  the same way — none fires, and none fires merely because it happened to
  be tested earlier than the modal's own arm

#### Scenario: Leaving and asking for help survive a sealed surface
- **WHEN** a modal surface is open
- **THEN** the global actions — switching mode, leaving, and opening the
  index — still resolve, which is what makes sealing everything else safe

#### Scenario: The pane is the last scope and it is total
- **WHEN** a key is pressed with nothing of uze's open and no binding
  matches it
- **THEN** it reaches the pane's program unchanged

#### Scenario: A mnemonic means one thing per mode
- **WHEN** the built-in keymap is validated
- **THEN** no mnemonic in a mode resolves to two different actions, and a
  keymap that introduces such a pair is reported as a conflict rather than
  silently resolved by order

#### Scenario: A structural key stays contextual
- **WHEN** Enter opens a row on one screen and confirms a dialog on
  another
- **THEN** that is not a conflict, and nothing asks anyone to remember
  which is which

### Requirement: The keymap is a resolved file, partial over a built-in default
The system SHALL ship a built-in default keymap and SHALL resolve an
operator's file over it, keeping only what differs. A file that names an
action or a scope this build does not know SHALL load with a warning; a
file whose chord cannot be parsed, or that binds two actions to one chord,
SHALL be reported as an error and the previous keymap SHALL stay in force.

#### Scenario: An operator's file carries only their differences
- **WHEN** an operator rebinds one action
- **THEN** their file names that binding alone, and every other action
  keeps whatever the built-in default says — including after the default
  changes in a later release

#### Scenario: A keymap written for a newer uze still loads
- **WHEN** a file names an action this build has never heard of
- **THEN** the rest of the file is applied and the unknown name is
  reported as a warning, not a failure to load

#### Scenario: A broken file never leaves the operator without a keyboard
- **WHEN** a file cannot be resolved
- **THEN** the keymap already in force stays in force, and the problem is
  reported with the entry that caused it

#### Scenario: The keymap is machine-scoped
- **WHEN** an operator's keymap is located
- **THEN** it belongs to the machine, beside the other authored appearance
  intent, and no project can change what the operator's keys do

### Requirement: A chord is refused when a terminal cannot deliver it as a chord
The chord grammar SHALL refuse chords that a terminal encodes as some other
key, naming which key they collide with. It SHALL classify every bindable
chord by what it takes to deliver it, and SHALL NOT offer a chord requiring
a capability the running terminal lacks.

#### Scenario: A chord that is another key is refused
- **WHEN** an operator binds a control chord that a terminal transmits as
  Tab, Enter, Backspace, line feed, Escape or NUL
- **THEN** it is refused with the key it actually is, before anything is
  written

#### Scenario: A chord the running terminal cannot send is not offered as available
- **WHEN** a chord requires the keyboard enhancement protocol and the
  running terminal does not support it
- **THEN** it is shown as unavailable with the reason, rather than bound
  and silently dead

#### Scenario: A chord the host is likely to take first is annotated
- **WHEN** a chord is one a terminal emulator or multiplexer commonly
  claims before an application sees it
- **THEN** that is stated where the chord is shown, so it is a choice
  rather than a surprise

#### Scenario: An operator can find out what their own terminal delivers
- **WHEN** an operator presses a chord at the probe
- **THEN** uze reports whether it arrived and what it resolved to, which is
  the answer for the terminal, multiplexer and connection actually in use

#### Scenario: Enhanced input never changes what a pane receives
- **WHEN** the running terminal supports the keyboard enhancement protocol
  and uze enables it
- **THEN** a keystroke forwarded into a pane arrives exactly once and in
  the encoding that pane's program expects

### Requirement: Every key uze advertises comes from the keymap
Any surface that names a key SHALL obtain it from the keymap by asking for
an action's chord. No advertised key SHALL be written as text.

#### Scenario: Rebinding updates every surface that mentions the key
- **WHEN** an operator rebinds an action
- **THEN** the help, the footer hints, and any other place that named that
  key show the new chord without a rebuild

#### Scenario: An unbound action is never advertised as bound
- **WHEN** an action has no chord in the current keymap
- **THEN** no surface prints a key for it, and it is still reachable by
  pointer and by the index of all actions

#### Scenario: Hints cannot drift from dispatch
- **WHEN** the sources are inspected
- **THEN** no rendering module contains a chord written as a literal

