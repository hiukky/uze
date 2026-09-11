# Appearance

Status: **implemented, 2026-09-05.** Owner: `crates/uze-theme`, which is
the authority for everything on this page — the vocabulary, the file
format, and the resolver. This document is the authoring guide; the crate's
own module docs are the reason each decision was made.

UZE's appearance is data. Every colour it draws and every mark it prints is
selected by *what the thing means*, and what that means looks like comes
from the active theme. So a theme is a file you write, and applying it
changes nothing else about how UZE behaves.

Appearance is **two choices, not one**: the palette, and the set of glyphs
every mark is drawn from. They are chosen separately because they are facts
of different kinds — a font is installed once, a palette is picked on a whim
— and neither ever changes the other.

## Where a theme lives

```
~/.uze/themes/<id>.json      a theme you wrote; the file's own stem is its id
~/.uze/theme-overrides.json  your own last word, over whichever theme is on
~/.uze/state/theme.json      which one is active
```

Five themes are built in and need no file:

| Id | What it is |
|---|---|
| `default` | UZE's own look: a near-black backdrop and one sage accent. |
| `dracula` | [Dracula](https://draculatheme.com): a purple accent and neon states over a blue-grey night. |
| `catppuccin-mocha` | [Catppuccin](https://catppuccin.com) Mocha: pastels over a deep indigo base. |
| `tokyo-night` | [Tokyo Night](https://github.com/enkia/tokyo-night-vscode-theme): soft blues over deep navy. |
| `tokyo-night-light` | Tokyo Night's light palette, the same hues deepened for a pale page. |

The four palettes are faithful ports: every colour is one the palette's
authors publish, including the pane's sixteen, and where a palette's hue
falls under the contrast floor for a meaning, another of *its* colours
carries that meaning instead. That is also why Catppuccin's light flavour is
not among them — Latte's yellow, peach and green all sit below 3:1 on its own
page. Diff highlighting uses the nearest syntax set UZE bundles (see *Where a
theme reaches*), so code in a diff is close to the palette rather than
identical to it. Where each palette comes from, and its licence, is in
`CREDITS.md`.

A file of your own with a built-in's id — `dracula.json` — wins over it: a
theme you wrote is yours.

```bash
uze theme list          # what this machine can draw with, marking the active one
uze theme set dawn      # draw in it, from now on, in the CLI and the TUI
uze theme show          # the active theme's resolved values, and its warnings
uze theme show dawn     # any theme's, whether or not it is active

uze theme glyphs        # the glyph sets, each drawn in its own glyphs
uze theme glyphs nerd   # draw every mark from that set, whatever theme is on
```

Inside the TUI, the **Appearance** screen holds both lists, and `t` still
opens the quick theme picker from anywhere. Selecting redraws the next
frame; no session, pane or agent is disturbed.

## The glyph set

Three sets ship, and choosing one never touches your colours:

| Set | What it is |
|---|---|
| `default` | Unicode any modern terminal font draws. No emoji, no private-use glyphs. |
| `ascii` | Every mark inside ASCII, for a terminal with no Unicode font. |
| `nerd` | Codicons — the icons VS Code draws its own chrome with. Needs a font patched by Nerd Fonts **v3**. **Recommended.** |

**Install a Nerd Font and run `uze theme glyphs nerd`.** The `default` set is
honest Unicode, but "Unicode" is a different guarantee per font rather than
one, and most monospace fonts fall short of it. Counting the symbols a font
does *not* serve itself, of the 42 the default set draws with a non-ASCII
glyph:

| Font | Served by some other font |
|---|---|
| DejaVu Sans Mono | 0 |
| JetBrainsMono Nerd Font | 5 |
| Noto Sans Mono | 13 |
| Liberation Mono | 20 |
| Ubuntu Mono | 27 |

Your terminal papers over this with font fallback — it borrows the glyph from
another family and squeezes it into the cell — so what you get is a wobble in
weight and style, not a broken layout. It still means a row of marks drawn
from two or three typefaces. The `nerd` set has no such gamble: one patched
font, one weight, drawn for a monospace grid.

`uze theme glyphs` prints each set drawn in its own glyphs, and the
Appearance screen does the same. **That is the whole interface for deciding**,
because there is no honest alternative: no escape sequence asks a terminal
which font it is rendering with, `$TERM` names the emulator rather than the
font, and a cursor-position probe measures width rather than presence — a
missing glyph and a present one both come back as one cell. So UZE never
guesses your font and never asks you to declare it. It draws the marks; you
pick the row that looks right.

If the `nerd` row is a run of empty boxes, your font is not patched, or is
patched by Nerd Fonts v2, where Codicons do not exist.

`ascii` is not a museum piece, but its case is narrow: a surface with **no
font fallback** to rescue it — a Linux virtual console, a recovery shell,
output captured by something that is not UTF-8. On a desktop terminal you
will never need it. It costs no palette if you do.

The `nerd` set declares its widths for the **Mono** builds (`… Nerd Font
Mono`), whose icons occupy one cell. The plain build draws the same
codepoints two cells wide, and nothing in Unicode distinguishes them — if
your preview column comes out ragged, that is which build you have, and one
`width` override per glyph in your own overrides fixes it.

## Which layer wins

```
built-in default → the glyph set → a theme's ancestors → the theme → your overrides
```

Later layer wins, which is the stack's only rule. Two consequences worth
knowing:

- A theme that declares only `colors` — the ordinary case — leaves your
  glyphs alone.
- A theme that deliberately declares a `symbol` decides that symbol, even
  over the set you chose. If you want yours back, `theme-overrides.json` is
  the layer above both.

`uze theme show` prints the layers in the order they applied, so "which one
won" is a question you can answer rather than guess at.

## A theme is partial

Everything a theme leaves out resolves from the built-in default. A usable
theme is a handful of lines:

```json
{
  "name": "dawn",
  "colors": {
    "surface.background": "#faf7f2",
    "text.primary": "#2b2a28",
    "text.bright": "#141312",
    "text.muted": "#7b736a",
    "accent": "#2f7d4f"
  }
}
```

That is a complete light theme. Every surface, border and diff wash derives
from the background you declared — see *Separation* below.

Point your editor at
[`crates/uze-theme/themes/theme.schema.json`](../crates/uze-theme/themes/theme.schema.json)
for completion over every token and symbol name. `default.json` and
`ascii.json` beside it are the worked examples, and the four palettes there
are partial themes of exactly the shape above: UZE loads them all through
the same resolver it loads yours with.

## Variations

A theme can be a variation of another. `extends` names the parent by id;
everything the child does not declare comes from the nearest ancestor that
does.

```json
{
  "extends": "dracula",
  "name": "Dracula Soft",
  "colors": { "surface.background": "#343746" }
}
```

That is a whole theme. Because merging happens between *declarations* rather
than between resolved colours, everything the parent expressed as a
reference still follows: change the background and every surface the parent
derived from it is recomputed against the new one; change the accent and
everything written `@accent` moves with it.

You can extend a theme UZE carries, too — the example above needs no
`dracula.json` of your own, and `"extends": "default"` works the same way. A chain
that loops is refused with the loop written out, and UZE stops following one
more than eight deep. Glyphs are not something to extend a theme for: they
are the other axis, and `uze theme glyphs` is where they are chosen.

`uze theme show` prints what a theme resolved from:

```
resolved from the built-in default → the `nerd` glyphs → `dracula` → `dracula-soft` → ~/.uze/theme-overrides.json
```

## Your own overrides

`~/.uze/theme-overrides.json` is the same format, applied last, over
whichever theme *and* whichever glyph set is active — and it keeps applying
when you change either. It is the top layer, so it is where you settle an
argument between the two:

```json
{
  "symbols": {
    "status.idle": "◌",
    "mark.official": { "glyph": "󰄬", "width": 2 }
  }
}
```

This is for the *individual* glyph, not for a whole set — a mark your own
font draws differently, a width your build disagrees with, a theme's symbol
you would rather not have. A whole set is `uze theme glyphs`. It is not a
theme either: it never appears in `uze theme list`, and there is nothing to
select.

## The five ways to write a colour

| Form | Means |
|---|---|
| `#rrggbb` | exactly this colour |
| `#rrggbbaa` | this colour at that alpha, composited over the theme's own `surface.background` |
| `~aa` | separated from the background by that much, in whichever direction is visible against it |
| `@another.token` | whatever that token resolves to |
| `@another.token/aa` | that token's value, at that alpha, over the background |

`~aa` is the one worth understanding. A terminal has no alpha channel, so a
raised surface has to be a real colour — and on a near-black backdrop that
means a little white, while on a light page it means a little black. Same
intent, opposite colour. Writing `~17` says *how far to separate* and lets
the loader decide which way, which is why declaring a light background is
enough to get a light theme's whole surface stack.

Aliases follow through your theme, not the default's values: `state.success`
is `@accent` in the built-in theme, so repainting the accent repaints
success with it. `@token/aa` is what lets a *tint* do the same — the
selected row is `@accent/17`, so it follows your accent instead of carrying
UZE's own green into your theme.

**The terminal's own sixteen are the exception.** `ansi.1`–`ansi.6` and
their bright forms are literal colours, not references, because a program
inside a pane that emits index 2 means *green* — whatever your theme calls
green. Only the four that are genuinely a role (`ansi.0`, `ansi.7`,
`ansi.8`, `ansi.15` — background, foreground and its dim and bright forms)
follow your tokens. Declare the rest if you want a pane to match your
palette; a full third-party palette usually ships all sixteen anyway.

Two rules the loader enforces: `surface.background` must be an opaque
`#rrggbb` (it is what everything else composites over), and an alias loop is
refused with the loop written out.

## Symbols

Every mark UZE draws as chrome is a named symbol, and a theme can replace
any of them:

```json
{
  "symbols": {
    "status.working": ["-", "\\", "|", "/"],
    "mark.official": "OK",
    "tree.branch": { "glyph": "|-", "width": 2 }
  }
}
```

UZE's own themes and sets carry **no emoji** — only symbols. An emoji is a
different font family, a width that varies by terminal, and a picture that
ignores the hue carrying the meaning, which is three reasons a status mark
cannot be one; a test holds everything bundled to it. Your theme is yours,
and may use whatever your terminal renders.

A set need not declare every symbol. `nerd` declares the marks, statuses,
chevrons and arrows, and inherits the tree glyphs, bars and typography —
a patched font already draws `├─` and `…` correctly, and an icon there
would be a downgrade. `ascii` is the exception and declares all of them,
because one Unicode glyph left in an otherwise-ASCII screen defeats the
only reason to choose it.

A symbol is a string, a list of frames for an animation, or an object with
an explicit `width` — for a glyph whose display width the terminal disagrees
with Unicode about, which is the usual story with a Nerd Font's private-use
range. UZE lays every column out from the resolved width, so replacing a
glyph with a wider one moves the column instead of shearing the row.

The one thing themes cannot make ASCII is prose. Arrows and separators
*inside* hint lines are notation — `"↑↓ select"` reads as itself in the
source and is translated to the active theme's glyphs when drawn — but a
sentence like "loading…" is content, and stays as written.

## What the warnings mean

`uze theme show` prints anything the loader had to say:

- **`x` is not a colour token this version of UZE knows.** A typo, or a
  theme written for a newer UZE. Ignored, never fatal: a theme in the wild
  has to keep loading when the vocabulary grows.
- **`x` has 1.8:1 contrast against the background.** Reported, never
  corrected — your colour is your decision. It fires on the failure mode a
  partial theme makes easy: you repaint the background, and the state hues
  you did not declare stay where they were. Every colour in your file looks
  fine; the ones you inherited are the problem.

Warnings are printed when you ask — `uze theme show`, and `uze theme set`
as you choose the theme — and not on every command after that. A theme that
will not load *at all* is different: it reports the token and the value that
broke it on every run, because it silently is not in force until you fix it.
UZE keeps drawing in the default meanwhile rather than refusing to run.

## What a theme does not control

- **Layout.** Paddings and minimum widths are invariants of a
  keyboard-driven interface, not appearance: below them the content stops
  being readable. A theme that could change them would break the layout
  rather than restyle it.
- **Anything but appearance.** No behaviour, no keybindings, no defaults.

## Where a theme reaches

Choosing one theme changes all of these at once, which is the point:

- the workspace client and the management TUI;
- the CLI's own output, including the usage and error text `clap` generates;
- what a program running inside a pane is told when it asks the terminal
  for its background or foreground (OSC 10/11), and the sixteen indexed
  colours it can name — so an agent picking a light- or dark-adapted UI
  picks the one you are actually looking at;
- the palette syntax-highlighted diff content is rendered with
  (`syntax.theme`, one of the sets UZE bundles).
