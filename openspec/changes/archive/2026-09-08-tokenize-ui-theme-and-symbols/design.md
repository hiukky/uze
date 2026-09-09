## Context

See proposal.md — Why. The design-relevant facts about today's code:

- The palette is 25 `const Color`s in `src/ui.rs` (22 in one block, plus
  `NAV_INACTIVE`, `LINE_ADDED_BG`, `LINE_REMOVED_BG`), referenced ~676 times
  across 17 files. 358 `.fg()`/`.bg()` call sites; only 6 functions map a
  *meaning* to a colour. Everything else names a colour at the point of
  drawing.
- Six of those colours are hand-composited: their doc comments say
  `rgba(255,255,255,0.09)` pre-blended over `BASE`, because ratatui has no
  alpha.
- Three surfaces carry a second copy of the palette: `src/progress.rs`
  (anstyle, CLI), `crates/uze-terminal/src/runtime.rs:1172`
  (`REPLY_BACKGROUND`/`REPLY_FOREGROUND` for OSC 10/11), and
  `crates/uze-extensions/src/git.rs:1001` (`themes["base16-ocean.dark"]`).
- ~40 distinct glyphs are inline literals. Geometry constants are duplicated
  too — `TRAILING_PAD` defined 4×, `H_PAD` 5×, `MIN_CONTENT_WIDTH` twice
  with *different* values (30 and 40).
- Architecture constraints that bound any answer: `src/` may not name
  `uze_core::`; `uze-extensions` depends on no UZE crate and names no FS,
  process or env API; `uze-terminal` depends on nothing in the workspace;
  `src/` already depends directly on `uze-git` and `uze-extensions`.

## Goals / Non-Goals

**Goals:**

- One vocabulary — tokens and symbols — that is the *only* way any UZE
  surface names an appearance.
- A theme is authorable by hand, in a few lines, without knowing Rust.
- Zero visual change on the default theme, proven by the existing tests.
- The vocabulary is enforced, not merely offered: naming a raw colour
  outside the adapter fails the build.

