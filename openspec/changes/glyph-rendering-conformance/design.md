## Context

See proposal.md, under Why, for the measurement that started this.

What exists today:

- `SymbolDef` carries a glyph and a declared `width`. Columns are laid out
  from the width, never from the string (`crates/uze-theme/src/symbol.rs`).
- `nerd.json` declares every icon at `width: 1` for "the Mono builds". A
  test holds each glyph to the Nerd Fonts v3 private-use ranges
  (`load.rs::every_nerd_glyph_is_an_icon_a_patched_font_supplies`). It says
  outright that it cannot check the optical size, and that "the generator
  measures it against a real font". That generator is not in the
  repository.
- Chrome is drawn only through `src/ui/widget/`, and colour and glyphs only
  through `theme`. Two architecture tests enforce that. So "how an icon is
  drawn" can be decided in one place.
- `conformance/` is the Harness Conformance Lab. It runs a real binary in a
  disposable Docker environment with no network, records
  `evidence/<vendor>.json`, and states one contract that names no vendor
  plus bindings per vendor. The Rendering Lab reuses that shape rather than
  inventing another.
- `macos-session.yml` already lends a person a real macOS desktop, which is
  where Terminal.app and iTerm2 evidence can come from.

The constraint behind every decision below comes from the prior design and
still holds: **no terminal can be asked which font it is drawing with.** So
the product cannot adapt to the build. It has to draw in a way that is
correct under all of them, and the Lab has to prove that it is.

What the metrics already show:

- Every build, including plain and Propo, **reserves one cell** for an icon.
- Mono fits the ink inside that cell by shrinking the icon to the cell's
  width.
- Plain builds size the icon to the font's height and let the ink run up to
  ~1.7 cells to the right, always starting at the cell's left edge.
- A few terminals redraw icons on their own terms. The Lab found that
  WezTerm and Ghostty rescale the icons of a Symbols-only *fallback* per
  icon, but draw the icons of a patched font the way the font does. Every
  other terminal measured draws the font's outlines as they are. (This
  design first assumed kitty, Ghostty and WezTerm enlarge an icon into a
  following space. With a patched font, none of them did.)

## Goals / Non-Goals

**Goals:**

- Under every Nerd Font build, and in every terminal we can run, UZE's
  icons are contained, whole, one size on a screen, and aligned (the four
  criteria in the new spec).
- Every claim about that is backed by recorded evidence: font metrics for
  what the font says, pixels for what the terminal draws.
- Evidence gets old. The nightly Lab run checks it again, and the public
  catalog is generated from it.

**Non-Goals:**

- The **same pixel size in every terminal**. Whether kitty draws an icon
  over two cells is kitty's choice. The spec lets sizes differ between
  terminals and forbids them differing between builds. Chasing
  cross-terminal parity would mean fighting each terminal's renderer with
  escape tricks that the next release breaks.
- Detecting the font or the terminal, which remains refused.
- Emoji, and icon fonts that are not Nerd Fonts v3.
- Proportional (Propo) builds laid out proportionally. A terminal forces
  them onto the grid anyway, and they are measured as such.
- Automated evidence for Windows Terminal, iTerm2 and Terminal.app. They get
  the manual path: a screenshot of the specimen, measured by the same
  analyzer.

## Decisions

### Correctness means four outcomes, and size may vary between terminals

The spec's four criteria (*Contained*, *Whole*, *One size*, *Aligned*) are
all things a person sees as a bug when they fail: an icon on top of a
letter, an icon cut in half, one icon visibly smaller than its neighbour, a
column that jogs. None of them is "matches a reference size". The reference
size differs by terminal on purpose, and a criterion that ignored this would
fail the matrix permanently or push us into per-terminal workarounds.

*Alternative, a golden screenshot per terminal:* rejected. It is brittle
across terminal releases and anti-aliasing changes, it says nothing about
why a run failed, and it cannot judge a manual screenshot from a machine the
Lab has never seen.

### An icon is a glyph plus a reserved blank cell, and the width stays 1

The icon's declared `width` stays at the advance the font reserves, which
is 1 in every build. Whatever draws an icon follows it with one blank cell,
the **gutter**, in the icon's own ground. Every build is then correct by
construction:

- Mono draws inside cell 1.
- A plain build overflows into cell 2, which is reserved for it.
- A terminal that scales into a following space finds that space reserved.

Whether a symbol is an icon is decided from the resolved glyph (a codepoint
in the private-use area), not from its name. A set that draws the same
symbol in plain Unicode therefore reserves nothing, and `default` and
`ascii` keep their exact layout. `SymbolDef::is_icon` answers that question.

