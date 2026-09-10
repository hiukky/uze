## ADDED Requirements

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

## MODIFIED Requirements

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
