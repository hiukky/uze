## Why

UZE's visual identity is compiled in. 25 colours live as `const`s in
`src/ui.rs`, referenced ~676 times across 17 files, and every one of the 358
`.fg()`/`.bg()` call sites names a colour rather than a meaning — so changing
the look means editing render code, and there is no way for anyone to ship a
theme at all. The same palette is transcribed by hand in three other places
(`src/progress.rs` for CLI output, `crates/uze-terminal`'s OSC 10/11 reply,
syntect's hard-coded `base16-ocean.dark`), which means a colour change made in
one is silently wrong in the others — an agent inside a pane asks the terminal
"what is your background?" and is told a colour that is no longer drawn.

The boundary that fixes this already exists and is already proven: an
extension names a `Role`, never a colour, and the host resolves it
(ADR-041, `crates/uze-extensions/src/view.rs`). The host itself is the one
part of the TUI that does not obey its own rule. Same for glyphs: ~40
symbols are inline string literals scattered through render code, so a
terminal without a Nerd Font, or a user who wants ASCII, has nothing to
change.

## What Changes

- **New crate `uze-theme`** — a leaf crate (serde + thiserror, no ratatui,
  no I/O beyond reading a theme file) holding the single design vocabulary:
  the colour `Token` enum, the `Symbol` enum, the theme file schema, and the
  resolver that turns a theme file into a resolved `Theme`.
- **Colour tokens replace colour constants.** Every `.fg()`/`.bg()` call site
  in `src/` names a `Token` (a meaning: `Token::TextMuted`,
  `Token::SurfaceSelected`, `Token::StateWarning`, `Token::AgentInFlight`)
  and resolves it against the active theme. `Color::Rgb` becomes illegal
  outside the theme adapter, enforced by the architecture suite.
- **Symbols become a library, not literals.** Every glyph the UI draws is a
  named `Symbol` resolved from the theme, the way an icon set is referenced
  by name on the web. A theme declares its own glyph for any symbol; the
  built-in `ascii` theme overrides all of them, so UZE is usable on a
  terminal with no Unicode font.
- **A theme is a file.** `~/.uze/themes/<name>.json`, validated against a
  published JSON Schema. A theme may be partial — anything it does not
  declare falls back to the built-in default — so a usable theme can be five
  lines. Colours accept `#rrggbb`, `#rrggbbaa` (composited over the theme's
  own background at load time, removing the hand-pre-blended surface shades
  that exist today), or `@token` to alias another token.
- **The CLI consumes the same theme.** `src/progress.rs` stops carrying its
  own copy of the palette and resolves the same tokens, so `uze status` and
  the TUI cannot drift apart. `NO_COLOR`/non-TTY behaviour is unchanged.
- **The pane's terminal answers with the truth.** `uze-terminal` receives its
  default foreground/background and the 16 ANSI colours from the caller
  instead of declaring them, so OSC 10/11 replies and indexed cell colours
  follow the active theme.
- **Syntax highlighting follows the theme.** The syntect theme name becomes a
  theme field instead of a hard-coded `base16-ocean.dark`, so a light theme
  does not leave diff content unreadable.
- **`uze theme list|use|show`** selects and inspects the active theme, and the
  TUI gains a live theme switch (no restart).
- Not changed: the default look. The built-in default theme is byte-identical
  to today's palette and glyph set, and the existing render tests keep
  passing — they assert tokens instead of RGB triples, which is what they
  meant all along.
- **Not** in scope: extensions authoring their own themes, per-project
  themes, theme distribution through the marketplace. `uze-extensions` keeps
  its own narrower `view::Role` as its ABI and gains no dependency.

## Capabilities

### New Capabilities

- `ui-theme`: what a theme is, where it lives, which surfaces obey it, how
  a partial theme resolves, and the guarantee that every surface UZE draws
  (TUI chrome, CLI output, pane defaults, diff highlighting) reports the
  same active theme.

### Modified Capabilities

None. No existing capability's requirements change: the theme's default is
today's appearance, and every affected surface keeps the behaviour its own
spec already states.

## Impact

- **New crate**: `crates/uze-theme` — leaf, depends on nothing in the
  workspace, consumed directly by the binary crate (`src/`) the way
  `uze-git` and `uze-extensions` already are.
- **`src/` (TUI + CLI)**: ~676 palette references and ~40 glyph literals
  migrate to token/symbol lookups across 17 files; `src/ui.rs`'s palette
  block and `src/progress.rs`'s five style constants are deleted;
  `src/ui/theme.rs` becomes the one adapter from `uze_theme::Rgb` to
  `ratatui::style::Color`, and `src/progress.rs` the one adapter to
  `anstyle`.
- **`crates/uze-terminal`**: default fg/bg and the ANSI palette move from
  `const` to runtime configuration; no protocol change.
- **`crates/uze-extensions`**: unchanged, except that the syntect theme name
  arrives as an argument rather than a literal.
- **`crates/uze-core`**: `UzeHome` gains `themes_dir()` and the active
  theme's state path.
- **Architecture suite**: one new rule (nothing outside the theme adapter
  names a raw colour), and the existing extension-independence rules stay
  as they are.
- **New CLI surface**: `uze theme list|use|show`, classified in
  `src/command_performance.rs` as `Budgeted`.