*Alternative, declare `width: 2`, as the current docs advise:* rejected. It
states the width wrong, since the font reserves one cell and not two. It
puts the fix in the data, where every set and every override has to
remember it. And it cannot say that the second cell must be painted in the
icon's ground, which is the part a chip's edge needs.

*Alternative, ship `nerd` and `nerd-wide`:* rejected for the same reason as
before. It also presumes the operator knows their build, and that is
exactly the unanswerable question.

**The gutter is a plain space**, and every surface already draws it. The
buffer test below found no surface where an icon was followed by anything
else, so there is no icon-slot primitive to route call sites through; the
test is what keeps it true. The one exception was in the CLI's prompt
hints, where `↑↓` put two icons side by side. `progress::slotted` gives a
glyph its gutter where it is an icon. A no-break space was considered as a
way to stop terminals from enlarging icons into the gutter, and rejected:
every surface would have to emit a second kind of blank, and it copies as
something other than a space. The Lab then showed no terminal enlarging a
patched font's icon into the gutter, so there was nothing to stop.

### The slot is enforced over rendered buffers, not over call sites

One test renders the product's surfaces into ratatui's `TestBackend` under
the `nerd` set. It scans every cell holding a private-use codepoint and
fails unless the next cell is blank and has the same background. Checking
the drawn result catches a surface that builds a `Span` from a symbol by
hand, which a source-level rule could not tell apart from a legitimate one.
It covers every management route and overlay, and the workspace with
agents, shells, the task states and the code surface open. It draws each
test under the `nerd` set on its own thread (`uze_theme::drawing_with`)
rather than swapping the process-wide theme under neighbouring tests.

### "One size" is measured once, the same way, by fonts and by pixels

For an icon, the **optical size** is the geometric mean of its ink box's
sides, in cell heights, after the build's own normalisation. It measures
area, not the longest side. A portrait file icon and a wide check mark with
the same longest side do not look the same size, and the same area does.
The fonts suite computes it from outlines and the Lab from pixels, with the
same formula in `contract.py`.

The tolerance comes from the baseline, not a guess. It is the tightest band
the regenerated set meets in every catalog build, capped at ±15%. That came
out at **±13%**.

**Marks** are exempt from *One size*. These are glyphs drawn at text weight
by meaning: a bullet, a status dot, a caret, an arrow beside a number, a
close cross, an ellipsis. Each carries its reason in `nerd_set.py`. Holding
them to the band would mean drawing a caret as a pictogram. They are still
held to *Contained*, *Whole* and *Aligned*.

### What the Lab fed back into the set

Outlines are not the whole story. WezTerm and Ghostty redraw a
Symbols-only fallback's icons at a size of their own, per icon. There,
`mark.sparkle` (Codicons, filled and outline) and `file.config` (the
Codicons gear) came out 20–38% off the set while passing everywhere else.
`nerd_set.LAB_REJECTED` records each rejected candidate with what the Lab
saw, and the generator skips it.

Swapping those two raised the median enough that `file.data` and `code`
fell out of the band in the plain builds. The generator then took the next
candidates by meaning: Font Awesome's database, and Codicons' `{}`. It
settled with every catalog build passing at ±13%, and every terminal
passing its Symbols-only case.

### The `nerd` set is generated, and squareness is how it survives every build

The generator comes into the repository as `conformance/rendering/generate_nerd.py`.
For each symbol it takes a list of candidate icon names, ordered by
meaning. It reads a pinned Nerd Fonts `glyphnames.json` and keeps the first
candidate whose ink is within the size band in every catalog build. It
prefers aspect ratios between 0.8 and 1.25, because a near-square icon comes
out the same size whether a build scales it by width (Mono) or by height
(plain). `architect`, a wide sitemap, is the case that motivated this. The
candidate lists live in the generator, which states the reason for each
choice. The metrics suite checks that regeneration reproduces `nerd.json`
byte for byte.

### Two suites in one Lab: fonts first, then pixels

`conformance/rendering/` follows the Harness Lab's layout:

```
conformance/rendering/
├── lab.py              # entry: --suite metrics|terminals [--terminal t] [--font f]
├── fonts.json          # the pinned catalog: family, build, release, url, sha256
├── contract.py         # the four criteria, the tolerance, the size formula
├── metrics.py          # suite 1: fontTools over the catalog, no terminal
├── analyze.py          # pixels -> per-symbol measurements -> verdict (also the manual path)
├── generate_nerd.py    # the set, by name and by measurement
├── Dockerfile          # Xvfb + a wlroots headless compositor, Mesa llvmpipe, the terminals, the fonts
├── terminals/<t>/bindings.py   # how t is launched with a font, and how it is captured; no assertion
└── evidence/
    ├── catalog.json    # committed: the summarised verdict the docs are generated from
    └── manual/         # committed: measured screenshots from terminals the Lab cannot run
```

