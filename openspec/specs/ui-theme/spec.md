# ui-theme Specification

## Purpose

Makes UZE's appearance data rather than code: one vocabulary of colour
tokens and named symbols that every surface UZE draws resolves against, and
one theme file anyone can write to change that appearance without building
the binary.
## Requirements
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

### Requirement: A theme can be a variation of another, and the operator's own overrides outlast whichever theme is on

A theme SHALL be able to name another theme it varies, inheriting every
declaration that theme makes and overriding only what it states itself.
Resolution SHALL merge declarations rather than resolved values, so an
ancestor's references still follow a descendant that repaints what they
point at. A chain that loops back on itself SHALL be refused, naming the
loop.

Separately from any theme, the operator SHALL be able to declare overrides
that apply over whichever theme is active, and those SHALL keep applying
when the active theme changes.

#### Scenario: A variation states only what it changes

- **WHEN** a theme names another theme it varies and declares one colour
- **THEN** every other colour and symbol resolves from the theme it varies
  (and, beneath that, from the selected glyph set and the built-in default),
  and any token that theme expressed as a reference to the changed colour
  follows the change

#### Scenario: A variation derives against its own background

- **WHEN** a variation changes only the background of the theme it varies
- **THEN** every surface and border its ancestors expressed as a separation
  from the background is recomputed against the new one

#### Scenario: A loop in the ancestry is refused

- **WHEN** a theme's ancestry leads back to a theme already in the chain, or
  goes further than UZE will follow
- **THEN** UZE reports it, naming the chain, and continues on the built-in
  default

#### Scenario: The operator's overrides survive a change of theme

- **WHEN** the operator declares overrides — the one glyph their own font
  draws differently, say — and then selects a different theme
- **THEN** the overrides still apply over the newly selected theme, without
  either theme having been edited

#### Scenario: UZE reports what a theme was assembled from

- **WHEN** the operator asks UZE about a theme
- **THEN** UZE names the layers it resolved from, in the order they applied,
  including the selected glyph set

### Requirement: A colour bound to a hue by contract does not follow a meaning

A colour whose consumer expects a specific hue SHALL NOT be derived from a
token whose meaning a theme is free to repaint. The indexed colours a
program inside a terminal pane names are such colours: index 2 is *green* to
the program that emits it, whatever the surrounding theme calls green.

#### Scenario: Repainting a meaning does not repaint the terminal's palette

- **WHEN** a theme paints the accent, or any other semantic colour, in a hue
  unlike the default's
- **THEN** the indexed colours a pane emits keep their own hues, except the
  four that are genuinely a role — the background, the foreground, and its
  dim and bright forms

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
  colour over the theme's own background, as a separation from that
  background in whichever direction is visible against it, as another
  token's value, or as another token's value at a given translucency
- **THEN** the token resolves to a single concrete colour in every case, and
  a translucent declaration resolves to the same colour a hand-composited
  opaque declaration of the same value would

#### Scenario: A tint follows the colour it tints

- **WHEN** a theme repaints a colour that another token is expressed as a
  translucent form of — the selected row's tint, a diff's wash
- **THEN** that token resolves from the repainted colour, not from whatever
  the built-in default's was

### Requirement: Symbols are a named set a theme can replace

Every glyph UZE draws as chrome — status marks, tree and divider glyphs,
spinner frames, affordance indicators — SHALL be referenced by a name from a
published symbol vocabulary and resolved against the active theme. A theme
SHALL be able to replace any symbol's glyph, and so SHALL a glyph set the
operator selects.

#### Scenario: A theme replaces a status glyph

- **WHEN** the active theme declares its own glyph for a named symbol
- **THEN** every place that symbol is drawn shows the theme's glyph, and the
  symbol's colour token is unaffected

#### Scenario: A pure-ASCII theme is usable on a terminal with no Unicode font

- **WHEN** the ASCII glyph set is in force, over any theme
- **THEN** every symbol UZE draws is within ASCII, and no layout depends on
  a glyph the terminal cannot render

#### Scenario: A replaced symbol does not break alignment

- **WHEN** a theme or a glyph set declares a symbol whose display width
  differs from the default's
- **THEN** UZE lays the containing row out from the resolved glyph's actual
  width, so columns stay aligned

### Requirement: The active theme is a machine-scoped selection that applies without restart

UZE SHALL record which theme is active as machine-scoped state, and SHALL
apply it to every surface it draws. It SHALL record the selected glyph set
the same way and in the same place, as a second and independent selection.
Changing either SHALL take effect in a running TUI without restarting it.

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

#### Scenario: A selection recorded before the second axis existed still loads

- **WHEN** UZE reads a recorded selection that names a theme and says nothing
  about a glyph set
- **THEN** the named theme is active and the default set is in force, without
  the operator being asked anything

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

A glyph that *is* the content — a diagram's lines, corners, junctions and
arrowheads, a rendered document's rule — SHALL be allowed through as given,
the way a colour that belongs to content is. Such a glyph SHALL be coloured
by a semantic role and never by a value, SHALL be written by a file named
for it with the reason stated, and SHALL have a plain ASCII form wherever
the drawing depends on it to be read. A glyph that marks, separates or
labels the surface around the content remains chrome and SHALL come from
the vocabulary.

#### Scenario: An extension's chrome follows the active theme

- **WHEN** an extension's view is drawn under a non-default theme
- **THEN** its chrome is drawn from that theme, resolved by the host, with
  no change to what the extension itself produced

#### Scenario: Content with its own colours is still allowed through

- **WHEN** an extension supplies a colour for content that carries its own
  palette
- **THEN** that colour is drawn as given, and it remains the only way an
  extension can put a specific colour on screen

