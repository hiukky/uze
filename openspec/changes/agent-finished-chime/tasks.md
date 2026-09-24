## 1. Record and service

- [x] 1.1 `uze-core::config`: `config.toml` read/set per section via `toml_edit`, preserving the rest; malformed file refused on read and write
- [x] 1.2 `UzeHome::config_path()` at the root; `state/theme.json` no longer read (alpha: no legacy path)
- [x] 1.3a `appearance` reads and writes `[appearance]`; `notifications` (`Chime`) reads and writes `[notifications]`, unknown value → Silent
- [x] 1.3 `uze-application`: `notifications()` service (`agent_finished`, `set_agent_finished`) and `Chime` re-export

## 2. Ringing

- [x] 2.1 A process-wide chime cell in `src/ui`, loaded at TUI start
- [x] 2.2 `TerminalSession::ring` writing BEL through the backend
- [x] 2.3 `expire_agent_activity` raises a due ring according to the choice (focused vs background)
- [x] 2.4 The workspace loop rings once after drawing, coalesced by a cooldown
- [x] 2.4a A settle window (`CHIME_SETTLE`): a turn rings only after staying ended; resumed work or an acknowledged check drops it
- [x] 2.5 Model tests: Out of sight ignores the focused pane, rings for a background one; Always rings for both; Silent never; two expiries coalesce

## 3. Manage modal

- [x] 3.1 Settings rows (the Appearance route renamed): a "Notifications" group with one card per choice, the active one marked
- [x] 3.2 Drawer text for a chime card
- [x] 3.3 Worker intent: persist the choice, update the live cell, preview-ring a ringing choice
- [x] 3.4 Model test: the group lists the three choices, marks the one in force, and Enter on one yields its intent

## 4. Docs

- [x] 4.0 `appearance.mdx` (where the selection lives, hand-editing) and `workspace/index.mdx` (when an agent finishes)

## 5. Gate

- [x] 5.1 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`
- [x] 5.2 `openspec validate agent-finished-chime --strict`