**Metrics** runs first and fails fast. It is cheap and deterministic, and a
set that fails it cannot pass a terminal. **Terminals** runs, for each
terminal and each font of the catalog, `uze theme specimen nerd` inside the
real emulator with the font forced through its own configuration. Padding
is set to zero, the cursor is hidden, and the size and theme are fixed. It
captures the screen with `import` on X11 or `grim` on Wayland (for foot),
then analyzes.

A terminal that cannot run here, for example a GPU renderer that refuses
llvmpipe, declares itself in `bindings.unsupported` with the reason, as
Harness Lab verticals do. The catalog then shows it as not verified.

Terminal versions are pinned in the image and recorded in the evidence.
Moving one is a deliberate commit, and the verdict diff is the review.

*Alternative, put the terminals in the existing conformance image:*
rejected. That image exists to run harnesses. Adding a display server,
eight terminals and forty fonts to it would make every harness run pay for
them.

### The specimen is plain text in the product's own slots

`uze theme specimen [set]` prints plain text, with no colour and no escape
sequence. The analyzer reads ink against the terminal's own background, and
a specimen that painted grounds would be measuring UZE's palette instead of
the font. The slot it draws is the product's: the glyph, a space, then text.
From top to bottom it draws:

1. A **calibration row** of full blocks (`█`), which gives the analyzer the
   cell width, the cell height and the grid origin, whatever crop or window
   chrome surrounds it.
2. For each symbol, one row with the icon in its slot followed by a
   reference letter, which the *Contained* and *Aligned* checks use.
3. For each symbol, a second row with the icon followed by free space,
   which the *Whole* check compares against.

A blank row separates every row from the next, so vertical overflow can be
attributed. The command is Budgeted in `command_performance.rs`, since it
reads a theme and draws.

Because the analyzer finds the calibration row by itself, a manual
screenshot from Windows Terminal or iTerm2 needs no preparation beyond
running the command.

### Evidence is committed as a summary, screenshots are artifacts

Every run writes a `verdict.json` per terminal (measurements, the four
verdicts, the terminal, font and `uze` versions) plus screenshots. CI
uploads these as artifacts, and `make rendering-replay` re-analyzes them
without a terminal.

`evidence/catalog.json` is the committed summary: terminal × family × build
→ pass or fail per criterion, the size the terminal chose, and manual
entries marked as such. Nightly fails when a fresh run disagrees with it.
The appearance page's catalog table and per-terminal settings are
generated from it between markers, and a drift check (the same way
`CREDITS.md` is checked) fails the build when the page and `catalog.json`
disagree.

### Diagrams

The Lab is test tooling, not a container of the product, and `theme
specimen` is one more command on an existing path. No diagram under
`docs/architecture/` changes.

## Candidate ADRs

- **A Rendering Lab measures what terminals draw from pixels, against
  pinned fonts.** It adds a third evidence tier with its own image and
  workflow, and a public claim (the catalog) that is generated from it.
- **An icon from a patched font is drawn with a reserved gutter, and its
  width stays its advance.** This is a layout rule every surface now obeys,
  and it is costly to reverse once themes and docs assume it.

## Risks / Trade-offs

- **[GPU terminals under software GL can be slow, flaky, or refuse to
  start]** → Mesa llvmpipe is pinned in the image. A terminal that still
  cannot run is declared unsupported with the reason rather than retried.
  No retry loops.
- **[Anti-aliasing makes ink edges fuzzy]** → Ink is thresholded against a
  background read from a known-empty cell. The criteria compare against the
  measured cell size with a one-pixel tolerance, and the calibration row
  makes the scale exact.
- **[The image gets large (eight terminals, KDE and GTK libraries, about
  forty fonts)]** → It is its own image, built only by `rendering.yml` and
  cached. Konsole is the heaviest entry, and it is the first to declare
  itself unsupported if the cost is not worth it.
- **[Ghostty on Linux has no single official binary]** → Build it in a
  pinned builder stage from the release tag. If that proves unworkable,
  declare it unsupported with the reason.
- **[A gutter adds a column after icons]** → Most icons already sit before
  a space, and the buffer test lists every place where one does not, so the
  cost is seen before it ships.
- **[An operator's `width: 2` override, taken from the old docs]** → It
  keeps working and reserves one column more. The corrected docs tell them
  to remove it, and nothing breaks if they do not.
- **[Tolerance tuned until the set passes]** → The cap (±15%) is set
  before the baseline is measured, and the chosen value is committed next
  to the measurement that justified it.
