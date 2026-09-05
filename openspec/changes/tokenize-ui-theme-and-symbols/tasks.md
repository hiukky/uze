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

- [ ] 3.1 Add a rule to `tests/architecture/layering.rs`: `Color::Rgb` may not appear in `src/` outside `src/ui/theme.rs`, with the current count as the starting `budget` and the reason/remedy written for someone hitting it cold
- [ ] 3.2 Add a second rule for symbols once §5 lands: no chrome glyph literal in `src/ui/**` outside the symbol adapter (scan for the inventoried glyph set), same budget mechanism

## 4. The ratatui adapter

- [ ] 4.1 `src/ui/theme.rs`: the one place converting `uze_theme::Rgb` to `ratatui::style::Color`, exposing `fg(Token)`, `bg(Token)`, `style(Token)` and the symbol lookups the render code will call
- [ ] 4.2 An inverse lookup (`Color → Token`) used only by tests, so render assertions name meanings and stop asserting RGB triples
- [ ] 4.3 Delete the palette block in `src/ui.rs` and `NAV_INACTIVE`/`LINE_ADDED_BG`/`LINE_REMOVED_BG`, leaving the adapter as the only definition

## 5. Migrating the TUI, file by file

Each task below is one file taken to zero raw colours and zero chrome glyph literals, lowering both architecture budgets, with its own tests updated to assert tokens:

- [ ] 5.1 `src/ui.rs` (chrome shared by both modes) and `src/ui/view.rs`
- [ ] 5.2 `src/ui/orchestrator/render.rs` — the largest surface (95 style call sites); take the six meaning→colour helpers (`caption_color`, `AgentTabStatus::color`/`glyph`, `task_mark`) to tokens and symbols first, since they are the shape every other site should end in
- [ ] 5.3 `src/ui/orchestrator.rs` and `src/ui/orchestrator/session.rs`
- [ ] 5.4 `src/ui/overlay.rs` and `src/ui/agent_support.rs`
- [ ] 5.5 `src/ui/management.rs` and `src/ui/root_picker.rs`
- [ ] 5.6 `src/ui/view/plugins.rs`, `harnesses.rs`, `overview.rs`, `profiles.rs`, `extensions.rs`, `health.rs`
- [ ] 5.7 `src/ui/extension_view.rs`: map `uze_extensions::view::Role` → `Token` (the crate itself stays untouched and gains no dependency), and move `LINE_ADDED_BG`/`LINE_REMOVED_BG` to `state.diff-added`/`state.diff-removed`
- [ ] 5.8 Consolidate the duplicated geometry constants the migration touched — `TRAILING_PAD` (4 definitions), `H_PAD` (5), and the two conflicting `MIN_CONTENT_WIDTH` values — into one named constant each, in code, not in the schema
- [ ] 5.9 Both architecture budgets reach zero; delete the budget entries rather than leaving them at `0`

## 6. The other three surfaces

- [ ] 6.1 `src/progress.rs`: resolve the same tokens through a small `anstyle` adapter and delete its five palette constants; `NO_COLOR`/non-TTY behaviour unchanged, with a test that the CLI and the TUI report the same value for a shared token
- [ ] 6.2 `crates/uze-terminal`: add `default_foreground`/`default_background`/`ansi_palette` to the runtime's construction config, delete `REPLY_BACKGROUND`/`REPLY_FOREGROUND`, and answer OSC 10/11 from the configured values — no protocol change, no new workspace dependency
- [ ] 6.3 The workspace client passes the active theme's values on attach, and `TerminalColor::Indexed` resolves through `ansi.*` instead of `Color::Indexed` passthrough
- [ ] 6.4 `crates/uze-extensions/src/git.rs`: take the syntect theme name as an argument instead of `base16-ocean.dark`; the host passes the active theme's `syntax.theme`

## 7. Selection, storage and product surface

- [ ] 7.1 `UzeHome::themes_dir()` and the active-theme state path in `uze-core`; `uze-application` exposes reading/writing the selection so `src/` never names `uze_core::`
- [ ] 7.2 Load the selected theme at startup for both the CLI and the TUI; a missing, unparseable or invalid theme reports the file and the problem by name and continues on the built-in default
- [ ] 7.3 `uze theme list|use|show` — `list` marks the active one and includes the built-ins; `show` prints the resolved tokens plus any load warnings; classify all three as `Budgeted` in `src/command_performance.rs`
- [ ] 7.4 A theme switch inside the TUI: pick a theme, `set_active`, redraw on the next frame with no session, pane or agent disturbed
- [ ] 7.5 Ship the second built-in theme (`ascii`: every symbol within ASCII, colours unchanged) and confirm no layout depends on a glyph the terminal cannot render

## 8. Evidence and documentation

- [ ] 8.1 Cover each spec scenario with a test: partial theme, malformed theme falls back with a named error, alias/alpha resolution, symbol replacement and width, ASCII theme, one token reaching all three surfaces, OSC 10/11 reporting the active background, the extension's chrome following the theme while its own `Rgb` content passes through
- [ ] 8.2 Update `docs/architecture/invariants.md` with the appearance invariant and the test that proves it, in the form the file already uses
- [ ] 8.3 Add the `designSystem` component and its edges to `docs/architecture/likec4/model.c4`, and validate the model (no `arch:validate` script exists in this repo — run `likec4 validate docs/architecture/likec4` directly, or record that the toolchain was unavailable)
- [ ] 8.4 Document authoring a theme — the schema, the three colour forms, the symbol set — as one page with a canonical owner, and update `AGENTS.md`'s workspace layout with the new crate and its rule
- [ ] 8.5 `make check` green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full workspace suite, `cargo deny check`, `make attributions`, and `openspec validate --all --strict`
