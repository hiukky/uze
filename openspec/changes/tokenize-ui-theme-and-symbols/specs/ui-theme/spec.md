## Purpose

Makes UZE's appearance data rather than code: one vocabulary of colour
tokens and named symbols that every surface UZE draws resolves against, and
one theme file anyone can write to change that appearance without building
the binary.

## ADDED Requirements

### Requirement: A theme is a file, and a partial theme is a valid theme

UZE SHALL read themes from files under the UZE home's themes directory, one
theme per file, identified by the file's own name. A theme file SHALL be
able to declare any subset of the vocabulary; every token and symbol it does
not declare SHALL resolve to the built-in default theme's value. A theme
SHALL NOT be required to declare anything at all in order to load.

#### Scenario: A theme declaring one token changes only that token

- **WHEN** a theme file declares a value for exactly one colour token and is
  made active
- **THEN** that token resolves to the declared value, every other token and
  every symbol resolves to the built-in default's value, and no surface
  fails to draw

#### Scenario: An unreadable or malformed theme never breaks the UI

- **WHEN** the active theme's file is absent, unparseable, or declares a
  token name that does not exist in the vocabulary
- **THEN** UZE reports the problem by name (the file and what is wrong with
  it) and continues running on the built-in default theme, rather than
  refusing to start or drawing an unstyled screen

#### Scenario: A theme file is validated against a published schema

- **WHEN** a theme file is loaded
- **THEN** it is validated against the theme schema UZE publishes, and a
  value of the wrong shape is reported as a named error against that schema

### Requirement: Colour is named by meaning, never by value, everywhere UZE draws

Every colour UZE draws SHALL be selected by a semantic token — what the
coloured thing means — and resolved against the active theme at draw time.
No surface SHALL carry a colour value of its own, and no surface SHALL carry
its own copy of the vocabulary.

#### Scenario: One token change reaches every surface at once

- **WHEN** the active theme changes the value of a token used by TUI chrome,
  by CLI output, and by the pane's default foreground
- **THEN** all three surfaces draw the new value, with no surface left on
  the previous one

#### Scenario: A theme can define any colour

- **WHEN** a theme declares a token as an opaque colour, as a translucent
  colour over the theme's own background, or as an alias of another token
- **THEN** the token resolves to a single concrete colour in every case, and
  a translucent declaration resolves to the same colour a hand-composited
  opaque declaration of the same value would

### Requirement: Symbols are a named set a theme can replace

Every glyph UZE draws as chrome — status marks, tree and divider glyphs,
spinner frames, affordance indicators — SHALL be referenced by a name from a
published symbol vocabulary and resolved against the active theme. A theme
SHALL be able to replace any symbol's glyph.

#### Scenario: A theme replaces a status glyph

- **WHEN** the active theme declares its own glyph for a named symbol
- **THEN** every place that symbol is drawn shows the theme's glyph, and the
  symbol's colour token is unaffected

#### Scenario: A pure-ASCII theme is usable on a terminal with no Unicode font

- **WHEN** the built-in ASCII theme is active
- **THEN** every symbol UZE draws is within ASCII, and no layout depends on
  a glyph the terminal cannot render

#### Scenario: A replaced symbol does not break alignment

- **WHEN** a theme declares a symbol whose display width differs from the
  default's
- **THEN** UZE lays the containing row out from the resolved glyph's actual
  width, so columns stay aligned

### Requirement: The active theme is a machine-scoped selection that applies without restart

UZE SHALL record which theme is active as machine-scoped state, and SHALL
apply it to every surface it draws. Selecting a different theme SHALL take
effect in a running TUI without restarting it.

#### Scenario: Selecting a theme applies it to the running TUI

- **WHEN** the operator selects a different theme from within the TUI
- **THEN** the next frame is drawn in that theme, the selection is recorded,
  and no session, pane, or agent is disturbed

#### Scenario: The selection survives across invocations

- **WHEN** a theme is selected and UZE is later run again
- **THEN** that theme is active, in both the TUI and the CLI

#### Scenario: The operator can see what is available and what is active

- **WHEN** the operator asks UZE which themes exist
- **THEN** UZE lists every theme it can load, including the built-in ones,
  and identifies which is active

### Requirement: A pane reports the colours it is actually drawn in

A program running inside a UZE terminal pane that asks the terminal for its
default foreground or background SHALL be told the colours the active theme
actually draws, and an indexed colour a program emits SHALL resolve through
the active theme's own set of those colours.

#### Scenario: A program probing the background gets the active theme's value

- **WHEN** a program inside a pane queries the terminal's default background
  and the active theme is not the default one
- **THEN** the answer is the active theme's background, not a compiled-in
  value

#### Scenario: Indexed colours follow the theme

- **WHEN** a program inside a pane emits an indexed terminal colour
- **THEN** that cell is drawn in the active theme's value for that index

### Requirement: Content that carries its own palette stays legible under any theme

Content UZE renders that carries a palette of its own rather than the
chrome's — syntax-highlighted diff content is the case today — SHALL be
selected by the active theme rather than fixed, so a theme cannot leave that
content unreadable against its own background.

#### Scenario: A light theme does not leave highlighted content unreadable

- **WHEN** a theme intended for a light background is active
- **THEN** syntax-highlighted content is rendered with the palette that
  theme names for it, not with a palette chosen for a dark background

### Requirement: Nothing an extension draws can name a colour or a glyph outside the vocabulary

An extension SHALL continue to describe its content in semantic terms only.
Introducing the theme vocabulary SHALL NOT give an extension a way to name a
chrome colour, and SHALL NOT require an extension to know the theme schema.

#### Scenario: An extension's chrome follows the active theme

- **WHEN** an extension's view is drawn under a non-default theme
- **THEN** its chrome is drawn from that theme, resolved by the host, with
  no change to what the extension itself produced

#### Scenario: Content with its own colours is still allowed through

- **WHEN** an extension supplies a colour for content that carries its own
  palette
- **THEN** that colour is drawn as given, and it remains the only way an
  extension can put a specific colour on screen
