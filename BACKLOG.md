# Backlog

Ordered by impact × risk × effort. Bugs and security first, then
orchestration robustness, then architecture that blocks other work, then
measured performance, then readability.

## Open

| # | Item | Why | Size |
|---|------|-----|------|
| 5 | Host-only gestures inside `ViewHit` (`GrabNavigatorEdge`, `DragContentScrollbar`, `ToggleSection`, `ResizeSection`) | Skipped twice as not contained: they ride `ExtensionHit`, shared by the management modal. Extension contract carries variants no extension handles. | M |
| 8 | CLI startup cost not measured outside the per-command budget tests | `hyperfine` is not installed (QUESTIONS #4); the budget tests already fail a command that regresses past its class. | S |
| 11 | Tests that wait with `thread::sleep` polling (30 sites; 11 in `uze-terminal`'s runtime tests waiting on PTY output) | Poll-and-sleep is the flake pattern the project forbids; the damage channel already gives an event to wait on, as the new pane tests do. | M |
| 12 | Coverage gaps in the client: `src/ui/worker.rs` 20.7 %, `src/ui/orchestrator/session.rs` 44 %, `src/self_update.rs` 68 % | Workspace total is 85 % lines, but the intent dispatch the TUI depends on is the least exercised code. | M |
| 10 | Duplicate transitive crates (`base64`, `bitflags`, `hashbrown`, `signal-hook`, `syn`, `thiserror`, `windows-sys`) | All transitive; resolvable only by upstream bumps. Track, don't act. | — |

## Done

See `REFACTOR_LOG.md`.
