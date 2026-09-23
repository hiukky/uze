# UZE Rendering Lab

The Rendering Lab proves what **fonts and real terminal emulators** make of
UZE's glyphs. The question it answers is the one an operator asks when the
same icon looks right in one terminal and broken in another: *does every
icon draw correctly here, whichever Nerd Font build is installed?*

It never mocks a terminal and never asserts on UZE's own report of itself.
It reads font files, and it reads pixels.

## What "correct" means

`contract.py` states four criteria once, naming no terminal and no font.
Each is something a person sees as a bug when it fails:

| Criterion | Fails when |
| --- | --- |
| **Contained** | an icon's ink leaves the two cells UZE reserves for it (its own and the blank one after it), or its row |
| **Whole** | the icon draws less ink in its slot than with free space around it: something cut it |
| **One size** | an icon is more than ±13% (`contract.TOLERANCE`) away from the set's median size |
| **Aligned** | the text after the slot does not start where the layout said it would |

Size is the geometric mean of the ink box's sides, in cell heights: an
area, because that is what an eye compares. Glyphs drawn at text weight by
meaning (a bullet, a caret, an arrow beside a number) are **marks**. Each
is declared in `nerd_set.py` with its reason and exempt from *One size*,
never from the rest.

Sizes may differ *between terminals*, because kitty enlarging icons is
kitty's choice. They may not differ between icons on one screen, and a
build of a font may not change the verdict.

## Layout

```
conformance/rendering/
├── lab.py             # entry, from the repository root (drives Docker)
├── contract.py        # the four criteria, the tolerance, the size formula
├── fonts.json         # the pinned catalog: Nerd Fonts release, archives, sha256
├── fonts.py           # fetch + verify the catalog (never committed)
├── metrics.py         # suite 1: outlines, no terminal
├── nerd_set.py        # per symbol: candidate icon names by meaning; the marks
├── generate_nerd.py   # writes crates/uze-theme/themes/nerd.json — never edit it by hand
├── capture.py         # suite 2, inside the image: every terminal × every font
├── terminals/<t>.py   # how <t> is launched with a font; no assertion
├── analyze.py         # pixels -> measurements -> verdict (also the manual path)
├── replay.py          # re-analyze a run's screenshots; the verdicts must reproduce
├── catalog.py         # evidence -> evidence/catalog.json -> the appearance page
├── Dockerfile         # Xvfb, headless sway, Mesa llvmpipe, the terminals, the fonts
├── evidence/
│   ├── catalog.json   # committed: what the docs are generated from
│   ├── metrics/       # committed: the fonts suite's measurement of the set
│   └── manual/        # committed: measured screenshots from terminals the Lab cannot run
└── tests/             # the analyzer and the fonts suite against known answers
```

## Running it

```bash
make rendering-metrics                 # fonts suite, every glyph set × every catalog font
make rendering                         # every terminal × every font
make rendering TERMINAL=kitty FONT="Hack"
make rendering-replay                  # the last run's screenshots must reproduce its verdicts
make rendering-sandbox                 # a shell in the image
make rendering-catalog                 # record the last run and regenerate the docs table
```

Everything runs in the image, with `--network none`. The fonts, terminals
and `uze` are exactly what the Dockerfile pins, and moving any of them is a
commit whose verdict diff is the review. Screenshots and per-run verdicts
land in `target/rendering-lab/`: they are CI artifacts, not repository
content.

Each case gets a fontconfig of its own that holds only the font under test,
plus DejaVu, which is what the Symbols-only font is a fallback *for*. So no
other font of the catalog can quietly stand in for a missing glyph. A
capture is taken once the specimen reports it has drawn and two consecutive
screenshots agree. There is no fixed sleep and no retry.

## The `nerd` set comes from here

`generate_nerd.py` resolves each symbol's candidate icon names from the
pinned `glyphnames.json`, by name and never by codepoint. It keeps the first
candidate, in order of meaning, that sits within the tolerance in every
build of every catalog font. A Mono build shrinks an icon to one cell's
width and a plain build sizes it by height, so the icons that survive both
are the near-square ones that fill their box. To change an icon, edit
`nerd_set.py` and regenerate. CI checks `--check`, which fails if
`nerd.json` is not what the generator produces.

## A terminal the Lab cannot run

Windows Terminal, iTerm2 and Terminal.app cannot run here. Measure them
from a screenshot. `macos-session.yml` lends a real macOS desktop for the
latter two.

```bash
uze theme specimen nerd --format json > layout.json
uze theme specimen nerd                      # then screenshot the terminal
python3 conformance/rendering/analyze.py shot.png --layout layout.json \
  --terminal "Windows Terminal 1.23" --font "JetBrainsMono Nerd Font Mono" \
  --out conformance/rendering/evidence/manual/windows-terminal--jetbrains-mono.json
```

The screenshot is read against the specimen's calibration row, so window
chrome and cropping do not matter. Use a light background with dark text,
the way the Lab draws it. The analyzer reads ink against the most common
colour on the screen.

## Adding a terminal or a font

- **A terminal:** add `terminals/<name>.py` (`NAME`, `DISPLAY`, `VERSION`,
  `launch(case)`), add it to `terminals.NAMES`, and install it in the
  Dockerfile, pinned. If it cannot run here, declare `UNSUPPORTED = "<why>"`
  rather than leaving it out.
- **A font:** add the archive and builds to `fonts.json`, with the digest
  from the release's `SHA-256.txt`. Then run `make rendering-metrics`: a
  font that breaks the set's band fails there before any terminal is
  involved.
