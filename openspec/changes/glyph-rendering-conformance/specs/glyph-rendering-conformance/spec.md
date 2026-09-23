## Purpose

States what it means for UZE's icons to draw correctly under any Nerd Font
build and in any terminal, and how that is proven: against real font files
and real terminal emulators, with recorded evidence rather than a claim.

## ADDED Requirements

### Requirement: Correct rendering is defined by outcomes a screen can show

An icon SHALL be judged to draw correctly for a given terminal, font family
and font build when all of the following hold for every icon of the set in
question:

- **Contained**: its ink stays inside the slot UZE reserved for it and never
  reaches a cell holding anything else.
- **Whole**: its ink in the slot matches its ink drawn with free space
  around it, so no terminal cut it at the slot's edge.
- **One size**: the optical size of every icon in the set, measured on that
  screen, falls within one declared tolerance of the set's median. Optical
  size is the area of the icon's ink, and a glyph the set declares a mark
  is exempt.
- **Aligned**: text after the slot starts at the column the layout declared.

The criteria SHALL be stated once, and they SHALL NOT name a terminal, a
font or a vendor, so every entry of the matrix is held to the same ones.

#### Scenario: A colliding icon fails the run

- **WHEN** a terminal draws an icon whose ink reaches the cell after its
  slot
- **THEN** that terminal and font pair is recorded as failing
  *Contained* for that symbol, and the run fails

#### Scenario: A clipped icon fails even though it collides with nothing

- **WHEN** an icon's ink inside its slot is measurably smaller than the
  same icon drawn with free space around it
- **THEN** the pair is recorded as failing *Whole* for that symbol

#### Scenario: A set with one outlier fails One size

- **WHEN** one icon's measured optical size falls outside the declared
  tolerance of the set's median on a screen
- **THEN** the symbol is named in the verdict, together with its measured
  size and the band it missed

### Requirement: The build of a font does not change the verdict

For every terminal and font family in the matrix, the plain, Mono and Propo
builds SHALL each meet the criteria. An icon SHALL NOT pass in one build of a
family and fail in another. The size a given terminal chooses to draw icons
at is the terminal's, and it MAY differ between terminals.

#### Scenario: Mono and plain builds agree

- **WHEN** the same terminal draws the specimen in `<Family> Nerd Font` and
  in `<Family> Nerd Font Mono`
- **THEN** both pass every criterion, and the text after each icon starts at
  the same column in both

#### Scenario: Terminals may differ in size but not in correctness

- **WHEN** one terminal scales icons into their slot and another does not
- **THEN** both pass if each draws them contained, whole, at one size and
  aligned, and the verdict records the size each one chose

### Requirement: Fonts are measured from their files before any terminal draws them

A metrics suite SHALL read every font in a pinned catalog and, for every
symbol of every bundled glyph set, record:

- whether the font supplies the symbol;
- the advance the font gives it;
- its ink bounds relative to the cell;
- its optical size after the scaling that build applies.

It SHALL fail when a symbol is missing from a Nerd Fonts v3 build, when the
font's ink for an icon exceeds the slot, or when the set's sizes spread
beyond the tolerance. It SHALL need no terminal and no network once the
catalog is fetched, and SHALL give the same answer on every run.

#### Scenario: A regenerated set with a wide icon is refused

- **WHEN** a change to the `nerd` set introduces an icon whose shape is so
  far from square that one build draws it outside the size band
- **THEN** the metrics suite fails and names the symbol, the font build and
  the measured size

#### Scenario: A symbol absent from a build is reported, not skipped

- **WHEN** a font in the catalog does not supply a symbol the set declares
- **THEN** the symbol is recorded as missing for that font, and the run
  fails for a Nerd Fonts v3 build

### Requirement: The font catalog is pinned and never committed

The catalog SHALL name each font by family, build, release version and
sha256 digest, and SHALL cover at least JetBrains Mono, Fira Code, Hack,
Iosevka, Cascadia (Caskaydia), Meslo and the Symbols-only font, each in its
plain, Mono and Propo builds where the release ships them. Fonts SHALL be
fetched when the Lab environment is built and verified against their
digests. No font file SHALL be committed to the repository.

#### Scenario: A font whose digest changed is refused

- **WHEN** a fetched font does not match the digest the catalog pins
- **THEN** the environment build fails rather than measuring an unknown font

### Requirement: Real terminals draw UZE's own specimen, and their pixels are measured

A Rendering Lab SHALL run real terminal emulators in a disposable
environment with no network, draw UZE's glyph specimen with the real `uze`
binary in every font of the catalog, capture the screen, and measure every
icon's ink from the pixels against the cell grid the specimen's calibration
row establishes. The matrix SHALL include kitty, WezTerm, Alacritty,
Ghostty, foot, a VTE-based terminal, Konsole and xterm.

Each terminal SHALL be described by how it is launched and captured, and
nothing else. What is asserted SHALL be the shared criteria. A terminal that
cannot be run in the Lab SHALL be declared unsupported with a written
reason, and SHALL NOT simply be left out.

#### Scenario: The specimen is drawn by the product, not by the Lab

- **WHEN** the Lab measures a terminal
- **THEN** what it measures was drawn by `uze theme specimen`, so the slots
  measured are the slots the product lays out

#### Scenario: A terminal the Lab cannot run is visible as such

- **WHEN** a terminal in the product's supported list cannot run in the Lab
- **THEN** the verdict lists it as unsupported, with the reason, and the
  generated catalog shows it as not verified

### Requirement: Every run leaves evidence that can be read and replayed

A run SHALL record, for every terminal, font and build it drew:

- the screenshot;
- the measured values for every symbol;
- the verdict on each criterion;
- the versions of the terminal, the font and `uze`.

The recorded measurements SHALL be enough to reproduce the verdict without
running a terminal again.

#### Scenario: A verdict is reproduced from its evidence

- **WHEN** the analyzer is run on a recorded run's screenshots
- **THEN** it produces the same per-symbol measurements and the same verdict
  the run recorded

### Requirement: A terminal the Lab cannot run can still be measured

The analyzer SHALL accept a screenshot of `uze theme specimen` taken on any
terminal, together with the terminal and font it was taken with, and SHALL
produce the same verdict it produces for the Lab. Such evidence SHALL be
marked as manual in the catalog.

#### Scenario: A Windows Terminal screenshot is measured

- **WHEN** an operator screenshots the specimen in Windows Terminal and runs
  the analyzer on it
- **THEN** the analyzer reports the per-symbol verdict, and the evidence
  can be added to the catalog marked as manual

### Requirement: The catalog of what works is generated from evidence

The public list of the fonts and terminals that draw UZE's icons correctly,
and the settings each terminal needs, SHALL be generated from the recorded
verdicts rather than written by hand. A published catalog that differs from
what the evidence produces SHALL fail the build.

#### Scenario: A regression reaches the catalog, not a reader

- **WHEN** a terminal release starts clipping icons and the nightly run
  records it
- **THEN** regenerating the catalog changes that terminal's entry, and the
  published page cannot keep saying it works
