# uze-extensions

Built-in TUI extensions. An extension answers with a `view::View` (a
full-frame surface) or a `view::Section` (a collapsible block of one of the
host's own columns) and never draws, computes geometry, or names a colour —
`src/ui/extension_view.rs` renders both.

- `code/` — the active checkout four ways: its changes, its files, its history, its map
- `architect/` — what somebody *wrote down* about the project, nothing measured
- `shared/` — what a second extension actually needed, and only then
- `view.rs`, `Host` — the two contracts, one in each direction

Every capability (running Git, reading a file, resolving `$HOME`) arrives
through `Host`. This crate depends on no other UZE crate and names no
process, filesystem or environment API; the architecture suite fails on each.

```bash
cargo test -p uze-extensions
```

See ADR-041 for why extension code is a different trust class from plugin bytes.
