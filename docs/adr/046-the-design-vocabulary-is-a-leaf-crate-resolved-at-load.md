# The design vocabulary is a leaf crate, resolved at load

Status: Accepted

## Context

UZE's appearance was 25 `const Color`s in `src/ui.rs`, referenced ~676
times across 17 files. Only 6 of the 358 `.fg()`/`.bg()` call sites mapped
a *meaning* to a colour; the rest named a colour at the point of drawing.
Six of those constants were hand-composited — their doc comments read
`rgba(255,255,255,0.09)` pre-blended over `BASE`, because ratatui has no
alpha — so a second theme would have meant redoing that arithmetic by
hand. Three surfaces already carried their own copy of the palette:
`src/progress.rs` (the CLI, drawn with anstyle, not ratatui),
`uze-terminal`'s OSC 10/11 replies, and the syntect theme name in
`uze-extensions`. Around 40 glyphs were inline literals.

That shape has one failure mode and it had already happened three times:
the design system lives next to whichever code draws first, and every
other surface grows a copy. Anything that makes appearance authorable
from outside the binary has to answer where the vocabulary lives before
it can answer what a theme file looks like.

The existing layering rules bound the answer: `src/` may not name
`uze_core::`; `uze-extensions` depends on no UZE crate and names no
filesystem, process or environment API; `uze-terminal` depends on nothing
in the workspace.

## Decision

The design vocabulary is its own leaf crate, `uze-theme`, and a theme is
data resolved once at load — drawing is a table lookup.

- **A crate, not a module.** `crates/uze-theme` depends on `serde`,
  `serde_json` and `thiserror` only. It exposes its own `Rgb`, the
  `Token` and `Symbol` enums, the theme file schema and the resolver. A
  module in `src/ui/` was rejected because `src/progress.rs` is a
  first-class consumer that does not draw with ratatui, and burying the
  system inside the TUI is what produced the duplication in the first
  place; a module in `uze-core` was rejected because appearance is not
  domain and `src/` may not name `uze_core::`.
- **A leaf.** It resolves no path, reads no environment and names no
  rendering library. The caller passes the directory — `src/` gets it
  from `UzeHome::themes_dir()` through `uze-application`, so no layering
  rule moves. Every consumer adapts `uze_theme::Rgb` to what it draws
  with.
- **Resolution happens at load.** A theme declares a colour as
  `#rrggbb`, as `#rrggbbaa` composited over the theme's own
  `surface.background`, or as `@alias`. The loader merges the partial
  theme over the built-in default, resolves aliases, composites alpha and
  validates; `Theme` is a flat `Token → Rgb` / `Symbol → SymbolDef`
  table with no aliases, no alpha and no `Option` at draw time. A
  malformed theme is rejected at load with the default kept active,
  never half-applied.
- **The active theme is process-global**, behind `RwLock<Arc<Theme>>`,
  written between frames from the input half and read uncontended on the
  draw path. Threading a `&Theme` through 358 call sites across ~40
  functions — several recursive — would add a parameter that can never
  legitimately differ between them.
- **The vocabulary is enforced, not offered.** Nothing outside
  `crates/uze-theme` and `src/ui/theme.rs` may name a colour value or
  write a chrome glyph inline; two rules in
  `tests/architecture/layering.rs` fail the build over it.

Two things deliberately stay out. Geometry — paddings, minimum widths —
is a layout invariant of a keyboard-driven TUI, not appearance, and does
not enter the schema. `uze-extensions` keeps its own 12-variant
`view::Role` and gains no dependency on the theme crate; the host maps
`Role` to `Token`, which keeps the extension ABI still while the schema
grows and keeps the trust surface ADR-041 exists to narrow from widening.

## Consequences

Adding a surface now means naming a meaning. "Is this part of the design
system?" became a compile question, and the answer to "where does this
colour come from?" is one crate for every surface UZE draws, including
the ones that do not use ratatui.

A theme file is a few lines and no Rust. The alpha form is what makes a
light theme cheap: the same `#ffffff17` declaration composited over a
light background yields the light shade without the author recomputing
anything.

The cost is a global with interior mutability, and a leaf crate whose
schema must be versioned as themes are written against it. The global is
the honest expression of an invariant — the theme cannot change
mid-frame — and it is what let the render tests assert tokens rather than
RGB triples, so they no longer care about it at all.

A full colour algebra (`lighten`, `mix`) is deferred. Alpha-over-
background covers every derived colour that exists; anything more is a
language, and a language in a config file needs a stronger reason than
six shades.

Source change: openspec/changes/tokenize-ui-theme-and-symbols/