**Non-Goals (design level, beyond the proposal's):**

- No live file-watching of theme files. A theme is re-read when selected,
  not when its file changes on disk.
- No contrast/accessibility auto-correction. The loader may *warn*; it never
  adjusts a colour the author wrote.
- No per-token override at the CLI (`--color accent=#fff`). A theme is a
  file; the CLI selects one.
- Geometry (paddings, widths) is **not** tokenized in this change. The
  duplicated constants are consolidated where the migration touches them
  anyway, but spacing does not enter the theme schema — see Decision 7.

## Decisions

### 1. A new leaf crate, `uze-theme`, holding vocabulary + schema + resolver

**Chosen:** `crates/uze-theme`, dependencies `serde`, `serde_json`,
`thiserror` only. No ratatui, no anstyle, no `UzeHome`. It exposes its own
`Rgb(u8,u8,u8)`, the `Token` and `Symbol` enums, `ThemeFile` (the schema),
and `Theme` (resolved). It reads a theme from a `&Path` handed to it; it
never resolves where that path is.

*Why a crate at all*, when today's only consumers are inside the binary
crate: the vocabulary is exactly the kind of thing that decays into "one
more const" when it lives next to the code that draws. A crate boundary
makes "does this belong to the design system?" a compile question. It also
gives the architecture suite a scope to point at ("nothing outside
`crates/uze-theme` and `src/ui/theme.rs` names a colour value"), and it lets
the schema be versioned independently of the TUI.

*Why a leaf* (no `uze-core` dependency, even for paths): the same reason
`uze-terminal` is one. A crate that resolves `$UZE_HOME` is a crate that has
opinions about the machine; this one should have opinions about colour. The
caller passes the directory. `src/` gets that directory from
`UzeHome::themes_dir()`, which `uze-application` already re-exports — so no
layering rule is touched.

*Alternatives considered:* a module in `src/ui/`. Rejected: `src/progress.rs`
(the CLI, not the TUI) is a first-class consumer, and burying the design
system inside the TUI module is what produced the current duplication. A
module in `uze-core`: rejected — appearance is not domain, and `src/` may not
name `uze_core::`, so every token reference would have to be laundered
through `uze-application`.

### 2. `uze-extensions` keeps its own `view::Role` and gains no dependency

The extension ABI stays a 12-variant `Role`; the host maps it to `Token` in
`src/ui/extension_view.rs` (the 12-line function that already exists). The
theme crate is *not* added to `uze-extensions`.

*Why:* the extension contract is deliberately narrower than the host's —
an extension draws content, not chrome, and has no business naming
`Token::SurfaceRaised` or `Token::TabStripBackground`. Giving it the full
vocabulary would widen the trust surface ADR-041 exists to keep narrow, and
would make the extension ABI move every time the theme schema grows. A
12-line mapping is the correct price for that independence.

### 3. Three declaration forms for a colour, one resolved value

A theme declares a token as one of:

- `"#rrggbb"` — an opaque colour.
- `"#rrggbbaa"` — composited at load time over the theme's own
  `surface.background` token, producing an opaque value.
- `"@some.token"` — an alias, resolved transitively (cycles are a load
  error naming the cycle).

*Why the alpha form:* it is what the code already does by hand and documents
in prose — six surfaces are `rgba(255,255,255,α)` pre-blended over `BASE`.
Making the loader do the blending means a theme author writes `#ffffff17`
once instead of computing `Rgb(32,34,35)`, and it is what makes a *light*
theme cheap: the same declaration composited over a light background yields
the light-theme shade automatically. Keeping it hand-blended would mean
every theme author redoes arithmetic that has one correct answer.

*Why not a full colour algebra* (`lighten(accent, 10%)`, `mix(a,b)`):
rejected for now. Alpha-over-background covers every derived colour that
exists today; anything more is a language, and a language in a config file
needs a much stronger reason than six shades.

### 4. Resolution happens once, at load; drawing is a lookup

`Theme` is a fully resolved, flat table: `Token → Rgb`, `Symbol → SymbolDef`.
No aliases, no alpha, no `Option` at draw time. The loader does merge
(theme over built-in default), alias resolution, alpha compositing, and
validation. A malformed theme is rejected *at load* with the default kept
active — never half-applied.

### 5. The active theme is process-global, behind `RwLock<Arc<Theme>>`

`uze_theme::active()` returns `Arc<Theme>`; `uze_theme::set_active(theme)`
swaps it.

*Why not a `&Theme` parameter* threaded through the render functions: 358
call sites across ~40 functions, several of them recursive tree renderers,
would each grow a parameter that can never legitimately differ between them —
the theme cannot change mid-frame. That is a large, permanent readability
cost for an invariant a global expresses better.

*Why not a plain `OnceLock`:* the spec requires switching themes in a running
TUI without restarting. `RwLock<Arc<_>>` gives a live swap with an
uncontended read on the draw path (the write happens once, from the input
half, between frames).

*Testability:* tests that assert appearance set the active theme in their own
scope; the render tests assert *tokens* (via the adapter's inverse lookup),
not RGB triples, so they stop caring about the global entirely.

### 6. Symbols carry a glyph and its display width, not just a string

`SymbolDef { glyph: String, width: u8 }`, width computed from the glyph at
load (unicode display width), overridable by the theme when a Nerd Font
glyph lies about its width. Call sites that lay out columns use the width
from the resolved symbol rather than `str::width` on a literal.

*Why:* the spec requires alignment to survive a replaced glyph. Today the
widths are implicit in the literals (`"● "` includes its trailing space —
`AgentTabStatus::glyph` returns the pad as part of the glyph). Making width
explicit is what lets a theme swap `●` for `*` or `` without every
containing row needing to know.

Spinner-style animated symbols are a `SymbolDef` list under one name
(`Symbol::SpinnerFrames`), so a theme replaces the whole animation
coherently rather than ten unrelated entries.

### 7. Geometry stays in code

Paddings, minimum widths and column widths are *layout invariants* of a
keyboard-driven TUI, not appearance: `MIN_SIDEBAR_WIDTH` exists because the
sidebar's content stops being readable below it, and a theme that changed it
would break the layout rather than restyle it. The duplicated constants
(`TRAILING_PAD` ×4, `H_PAD` ×5, two conflicting `MIN_CONTENT_WIDTH`) are
consolidated into named constants in this change because the migration
touches those lines anyway — but they do not enter the schema.

### 8. `uze-terminal` is configured, not coupled

`RuntimeConfig` gains `default_foreground`, `default_background`, and
`ansi_palette: [Rgb; 16]`; the two `const`s at `runtime.rs:1172` are deleted.
The client passes the active theme's values on attach. No protocol change —
these are server-construction inputs, and `uze-terminal` keeps depending on
nothing in the workspace.

### 9. The syntect theme name is a theme field, passed as an argument

`uze-extensions`'s highlighter takes the theme name as a parameter instead of
naming `base16-ocean.dark`. The theme file declares `syntax.theme`, validated
at load against syntect's bundled set so a typo is a named load error rather
than a panic at first diff.

### 10. `Token` is a flat enum with a dotted serialized name

`Token::TextMuted` serializes as `"text.muted"`, `Token::SurfaceSelected` as
`"surface.selected"`. Flat in Rust (exhaustive `match` in the resolver,
no nesting to walk); dotted in JSON (groups read naturally to an author,
and the schema can document them by prefix). Roughly 35 tokens in five
groups: `base.*`, `text.*`, `surface.*`, `border.*`, `state.*`, plus
`ansi.*` for the pane's 16.

*Naming rule:* a token names what the thing *is*, never what it looks like.
`CYAN`/`VIOLET` — which exist today only because the five state hues were
taken — become `state.in-flight` and `state.landed`. A theme is then free to
make both green.

### 11. Migration is enforced by a rule added *before* the migration

`tests/architecture/layering.rs` gains a rule: `Color::Rgb` may not appear in
`src/` outside `src/ui/theme.rs`, with the existing `budget` mechanism
carrying the count down as files migrate. This is what makes a 17-file
mechanical migration safe to do incrementally without a half-migrated state
becoming permanent — the same technique the suite already uses for the
`uze_core::` reaches.

### 12. Appearance resolves as a stack, and the operator's overrides are not a theme

Added after the first implementation, when writing a real third-party
palette (Dracula) showed that "a theme file, completed from the default" is
not enough for two things people obviously want: a *variation* of a theme,
and per-machine tweaks that survive switching themes.

**Chosen:** one resolver over an ordered stack of `ThemeFile` layers —
built-in default, the ancestry a theme declares with `extends`, the theme,
then `~/.uze/theme-overrides.json`. Merging stays at the *declaration*
level at every level, which is what carries an ancestor's references
through a descendant that repaints what they point at.

*Prior art, and why not the alternatives.* VS Code keeps a theme in one
large JSON contributed by an extension, with `include` for reuse inside
that extension and a *separate* user-settings layer
(`workbench.colorCustomizations`) that overrides whichever theme is
active — two mechanisms, because authoring a variant and tweaking your own
machine are different acts. Helix does the first with a single
`inherits = "…"` key in the theme file; Zed does it by shipping a *family*
file carrying several named variants. Terminals (Alacritty, Ghostty) do the
second with a `theme = name` plus explicit palette overrides in the user's
own config.

We take Helix's `extends` for authoring and VS Code's separate layer for
the operator, and skip Zed's family file: a family is expressible as two
files where one extends the other, and one-file-many-themes would mean an
id that is a path into a document rather than a filename.

*Why the overrides file is not simply a theme with `extends: "<active>"`:*
because the active theme changes, and the whole point is that they do not
have to edit anything when it does. It also never becomes selectable, which
is right — it is not a look, it is their machine.

### 13. A colour bound to a hue by contract does not follow a meaning

The sixteen indexed colours a program inside a pane can name were aliased
to semantic tokens (`ansi.2` was `@accent`), which reads well and is wrong:
UZE's accent is green, so it worked by coincidence. Under Dracula, whose
accent is purple, a pane printing green came out purple.

**Chosen:** the twelve hue-bound entries are literals in the default;
only the four that are genuinely a role — background, foreground, and its
dim and bright forms — stay aliases. A theme wanting a coherent pane
declares its own sixteen, which any real third-party palette already has.

## Candidate ADRs

- **A design system is a crate, not a module** — new crate boundary,
  expensive to unwind once 676 call sites depend on it.
- **Appearance is data resolved at load, drawing is a lookup** — the
  alpha-compositing/alias schema and the global resolved theme are a pattern
  every future surface must follow.

## Risks / Trade-offs

- **A 676-reference mechanical migration is where visual regressions hide.**
  → The default theme is byte-identical to today's constants, and the
  migration is per-file with the architecture rule's budget decreasing each
  step, so a diff that changes appearance is a diff that changed a token
  mapping — visible in review rather than buried in a rewrite.
- **A global active theme is shared mutable state.** → Written once, between
  frames, from the input half; read under `RwLock` on the draw path. The
  alternative (a parameter on 40 functions) costs more than it protects.
- **Alpha compositing over `surface.background` makes token order matter** —
  a theme that aliases its background to something translucent is circular.
  → The loader resolves `base`/`surface.background` first and rejects a
  translucent or aliased background with a named error.
- **A user-supplied theme can make UZE unreadable** (foreground = background).
  → The loader warns on any token whose contrast against its background falls
  below a stated ratio, and `uze theme show` reports it; it does not correct
  it. An unreadable theme the user wrote is the user's call; an unreadable
  theme they cannot diagnose is ours.
- **Symbol width is a display-width guess** for glyphs whose terminal width
  varies (Nerd Font private-use area). → `width` is overridable per symbol in
  the theme file, which is the only thing that can actually be right for a
  given font.
- **The theme file becomes a compatibility surface.** A token renamed later
  breaks every theme in the wild. → Unknown token names are a *warning*, not
  a failure, so a theme written for a newer UZE still loads on an older one;
  and the schema carries a version field from day one.

## Open Questions

- Whether the two built-in themes ship as embedded JSON (parsed at startup,
  same code path as a user theme) or as a Rust literal `Theme` (no parse
  cost). Leaning embedded JSON so the built-ins are also the worked examples,
  but this is a startup-cost question answerable during implementation
  without changing the schema, the specs, or the task breakdown.

## Architecture model update

This change adds a component (`crates/uze-theme`) and a relationship
(Terminal UI / Workspace Client → Design System), so the LikeC4 model under
`docs/architecture/likec4/` is updated as part of it: a `designSystem`
component inside the `uze.core` container, with edges from `terminalUi` and
`workspaceClient`. There is no `arch:validate` script in this repository
(no `package.json`/`justfile`, and the `Makefile` has no arch target), so
validation is `likec4 validate docs/architecture/likec4` run directly, or
skipped with that noted if the toolchain is unavailable locally.
