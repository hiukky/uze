## 1. A glyph set is a thing UZE carries

- [x] 1.1 `load.rs` — split the two lookups that are one today.
  `builtin_names()` narrows to the bundled *themes* (`["default"]`); a new
  `glyph_sets() -> &'static [&'static str]` answers `["default", "ascii",
  "nerd"]`, and `glyph_set_file(id) -> Option<&'static ThemeFile>` answers
  the file for `ascii`/`nerd` and `None` for `default` — because the default
  set is the absence of a layer, not an empty one.
- [x] 1.2 Re-export both from the crate root beside `builtin_names`. Nothing
  in `resolve_stack` changes: it already applies a slice in order, and this
  whole change is one more entry in that slice.
- [x] 1.3 `builtin("ascii")` stops answering a whole `Theme` — the only
  caller is the `active.rs` test, which becomes a `resolve_stack` of
  `[default_file(), glyph_set_file("ascii")]`. Keeping a resolved ASCII
  *theme* alive would keep exactly the confusion this change removes.
- [x] 1.4 Tests over the bundled sets: every symbol a set declares is a name
  this build knows (a typo must not resolve to silence), and the **ASCII set
  declares every entry in `Symbol::ALL`** — one Unicode glyph left in an
  otherwise-ASCII screen breaks the only reason to select it. A set that
  states a difference and inherits the rest is correct by design, so
  completeness is asserted of `ascii` alone.

## 2. The selection

- [x] 2.1 `theme_state::ThemeSelection` gains `glyphs: Option<String>` with
  `#[serde(default)]`, in the same `state/theme.json`. Absent means the
  default set — no migration, because absence already means the right thing.
- [x] 2.2 `theme_state::glyphs(home)` / `set_glyphs(home, id)`, mirroring
  `active`/`set_active`, including the "reading a file that predates the
  field" test.
- [x] 2.3 A test that a selection written by this build still deserializes
  through a `ThemeSelection` without the field — the rollback claim in
  design.md, held by a test rather than by inspection.
- [x] 2.4 `Themes` gains `glyphs()`, `select_glyphs()` and a
  `GlyphSetSummary` list, beside `active()`/`select()`/`list()`. The summary
  carries an id and nothing else: the module's own doc refuses to route
  resolved appearance through the facade, and the layering rule forbids
  `src/` naming `uze_core::` — not `uze_theme`, which is where every glyph
  already comes from. So the preview reads the set's glyphs from the design
  system directly, the way `src/ui/theme.rs` reads every other glyph.

## 3. Resolution

- [x] 3.1 `src/theme.rs` — delete `builtin_layer`. The `match` on the
  literal id `"ascii"` is the thing being removed; the layer it pushed is
  now pushed from `glyph_set_file(&app.themes().glyphs()?)`, between the
  built-in default and the theme's ancestry.
- [x] 3.2 The `named` vector `uze theme show` prints gains the set, in the
  position it applied — a stack whose report omits a layer is worse than no
  report, because the operator uses it to find which layer won.
- [x] 3.3 `install()` applies when *only* a glyph set is chosen. Today the
  condition is "a theme was selected, or overrides exist"; a set is the
  third way to have an opinion, and without this someone selects `nerd`,
  never selects a theme, and nothing happens.
- [x] 3.4 `written()` gains the case that names the move: an id that is a
  glyph set rather than a theme reports so, and names
  `uze theme glyphs <id>`. This is what a machine with `ascii` recorded as
  its active theme meets — an error message, not migration code.
- [x] 3.5 Tests over the assembled stack: a set applies under a theme that
  declares no symbols; a theme's own symbol wins over the set; overrides win
  over both; a set applies with no theme selected. These are the four
  scenarios the spec turns on, and they are cheapest here.

## 4. The `nerd` set

- [x] 4.1 `crates/uze-theme/themes/nerd.json` — `symbols` only, no colours,
  covering the marks, statuses, chevrons and arrows where an icon says the
  thing better than a letterform. **Generated from the official
  `glyphnames.json` by Codicon name**, never by typed codepoint, so a
  transcription error is impossible by construction; the file's description
  records the source and its version. A test pins every glyph to the Codicon
  range, which is what catches a wrong glyph class later.
- [x] 4.2 Declare `width` explicitly on every entry, targeting the `Nerd
  Font Mono` builds. `unicode-width` reports 1 for the private-use area
  whichever build is installed, so an inherited measurement would be right
  only by luck; the file states its assumption instead.
- [x] 4.3 Run the set against the existing bundled-theme no-emoji test.
  Codicons sit in the private-use area rather than the pictographic ranges
  that test scans, so it should stand unchanged — verify rather than assume.
- [x] 4.4 The spinner stays braille in every set. Codicons' `loading` is one
  glyph meant to be spun by CSS — there are no frames — so taking it would
  freeze the only moving thing in the UI. `nerd` leaves `status.working`
  undeclared and inherits the braille cycle, which a patched font draws
  because braille is ordinary Unicode in the base font.

## 5. The CLI

- [x] 5.1 Rename `ThemeAction::Use` to `ThemeAction::Set` — `uze theme set
  <id>`. No alias: pre-1.0, and clap's unknown-subcommand error already
  names what does exist.
- [x] 5.2 `uze theme glyphs [<set>]` — lists the sets marking the active
  one, or selects one. Same text/JSON shape as `theme list`, since it
  answers the same kind of question.
- [x] 5.3 Classify both leaf commands in `command_performance.rs` as
  `Budgeted`; `tests::every_cli_command_is_classified` fails by name
  otherwise.
- [x] 5.4 `theme list` stops carrying `ascii`. Swept `tests/`, `journeys/`
  and `conformance/` for the expectation: nothing asserted it — the only
  references left are in `docs/theming.md`, which task 7.1 rewrites.

