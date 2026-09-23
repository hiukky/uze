## ADDED Requirements

### Requirement: An icon is drawn in a slot of its own

Wherever UZE draws a symbol that resolves to a glyph from a patched icon
font, it SHALL follow the glyph with one blank cell that it reserves as part
of the icon, painted in the same ground as the glyph. The icon's declared
width SHALL remain the advance its build reserves, which is one cell. The
slot is part of how the icon is drawn, not of how wide the glyph is.

This is what makes an icon correct in every build. A Mono build draws the
icon inside the first cell. A plain build draws it across both cells. A
terminal that scales icons into the free space finds the space reserved.

#### Scenario: A plain build's wide icon lands in reserved space

- **WHEN** the operator's font is a plain Nerd Font build and an icon's ink
  extends past its cell
- **THEN** the ink falls on the blank cell reserved for it, never on the
  text, border or chip edge that follows

#### Scenario: Every surface honours the slot

- **WHEN** any surface is drawn under a glyph set that draws from a patched
  font
- **THEN** every cell holding an icon glyph is followed by a blank cell in
  the same ground

#### Scenario: A set without icons reserves nothing

- **WHEN** a glyph set draws a symbol from plain Unicode, or leaves it blank
- **THEN** no slot cell is reserved for it, so the `default` and `ascii`
  sets lay out exactly as before

### Requirement: A bundled icon set is one optical size under every build

A glyph set UZE bundles for a patched icon font SHALL choose each glyph by
meaning first and then by measured metrics. Every icon it declares SHALL fall
within one size tolerance of the set's median in every build of every font
in the pinned catalog, whether the build fits icons to the cell's width or
to the font's height. The set SHALL be regenerable from the icon names it
declares, and the tolerance SHALL be stated where the set is generated.

A glyph drawn at text weight by meaning, such as a bullet, a status dot, a
caret or an arrow beside a number, SHALL be declared a mark, with its
reason. A mark is exempt from the size tolerance and held to every other
criterion.

#### Scenario: A caret is not drawn as a pictogram

- **WHEN** a symbol is a caret leading a heading
- **THEN** it is declared a mark with its reason, and the icons around it
  are held to one size without it

#### Scenario: A wide icon is not chosen

- **WHEN** two icons say the same thing and one is far wider than it is
  tall
- **THEN** the set carries the one closer to square, because that one keeps
  its size when a Mono build shrinks it to fit a single cell

### Requirement: A glyph set can be drawn as a specimen

UZE SHALL provide `uze theme specimen [set]`. It SHALL draw every symbol of
the named set, or of the active one, each in the slot the product draws it
in, labelled by symbol name, together with a calibration row that
establishes the cell grid. It SHALL change no selection and SHALL write
nothing.

#### Scenario: An operator inspects a set in their own terminal

- **WHEN** the operator runs `uze theme specimen nerd`
- **THEN** every symbol of the `nerd` set is drawn in its slot beside its
  name, and the active theme and glyph set are unchanged