#### Scenario: A diagram's lines are content

- **WHEN** an extension draws a diagram whose boxes and edges are made of
  box-drawing characters
- **THEN** those characters are drawn as given, in the colour the active
  theme resolves for the role each carries

#### Scenario: A diagram drawn where box-drawing cannot be trusted

- **WHEN** the operator switches the diagram to its ASCII rendering
- **THEN** every glyph the drawing depends on is an ASCII character

#### Scenario: A chrome glyph written inline by an extension

- **WHEN** a file that is not named for content glyphs writes a chrome
  glyph inline
- **THEN** the build fails, naming the file and the rule

### Requirement: The glyph set is selected independently of the palette

UZE SHALL carry more than one glyph set for its symbol vocabulary, and SHALL
let the operator select one as machine-scoped state independently of which
theme is active. Selecting a set SHALL NOT change any colour, and selecting a
theme SHALL NOT change the selected set.

A set SHALL declare the symbols it means to change and SHALL inherit the
rest, so a set exists to state a difference rather than to restate the whole
vocabulary — except where the set makes a promise about every symbol, as the
ASCII set does.

A set SHALL apply beneath the active theme and above the built-in default, so
a theme that declares a symbol of its own still decides that symbol, and the
set supplies every symbol no theme declared. The operator's own overrides
SHALL continue to apply over both.

One of the sets SHALL be drawn entirely within ASCII, so a terminal with no
Unicode font has a complete answer that costs no palette.

#### Scenario: Choosing a set leaves the palette alone

- **WHEN** the operator selects a different glyph set
- **THEN** every symbol is drawn from that set, every colour is unchanged,
  and the theme that was active is still active

#### Scenario: Choosing a theme leaves the glyphs alone

- **WHEN** the operator selects a different theme that declares no symbols of
  its own
- **THEN** the glyphs stay the ones the operator selected, rather than
  reverting to whatever the theme's ancestry carried

#### Scenario: A theme's own symbol still decides that symbol

- **WHEN** the active theme declares a glyph for a named symbol and a glyph
  set is selected
- **THEN** the theme's glyph is drawn for that symbol, and the set supplies
  every symbol the theme did not declare

#### Scenario: The operator's overrides win over both

- **WHEN** the operator declares an override for a symbol
- **THEN** that glyph is drawn whichever theme and whichever set are active

#### Scenario: A set applies with no theme chosen

- **WHEN** a glyph set is selected and no theme ever has been
- **THEN** the set applies over the built-in default, the way overrides
  already do

#### Scenario: A set drawn from a patched icon font declares its own widths

- **WHEN** a set's glyph occupies a different number of terminal cells than
  its codepoint alone implies
- **THEN** UZE lays the containing row out from the width the set declares,
  so columns stay aligned

#### Scenario: A set states a difference rather than the whole vocabulary

- **WHEN** a selected set declares some symbols and not others
- **THEN** the symbols it declares are drawn from it, and every symbol it
  leaves out is drawn from the built-in default, so a set never has to
  restate a glyph it does not mean to change

#### Scenario: The selected set survives across invocations

- **WHEN** a glyph set is selected and UZE is later run again
- **THEN** that set is in force, in both the TUI and the CLI

#### Scenario: The operator can see what sets exist and which is active

- **WHEN** the operator asks UZE which glyph sets exist
- **THEN** UZE lists every set it carries and identifies the active one

### Requirement: A set may draw what plain Unicode cannot

The symbol vocabulary SHALL be able to name marks that only a patched icon
font can draw — the kinds of thing a file tree's rows are, among them — and
a set that cannot draw one SHALL resolve it to nothing rather than to a
substitute. UZE SHALL then draw nothing and reserve no space for it, so a
surface reads the same as it did before the mark existed.

This is what keeps the vocabulary honest about the emoji rule: plain
Unicode has no folder or document mark a terminal does not take from its
emoji font, and UZE carries no emoji, so "nothing" is the correct answer
for a set with no patched font behind it rather than a gap in that set.

#### Scenario: A set with no icons for a mark simply does not draw it

- **WHEN** a surface asks for a mark the active glyph set leaves blank
- **THEN** nothing is drawn in its place and no column is held for it, and
  every other mark on the row keeps the position it had

#### Scenario: A set with icons draws them without the surface knowing

- **WHEN** the active set declares those marks and a file tree is drawn
- **THEN** each row carries the mark for what it is, chosen by the surface
  as a *kind* rather than as a glyph, so the same tree draws differently
  under a different set with no change to the surface

### Requirement: Appearance is chosen by looking at it, never inferred

UZE SHALL provide a surface inside the product where the themes this machine
can draw with and the glyph sets UZE carries are both listed and selectable,
and SHALL render each glyph set in its own glyphs so the operator decides by
seeing what this terminal actually draws.

No terminal can be asked which font it is rendering with. UZE SHALL therefore
NOT infer the glyph set from the terminal, the environment, or the platform,
and SHALL NOT ask the operator to declare which font they installed — a
question they would have to answer from memory about a fact only the screen
can settle.

#### Scenario: A set is judged by its own marks

- **WHEN** the operator opens the appearance surface
- **THEN** each glyph set is shown drawn in its own glyphs, so a set this
  terminal cannot render is visible as unrenderable before it is chosen

#### Scenario: Selecting from the surface applies at once

- **WHEN** the operator selects a theme or a glyph set from that surface
- **THEN** the next frame is drawn with it, the selection is recorded, and no
  session, pane, or agent is disturbed

#### Scenario: UZE never guesses the font

- **WHEN** no glyph set has been selected
- **THEN** the set UZE draws with is the one it defaults to, chosen for
  breadth of terminal support, and no detection of the operator's font is
  attempted

