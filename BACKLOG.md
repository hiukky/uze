# Backlog

Ordered by impact × risk × effort. Bugs and security first, then
orchestration robustness, then architecture that blocks other work, then
measured performance, then readability.

## Open

| # | Item | Why | Size |
|---|------|-----|------|
| 5 | Host-only gestures inside `ViewHit` (`GrabNavigatorEdge`, `DragContentScrollbar`, `ToggleSection`, `ResizeSection`) | Skipped twice as not contained: they ride `ExtensionHit`, shared by the management modal. Extension contract carries variants no extension handles. | M |
| 8 | CLI startup cost not measured outside the per-command budget tests | `hyperfine` is not installed (QUESTIONS #4); the budget tests already fail a command that regresses past its class. | S |
| 12 | `src/ui/orchestrator/session.rs` coverage (44 % before this pass) | Characterized since: worker results, closing the last space, the space row's landing, the context menu's keys. Still untested: hover tooltips, tab drag. Low risk; visual. | S |
| 13 | No property tests for task, slot and space state | The transitions are small and covered by example tests; `proptest` would be a new dev-dependency and needs a concrete invariant example tests miss before it earns one. | M |
| 10 | Duplicate transitive crates (`base64`, `bitflags`, `hashbrown`, `signal-hook`, `syn`, `thiserror`, `windows-sys`) | All transitive; resolvable only by upstream bumps. Track, don't act. | — |

## Done

See `REFACTOR_LOG.md`.
