## 1. Selection

- [x] 1.1 `src/ui/orchestrator/selection.rs`: `PaneSelection` (press, follow clamped to the pane, reading-order `contains`, `text` without row padding or wide-character spill), with unit tests
- [x] 1.2 `osc52` and its std-only base64, tested against the RFC 4648 vectors
- [x] 1.3 Draw the selection in `render_pane` by reversing each selected cell's own style

## 2. Gesture ownership

- [x] 2.1 Press over a pane starts a selection unless Shift is held over a program that asked for the mouse; any press or key puts an existing selection away
- [x] 2.2 Drag follows the selection and forwards nothing to the program
- [x] 2.3 Release copies a non-empty selection (toast + `WorkspaceModel::clipboard`); a release that never moved forwards the held press and the release to the program
- [x] 2.4 Frame loop emits a pending copy through `TerminalSession::emit` between frames
- [x] 2.5 Driven tests: drag copies; drag over a mouse-owning program copies and forwards nothing; a click reaches that program on release; Shift hands it the drag

## 3. Validation

- [x] 3.1 `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo test --lib`, architecture suite
- [x] 3.2 Operator validates by hand in Windows Terminal (WSL): a shell pane, a Claude Code pane, a Codex pane; click, drag, Shift-drag, paste on the Windows side
- [x] 3.3 Journey `04-workspace/09-text-copied-from-a-pane`: a drag over a shell and over a program that owns the mouse, checked against the bytes the app wrote to its terminal and the input the program received (runner gains `drag` and `open: tap:`)
- [x] 3.4 `openspec validate pane-copy-on-select --strict`
