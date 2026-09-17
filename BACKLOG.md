# Backlog

Ordered by impact × risk × effort. Bugs and security first, then
orchestration robustness, then architecture that blocks other work, then
measured performance, then readability.

## Open

| # | Item | Why | Size |
|---|------|-----|------|
| 4 | Conformance Lab not run on the refactored integrations | The integrations consolidation changed probes (timeouts, `--version` success), MCP routes and the generated marketplace; only the Lab proves real harnesses still read them. | M (time) |
| 5 | Host-only gestures inside `ViewHit` (`GrabNavigatorEdge`, `DragContentScrollbar`, `ToggleSection`, `ResizeSection`) | Skipped twice as not contained: they ride `ExtensionHit`, shared by the management modal. Extension contract carries variants no extension handles. | M |
| 8 | No measurement harness for TUI frame cost or CLI startup | Budget tests exist per command; no frame benchmark. `hyperfine`/`perf` not installed here. | M |
| 9 | Evaluate `clippy::pedantic` | Apply only the lints that find real defects (casts, `must_use`), not style. | M |
| 10 | Duplicate transitive crates (`base64`, `bitflags`, `hashbrown`, `signal-hook`, `syn`, `thiserror`, `windows-sys`) | All transitive; resolvable only by upstream bumps. Track, don't act. | — |

## Done

See `REFACTOR_LOG.md`.
