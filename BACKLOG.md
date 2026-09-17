# Backlog

Ordered by impact × risk × effort. Bugs and security first, then
orchestration robustness, then architecture that blocks other work, then
measured performance, then readability.

## Open

| # | Item | Why | Size |
|---|------|-----|------|
| 4 | Conformance Lab not run on the refactored integrations | The integrations consolidation changed probes (timeouts, `--version` success), MCP routes and the generated marketplace; only the Lab proves real harnesses still read them. | M (time) |
| 5 | Host-only gestures inside `ViewHit` (`GrabNavigatorEdge`, `DragContentScrollbar`, `ToggleSection`, `ResizeSection`) | Skipped twice as not contained: they ride `ExtensionHit`, shared by the management modal. Extension contract carries variants no extension handles. | M |
| 8 | CLI startup cost not measured outside the per-command budget tests | `hyperfine` is not installed (QUESTIONS #4); the budget tests already fail a command that regresses past its class. | S |
| 10 | Duplicate transitive crates (`base64`, `bitflags`, `hashbrown`, `signal-hook`, `syn`, `thiserror`, `windows-sys`) | All transitive; resolvable only by upstream bumps. Track, don't act. | — |

## Done

See `REFACTOR_LOG.md`.
