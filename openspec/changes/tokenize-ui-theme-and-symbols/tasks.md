## 1. The crate and its vocabulary

- [x] 1.1 Create `crates/uze-theme` (edition 2024, workspace version, `serde`/`serde_json`/`thiserror`), add it to `[workspace] members`, and give `src/lib.rs` a module doc stating what belongs in it: appearance vocabulary and nothing that knows the machine, the domain, or a rendering library
- [x] 1.2 Define `Rgb(u8,u8,u8)` and the `Token` enum — flat in Rust, dotted in serde (`base.*`, `text.*`, `surface.*`, `border.*`, `state.*`, `ansi.*`) — naming meanings, not hues: today's `CYAN`/`VIOLET` become `state.in-flight`/`state.landed`
- [x] 1.3 Define the `Symbol` enum covering every glyph the TUI draws today (~40, inventoried from the literals in `src/ui.rs` and `src/ui/**`), plus `SymbolDef { glyph, width }` and the multi-frame form for spinners
- [x] 1.4 Write the built-in default theme with values byte-identical to today's 25 colours and every current glyph, and assert that identity in a test that names each token against the literal it replaces — this is the test that makes the whole migration a no-op visually

## 2. Schema, loader, resolver

- [x] 2.1 `ThemeFile` serde types: a `version` field, `colors` (token → declaration), `symbols` (symbol → glyph or `{glyph,width}`), `syntax.theme`, and a `name`/`description`
- [x] 2.2 Colour declaration parsing: `#rrggbb`, `#rrggbbaa`, `@token` alias; reject a malformed literal with an error naming the token and the offending text
- [x] 2.3 Resolution order: resolve `surface.background` first (rejecting a translucent or aliased background by name), then aliases (rejecting cycles by naming the cycle), then composite every `#rrggbbaa` over the background
- [x] 2.4 Merge a partial theme over the built-in default so every token and symbol resolves; an undeclared entry takes the default's value
- [x] 2.5 Unknown token/symbol names load with a warning, never a failure, so a theme written for a newer UZE still loads — return the warnings alongside the `Theme` rather than logging from the crate
- [x] 2.6 Contrast check: report (never correct) any token whose contrast against its background falls below the stated ratio, as a warning on the same channel
- [x] 2.7 Validate `syntax.theme` against syntect's bundled theme names at load, so a typo is a named load error and not a panic at first diff
- [x] 2.8 Publish the JSON Schema for a theme file and a test asserting the built-in themes validate against it
- [x] 2.9 `active()`/`set_active()` over `RwLock<Arc<Theme>>`, defaulting to the built-in theme with no I/O, so any consumer works before a theme is ever loaded

## 3. The migration rule, before the migration

- [x] 3.1 Add a rule to `tests/architecture/layering.rs`: `Color::Rgb` may not appear in `src/` outside `src/ui/theme.rs`, with the current count as the starting `budget` and the reason/remedy written for someone hitting it cold
- [x] 3.2 Add a second rule for symbols once §5 lands: no chrome glyph literal in `src/ui/**` outside the symbol adapter (scan for the inventoried glyph set), same budget mechanism

## 4. The ratatui adapter

- [x] 4.1 `src/ui/theme.rs`: the one place converting `uze_theme::Rgb` to `ratatui::style::Color`, exposing `fg(Token)`, `bg(Token)`, `style(Token)` and the symbol lookups the render code will call
- [x] 4.2 An inverse lookup (`Color → Token`) used only by tests, so render assertions name meanings and stop asserting RGB triples
- [x] 4.3 Delete the palette block in `src/ui.rs` and `NAV_INACTIVE`/`LINE_ADDED_BG`/`LINE_REMOVED_BG`, leaving the adapter as the only definition

## 5. Migrating the TUI, file by file

Each task below is one file taken to zero raw colours and zero chrome glyph literals, lowering both architecture budgets, with its own tests updated to assert tokens:

- [x] 5.1 `src/ui.rs` (chrome shared by both modes) and `src/ui/view.rs`
- [x] 5.2 `src/ui/orchestrator/render.rs` — the largest surface (95 style call sites); take the six meaning→colour helpers (`caption_color`, `AgentTabStatus::color`/`glyph`, `task_mark`) to tokens and symbols first, since they are the shape every other site should end in
- [x] 5.3 `src/ui/orchestrator.rs` and `src/ui/orchestrator/session.rs`
- [x] 5.4 `src/ui/overlay.rs` and `src/ui/agent_support.rs`
- [x] 5.5 `src/ui/management.rs` and `src/ui/root_picker.rs`
- [x] 5.6 `src/ui/view/plugins.rs`, `harnesses.rs`, `overview.rs`, `profiles.rs`, `extensions.rs`, `health.rs`
- [x] 5.7 `src/ui/extension_view.rs`: map `uze_extensions::view::Role` → `Token` (the crate itself stays untouched and gains no dependency), and move `LINE_ADDED_BG`/`LINE_REMOVED_BG` to `state.diff-added`/`state.diff-removed`
- [x] 5.8 Consolidate the duplicated geometry constants the migration touched — `TRAILING_PAD` (4 definitions) and the popup inset `H_PAD`/`V_PAD` (5) become one shared pair in `src/ui.rs`. Correction to the survey this task came from: the two `MIN_CONTENT_WIDTH` values are not a duplication but two different measurements sharing a name (the sidebar's content floor, the extension navigator's), so the extension's is renamed rather than merged; and the status catalog's inset of 1 is deliberate, so it keeps its own name
- [x] 5.9 The TUI's colour budget reaches zero; `src/progress.rs` keeps its 5 until §6.1. The symbol rule is a dedicated test rather than a `Rule` — one needle per glyph would have meant twenty-odd rules, and the arrows and middot in hint lines are authored notation the renderer translates, which a blanket scan cannot tell from a mark

