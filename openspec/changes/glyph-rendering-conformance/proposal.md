## Why

The `nerd` glyph set draws the same codepoints very differently depending on
which Nerd Font build and which terminal draws them. A metrics read of the
fonts on a development machine settled what the difference is:

| Build | Space the terminal reserves | Widest ink | Icons whose ink leaves the cell |
|---|---|---|---|
| JetBrainsMono Nerd Font **Mono** | 1 cell | 1.00 cell | 0 of 45 |
| JetBrainsMono Nerd Font | 1 cell | **1.73** cells | **35 of 45** |
| FiraCode Nerd Font **Mono** | 1 cell | 1.00 cell | 0 of 45 |
| FiraCode Nerd Font | 1 cell | **1.69** cells | **34 of 45** |

Every build reserves **one** cell, but the plain build draws almost every
icon across the next cell as well. That next cell is whatever UZE put there,
so the icon collides with a letter or crosses a chip's edge. What an
operator sees is "the same icon is fine here and broken there". And even on
the Mono builds the set is not one optical size: `architect` is 0.47 em
tall and `code` is 0.69 em, because a wide icon squeezed into one cell comes
out shorter.

`appearance.mdx` and the design that introduced the set both say the plain
build draws icons "two cells wide". The measurement contradicts that, and
the `width: 2` override they recommend reserves space in the wrong way. The
set's claim to be "one optical size" was measured once, by a generator that
was never committed, and nothing checks it now.

The goal is that UZE's icons draw correctly whichever Nerd Font family and
build the operator installed, in every terminal we can run, **with evidence
for it rather than a claim**.

## What Changes

- **An icon gets a slot of its own.** Any glyph drawn from a patched icon
  font is followed by a blank cell that UZE reserves, painted in the same
  ground as the icon. An icon's ink therefore always lands in space UZE
  owns, in every build, instead of on whatever came next. A buffer-level
  test holds every surface to it.
- **The `nerd` set is chosen by measurement.** Its generator is committed
  and picks each glyph by meaning first and then by metrics: a shape close
  to square keeps its size under every build's scaling, whether the build
  fits the icon by width (Mono) or by height (plain). Icons outside the
  tolerance are replaced, `architect` first.
- **A font-metrics suite.** It reads a pinned catalog of Nerd Font
  families, in their plain, Mono and Propo builds, with fontTools. It checks
  that every symbol of every bundled set is present, fits the slot and falls
  within the size band. Needs no terminal and gives the same answer every
  run.
- **A Rendering Lab.** Real terminal emulators run in a disposable Docker
  environment on a virtual display: kitty, WezTerm, Alacritty, Ghostty,
  foot, VTE, Konsole and xterm. Each draws UZE's own glyph specimen in
  every font of the catalog, and the Lab measures every icon's ink from the
  pixels. The result is a per-terminal, per-font verdict with the
  screenshots as evidence.
- **`uze theme specimen [set]`**, a new CLI command. It draws every symbol
  of a set in the slots the product uses, beside a calibration row. The
  Lab measures it, and an operator can screenshot it on a terminal the Lab
  cannot run (Windows Terminal, iTerm2, Terminal.app) and measure that
  screenshot with the same analyzer.
- **Docs generated from evidence.** The "fonts and terminals that work"
  catalog and the per-terminal settings on the appearance page are produced
  from the Lab's verdicts. A catalog that no longer matches the evidence
  fails the build. The wrong "two cells wide" guidance is corrected.

## Capabilities

### New Capabilities

- `glyph-rendering-conformance`: What "an icon draws correctly" means,
  measured against real fonts and real terminals. Covers the conformance
  criteria, the pinned font catalog, the terminal matrix, the evidence each
  run records, the manual evidence path for terminals the Lab cannot run,
  and the catalog generated from evidence.

### Modified Capabilities

- `ui-theme`: an icon from a patched font is drawn in a reserved slot of
  its own, a bundled icon set holds one optical size under every build of a
  font, and `uze theme specimen` draws a set for inspection.

## Impact

- `crates/uze-theme/themes/nerd.json`: glyph choices regenerated. Widths
  stay at 1, which is what every build actually reserves.
- `crates/uze-theme`: `SymbolDef::is_icon`, and `drawing_with` for drawing
  one thread under another set. `src/ui/`: tests that hold every surface to
  the slot. `src/progress.rs`: `slotted`, for the one CLI place that put two
  icons side by side.
- `src/main.rs` and `src/command_performance.rs`: `theme specimen`,
  classified Budgeted.
- `conformance/rendering/` (new): the metrics suite, the Lab image, a
  binding per terminal, the pixel analyzer, the nerd-set generator and
  `fonts.json`.
- `.github/workflows/rendering.yml` (new): runs on changes to glyph sets,
  widgets and the Lab itself, and nightly.
- `web/content/docs/appearance.mdx`: the corrected guidance and the
  generated catalog.
- No new Rust dependency. fontTools, Pillow and the terminals live only in
  the Lab image. Fonts are downloaded at image build, pinned by version and
  sha256, and never committed.
