## Purpose

Tells the operator, by a discreet sound they opted into, that an agent in the
workspace finished its turn, so work done out of sight is not found late.

## ADDED Requirements

### Requirement: The operator chooses which finished turns make a sound

The operator SHALL have two ways to make the same choice, writing the same
record: the workspace client's own control and the CLI —
`uze config notification on|off|silent` (the grammar change names the CLI;
this change carries the choice itself). Three choices for a finished agent
turn exist: Silent, Out of sight, and Always, where `on` is Always and `off`
is Out of sight. A machine on which the operator never chose
SHALL behave as Silent. The choice SHALL be machine-scoped: it SHALL apply to
every workspace on the machine and SHALL NOT be written into any project. It
SHALL be kept in the operator's settings file, `config.toml` at the root of
the UZE home, as a value the operator can also write by hand; a value UZE does
not recognise SHALL behave as Silent. `uze config notification test` SHALL
ring once regardless of the choice in force, so the person can hear what
their machine will sound like.

#### Scenario: A fresh machine is silent

- **WHEN** an agent's turn ends on a machine where no choice was ever made
- **THEN** no sound is made

#### Scenario: A hand-written choice is honoured

- **WHEN** the operator writes `agent_finished = "always"` under
  `[notifications]` in `config.toml`
- **THEN** the next launch rings for every finished turn

#### Scenario: The choice survives a restart

- **WHEN** the operator chooses Always and then quits and relaunches `uze`
- **THEN** Always is still the choice in force and is shown as such

#### Scenario: The CLI sets the same choice the workspace client sets

- **WHEN** the operator runs `uze config notification off`
- **THEN** `config.toml` holds the value the workspace client's own control
  would have written, and both surfaces show the same choice as in force

#### Scenario: Test rings whatever the choice says

- **WHEN** the operator runs `uze config notification test` on a machine
  whose choice is Silent
- **THEN** the bell rings once, and the choice in force is unchanged

### Requirement: The sound is the terminal's own bell

A ring SHALL be the terminal bell control character written to the terminal
the client draws on, so that what it sounds like — a tone, a flash, nothing —
is decided by the operator's terminal settings. UZE SHALL NOT play audio
itself.

#### Scenario: A ring reaches the host terminal

- **WHEN** a finished turn qualifies for a ring
- **THEN** the terminal receives exactly one bell character for it, and the
  screen drawn around it is unchanged

### Requirement: Which turns ring follows the choice

A turn's end SHALL be the same moment the sidebar stops showing the agent as
working, and a turn SHALL ring only once it has stayed ended for a settle
window of about ten seconds: an agent that resumes work inside that window
SHALL NOT ring for the pause. With Out of sight, a settled turn SHALL ring
only while its tab still carries the finished check — that is, it ended out
of sight and has not been looked at since. With Always, every settled turn
SHALL ring. With Silent, none SHALL.

#### Scenario: A pause the agent resumes from is silent

- **WHEN** the choice is Always and an agent stops repainting long enough for
  its spinner to stop, then resumes work within the settle window
- **THEN** no sound is made

#### Scenario: A background tab looked at before it settles is silent

- **WHEN** the choice is Out of sight, an agent finishes in another tab, and
  the operator switches to that tab within the settle window
- **THEN** no sound is made

#### Scenario: Out of sight ignores the tab on screen

- **WHEN** the choice is Out of sight and the agent in the focused tab
  finishes its turn
- **THEN** no sound is made

#### Scenario: Out of sight rings for a background tab

- **WHEN** the choice is Out of sight and an agent in another tab finishes
  its turn and stays idle through the settle window
- **THEN** the bell rings once

#### Scenario: Always rings for the tab on screen

- **WHEN** the choice is Always and the agent in the focused tab finishes
  its turn and stays idle through the settle window
- **THEN** the bell rings once

### Requirement: Rings do not pile up

Turns ending close together SHALL ring at most once within a short cooldown
window, so several agents finishing together make one sound, not a burst.

#### Scenario: Two agents finish together

- **WHEN** the choice is Always and two agents finish within the same
  cooldown window
- **THEN** the bell rings once

### Requirement: The choice is made in the Manage modal

The Manage modal's Settings screen SHALL list the three choices as a group
of their own, titled Notifications, marking the one in force. Choosing one SHALL take effect in the
running workspace at once, without a restart, and choosing a ringing option
SHALL ring once so the operator hears what they chose.

#### Scenario: Choosing takes effect immediately

- **WHEN** the operator selects Always in the Settings screen and closes
  the modal
- **THEN** the next finished turn rings, with no relaunch in between

#### Scenario: Choosing a ringing option previews it

- **WHEN** the operator selects Out of sight or Always
- **THEN** the bell rings once at that moment

### Requirement: The operator's settings are one file they can edit

UZE SHALL keep the operator's machine settings — the theme, the glyph set,
and when finished agents ring — in one TOML file, `config.toml`, at the root
of the UZE home. Writing one setting SHALL leave every other part of the file
unchanged, including comments and sections this build does not know. A file
that does not parse SHALL be reported and SHALL NOT be written over.

#### Scenario: Choosing a theme keeps the operator's comments

- **WHEN** `config.toml` holds a comment and a section UZE does not know, and
  the operator chooses a theme
- **THEN** the file holds the new theme and the comment and the unknown
  section exactly as they were

#### Scenario: A broken file is not overwritten

- **WHEN** `config.toml` does not parse and the operator chooses a setting
- **THEN** the choice is refused with the file named, and the file is left as
  it was