## 6. The Appearance route

- [x] 6.1 `Route::Appearance` — the variant, its `label()` ("Appearance"),
  its stable `id()` ("appearance"), its place in `ROUTES`, and `from_id`.
  The id is what remembers where the operator was between runs, so it is
  chosen once and never renamed.
- [x] 6.2 `src/ui/view/appearance.rs`, shaped like `view/keys.rs`: a list
  with a detail side. Two groups — the themes this machine can draw with,
  and the glyph sets UZE carries — each row selectable, the active one
  marked.
- [x] 6.3 The preview strip: each glyph set drawn **in its own glyphs**,
  against a fixed-width ruler, so a set this terminal cannot render shows as
  tofu and a set whose declared widths are wrong shows as misalignment —
  both before the operator commits. This is the part that replaces font
  detection, so it carries the screen.
- [x] 6.4 `Hit` variants for selecting a theme row and a set row, and the
  worker `Intent` that records the choice and calls `uze_theme::set_active`
  with the re-resolved stack. Selecting must not disturb a session, a pane
  or an agent — the theme swap already happens between frames.
- [x] 6.5 A `TestBackend` test in `src/ui/tests.rs` that each set's preview
  renders that set's own glyphs, and that selecting a set redraws the next
  frame with it. Screen text asserted here, where it is cheap and precise,
  rather than in a journey.
- [x] 6.6 Check the architecture suite: the new view names no colour value
  and writes no chrome glyph inline (`layering.rs`), and reaches
  `uze_application` only — never `uze_core::`. The preview strip is the one
  place in the codebase that legitimately renders a glyph it did not resolve
  from the *active* theme, so it needs the read model to hand it the set's
  glyphs rather than reaching for them.

## 7. Docs and the gate

- [x] 7.1 `docs/theming.md` — the axis, the layer order in one diagram, and
  the two selections as separate acts. The "Your own overrides" section
  narrows to what it was always for: the individual glyph your own font
  draws differently, and the `width` override for a non-Mono build.
- [x] 7.2 Document that the `nerd` set needs Nerd Fonts v3, and why the
  answer to "which font do I have?" is the preview rather than a question.
- [x] 7.3 `make check` clean: `cargo fmt --check`, `cargo clippy
  --all-targets -- -D warnings`, the workspace suite, and `openspec validate
  --all --strict`.
- [x] 7.4 Re-read `docs/architecture/invariants.md` for anything the deleted
  `builtin_layer` was named in, and update it if so.

## 8. The public docs

Tasks 7.1 and 7.2 rewrote `docs/theming.md` and left the site untouched,
which is the surface most readers actually meet. This group closes that,
and adds the page the Keys screen has never had.

- [x] 8.1 `web/content/docs/theming.mdx` becomes `appearance.mdx` — the page
  is about two axes now, and one of them is not a theme. Add a redirect in
  `web/next.config.mjs` so the published `/docs/theming` still resolves;
  there is no redirect table today, so this creates it.
- [x] 8.2 A **Fonts** section, which is the thing a reader actually needs
  and no page currently says: install a Nerd Font and select the `nerd`
  set, what each of the three sets is for, and the measured coverage — the
  `default` set relies on font fallback for five symbols even on a patched
  JetBrains Mono, and for twenty-seven of forty-two on Ubuntu Mono.
- [x] 8.3 Say plainly why uze never detects the font: no escape sequence
  answers it, `$TERM` names the emulator, and a cursor probe measures width
  rather than presence. The preview is the interface, and that is a
  decision rather than a gap.
- [x] 8.4 `web/content/docs/keys.mdx` — the Keys screen and `keys.json` have
  no page on the site at all, only `docs/keymap.md` in the repository.
- [x] 8.5 A `Customization` section in `web/content/docs/meta.json` holding
  both, since they are the two things a reader changes to fit their own
  machine rather than reference for what uze does.
- [x] 8.6 `web/content/docs/cli.mdx` — `theme use` is gone; `theme set` and
  `theme glyphs` are what exist.
- [x] 8.7 `docs/theming.md` renamed to `docs/appearance.md` for the same
  reason, and the reference in `AGENTS.md` updated with it.

## 9. Icons where they earn their keep

Opened after the first eight closed, because seeing the sets in a working
terminal is what showed where the vocabulary was thin and where it was
being misused.

- [x] 9.1 A `file.` family in the vocabulary — kinds, never languages: a
  theme can be asked to draw "source code" and cannot be asked to draw "a
  Rust file". The built-in sets leave them blank, which is the honest
  answer rather than a hole: plain Unicode has no folder or document mark
  a terminal does not take from its emoji font.
- [x] 9.2 `NavigatorRow` carries a `RowIcon` — what the row *is* — so the
  extension classifies and the host draws, the way every other mark works.
  The changes list asks for none: it marks status, and two marks per row
  is one too many.
- [x] 9.3 Classification by whole name as well as extension: `Makefile`
  and `LICENSE` carry their meaning without one, and they are exactly the
  files at the root of every repository.
- [x] 9.4 `task.ready` stops borrowing `arrow.shift`, which the vocabulary
  defines as the shift *key* in a hint line — a theme repainting the
  keyboard's marks would have repainted a task's standing with them.
- [x] 9.5 `mark.sparkle` becomes `oct-sparkle_fill`, the one deliberate
  non-Codicon, named in the test so staying an exception is a decision.
- [x] 9.6 The empty states say two things instead of one: what is the
  matter, and what to do about it. Set a third of the way down rather than
  pinned to the top edge, where a line of text reads as a document that
  got cut off.
- [x] 9.7 The Appearance drawer follows the house pattern — a recessed
  slab behind a left rule, a drag handle on it, labelled blocks — and its
  preview takes only the marks the column has room for.
