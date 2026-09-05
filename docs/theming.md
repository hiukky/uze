# Theming

Status: **implemented, 2026-09-05.** Owner: `crates/uze-theme`, which is
the authority for everything on this page — the vocabulary, the file
format, and the resolver. This document is the authoring guide; the crate's
own module docs are the reason each decision was made.

UZE's appearance is data. Every colour it draws and every mark it prints is
selected by *what the thing means*, and what that means looks like comes
from the active theme. So a theme is a file you write, and applying it
changes nothing else about how UZE behaves.

## Where a theme lives

```
~/.uze/themes/<id>.json      a theme you wrote; the file's own stem is its id
~/.uze/theme-overrides.json  your own last word, over whichever theme is on
~/.uze/state/theme.json      which one is active
```

Two themes are built in and need no file: `default` (UZE's own look) and
`ascii` (the same colours with every glyph inside ASCII, for a terminal
with no Unicode font). A file of your own named `ascii.json` wins over the
built-in — a theme you wrote is yours.

```bash
uze theme list          # what this machine can draw with, marking the active one
uze theme use dawn      # draw in it, from now on, in the CLI and the TUI
uze theme show          # the active theme's resolved values, and its warnings
uze theme show dawn     # any theme's, whether or not it is active
```

Inside the TUI, `t` opens the same list. Selecting redraws the next frame;
no session, pane or agent is disturbed.

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
`ascii.json` beside it are the worked examples: UZE loads them through the
same resolver it loads yours with.

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

You can extend a theme UZE carries, too — `"extends": "ascii"` gives you its
glyphs and leaves the colours to you. A chain that loops is refused with the
loop written out, and UZE stops following one more than eight deep.

`uze theme show` prints what a theme resolved from:

```
resolved from the built-in default → `dracula` → `dracula-soft` → ~/.uze/theme-overrides.json
```

## Your own overrides

`~/.uze/theme-overrides.json` is the same format, applied last, over
whichever theme is active — and it keeps applying when you switch themes.
It is the right place for anything that belongs to *your machine* rather
than to a palette:

```json
{
  "symbols": {
    "status.idle": "◌",
    "mark.official": "󰄬"
  }
}
```

A Nerd Font is the case this exists for. Your glyphs are a fact about the
font you installed, not about whether you are on Dracula today — so they
live here instead of being copied into every theme you might switch to. It
is not a theme: it never appears in `uze theme list`, and there is nothing
to select.

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

UZE's own themes carry **no emoji** — only symbols. An emoji is a different
font family, a width that varies by terminal, and a picture that ignores the
hue carrying the meaning, which is three reasons a status mark cannot be
one; a test holds the bundled themes to it. Your theme is yours, and may use
whatever your terminal renders.

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

Warnings are printed when you ask — `uze theme show`, and `uze theme use`
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