## 6. The other three surfaces

- [x] 6.1 `src/progress.rs`: resolve the same tokens through a small `anstyle` adapter and delete its five palette constants; `NO_COLOR`/non-TTY behaviour unchanged, with a test that the CLI and the TUI report the same value for a shared token
- [x] 6.2 `crates/uze-terminal`: add `default_foreground`/`default_background`/`ansi_palette` to the runtime's construction config, delete `REPLY_BACKGROUND`/`REPLY_FOREGROUND`, and answer OSC 10/11 from the configured values — no protocol change, no new workspace dependency
- [x] 6.3 The workspace client passes the active theme's values on attach, and `TerminalColor::Indexed` resolves through `ansi.*` instead of `Color::Indexed` passthrough
- [x] 6.4 `crates/uze-extensions/src/git.rs`: take the syntect theme name as an argument instead of `base16-ocean.dark`; the host passes the active theme's `syntax.theme`

## 7. Selection, storage and product surface

- [x] 7.1 `UzeHome::themes_dir()` and the active-theme state path in `uze-core`; `uze-application` exposes reading/writing the selection so `src/` never names `uze_core::`
- [x] 7.2 Load the selected theme at startup for both the CLI and the TUI; a missing, unparseable or invalid theme reports the file and the problem by name and continues on the built-in default
- [x] 7.3 `uze theme list|use|show` — `list` marks the active one and includes the built-ins; `show` prints the resolved tokens plus any load warnings; classify all three as `Budgeted` in `src/command_performance.rs`
- [x] 7.4 A theme switch inside the TUI: pick a theme, `set_active`, redraw on the next frame with no session, pane or agent disturbed
- [x] 7.5 Ship the second built-in theme (`ascii`: every symbol within ASCII, colours unchanged) and confirm no layout depends on a glyph the terminal cannot render

## 8. Evidence and documentation

- [x] 8.1 Cover each spec scenario with a test: partial theme, malformed theme falls back with a named error, alias/alpha resolution, symbol replacement and width, ASCII theme, one token reaching all three surfaces, OSC 10/11 reporting the active background, the extension's chrome following the theme while its own `Rgb` content passes through
- [x] 8.2 Update `docs/architecture/invariants.md` with the appearance invariant and the test that proves it, in the form the file already uses
- [x] 8.3 Add the `designSystem` component and its edges to `docs/architecture/likec4/model.c4`, and validate the model (no `arch:validate` script exists in this repo — run `likec4 validate docs/architecture/likec4` directly, or record that the toolchain was unavailable)
- [x] 8.4 Document authoring a theme — the schema, the three colour forms, the symbol set — as one page with a canonical owner, and update `AGENTS.md`'s workspace layout with the new crate and its rule
- [x] 8.5 Gate green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full workspace suite (31 suites, 0 failures), and `openspec validate --all --strict` (25/25). `cargo deny check` and `make attributions` could not run — neither `cargo-deny` nor `cargo-about` is installed on this machine — but `Cargo.lock` is unchanged by this work (`unicode-width` and the `syntect` dev-dependency were already in the tree), so there is no new third-party code for either to have an opinion about; CI runs both

## 9. Review, and what a third-party palette exposed

Writing a real Dracula theme found three defects the built-in palette could not, because in it `accent` and `state.success` are the same green:

- [x] 9.1 A fifth colour form, `@token/aa` — another token's value at that alpha over the background. Its absence is why the next two existed: a tint had no way to name what it tinted
- [x] 9.2 `surface.selected` and both diff washes were literal alpha values of UZE's own sage and red, so a theme that repainted the accent still got a green selected row. They now name the token they tint
- [x] 9.3 `ansi.1`–`ansi.6` aliased semantic tokens, so a theme with a purple accent made the terminal's *green* purple. The sixteen are bound to a hue by contract; only the four that are genuinely a role (background, foreground, dim, bright) stay aliases
- [x] 9.4 Theme resolution existed in three places (`theme::install`, `main.rs::resolve_theme`, `worker.rs::select_theme`). One `theme::resolve` now, in the library half so both surfaces reach it

## 10. Themes as layers: variations and the operator's own overrides

- [x] 10.1 `resolve_stack(id, layers)` replaces the file-plus-base resolver: appearance arrives in layers, and merging still happens between declarations so an ancestor's references survive a descendant repainting what they point at
- [x] 10.2 `extends` in a theme file, walked in `src/theme.rs` (which owns path resolution; `uze-theme` still resolves none). Loop detection naming the loop, and a depth cap
- [x] 10.3 Extending a built-in works — `extends: "ascii"` gives a theme ASCII glyphs and keeps its own colours
- [x] 10.4 `~/.uze/theme-overrides.json`: the operator's last word, applied over whichever theme is active and surviving a change of theme. Beside `themes/` rather than in it, because it is not selectable and never stops applying
- [x] 10.5 Overrides apply even with no theme selected — a Nerd Font belongs to the machine, not to having chosen a palette
- [x] 10.6 `uze theme show` reports the layers it resolved from, and a resolution failure names the theme (and its ancestry) rather than only the token
- [x] 10.7 Schema, spec, `docs/theming.md` and tests for all of it
