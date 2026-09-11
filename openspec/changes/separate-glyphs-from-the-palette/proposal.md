## Why

Tokenizing the symbol vocabulary made a pure-ASCII UZE possible, but it left
the glyph set welded to the palette: `ascii` is a *theme*, and `builtin_layer`
recognizes it by name, so choosing how UZE looks and choosing whether your
terminal can draw it are the same choice. An operator with a Nerd Font
installed has to give up every palette or hand-copy forty-four glyphs into
`theme-overrides.json`, and a palette someone writes is unusable on a terminal
that cannot draw the glyphs its author happened to have.

The two decisions have different lifetimes — a font is installed once, a
palette is chosen on a whim — and nothing can detect which font a terminal is
using, so the product has to let someone *look* at the sets and pick.

## What Changes

- The glyph set becomes a machine-scoped selection of its own, independent of
  the theme, applied as one more layer in the stack `resolve_stack` already
  walks: built-in default → glyph set → the theme's ancestry → the theme →
  the operator's overrides. Later layer wins, which is the rule that already
  governs the stack; no new precedence.
- UZE carries three sets: `default` (today's conservative Unicode, and no
  layer at all), `ascii`, and a new `nerd` drawn from Codicons — the icon set
  VS Code draws its own chrome with, designed for UI marks at terminal sizes
  on a monospace grid, and stable since Nerd Fonts v3.
- **BREAKING**: `ascii` stops being a theme. It leaves `uze theme list` and
  becomes a value of the new axis; `uze theme use ascii` becomes
  `uze theme glyphs ascii`. The `builtin_layer` special case that gave one
  theme id a privilege by name is deleted.
- **BREAKING**: `uze theme use <id>` is renamed `uze theme set <id>`.
  `theme use` reads as a redundancy beside `theme list` and `theme show`, and
  the axis being added would have made it two verbs for one act (`use` a
  theme, `glyphs` a set).
- New `uze theme glyphs [<set>]` — lists the sets marking the active one, or
  selects one.
- New **Appearance** route in the management TUI, a seventh alongside
  Overview, Plugins, Extensions, Integrations, Profiles and Keys: the themes
  this machine can draw with and the three glyph sets, each set rendered in
  its own glyphs so the operator chooses by seeing what their terminal
  actually draws. This is the product's answer to a font that cannot be
  detected — not a question about what someone installed, but the marks
  themselves, on screen, next to each other.
- A glyph set chosen with no theme chosen applies on its own, the way
  overrides already do.

## Capabilities

### New Capabilities

None. This is the existing appearance capability gaining a second axis and
the surface to manage it.

### Modified Capabilities

- `ui-theme`: symbols become a selectable set rather than a property of the
  active theme; the machine-scoped selection requirement gains a second,
  independent axis; and choosing appearance acquires a surface inside the
  product where the choice can be made by looking.

## Impact

- `crates/uze-theme` — new `themes/nerd.json` (symbols only, explicit
  `width` per glyph); `load.rs` splits `builtin_names()` (bundled *themes*)
  from a new glyph-set lookup. `resolve_stack` itself is unchanged: it
  already applies a slice of layers in order, and the whole feature is one
  more entry in that slice.
- `crates/uze-core::theme_state` — `ThemeSelection` gains an optional
  `glyphs` field in the same `state/theme.json`. Absent reads as the default
  set, so an existing file needs no migration.
- `crates/uze-application::application::theme` — `glyphs()` /
  `select_glyphs()` beside the existing `active()` / `select()`.
- `src/theme.rs` — `builtin_layer` deleted, the glyph-set layer inserted in
  its place; `install()` must apply when only a glyph set is chosen.
- `src/ui` — a `Route::Appearance` variant, a `view/appearance.rs`, and the
  hits and intents it needs.
- `src/main.rs`, `src/command_performance.rs` — the renamed and the new leaf
  command, classified.
- `docs/theming.md` — the axis, and `theme-overrides.json` narrowing to what
  it was always for: the individual glyph your font draws differently.
- No new dependency. Codicons are codepoints in a font the operator already
  installed, not a crate.
