## 1. Maintenance domain and safety policy

- [x] 1.1 Add typed maintenance plan/report/outcome models in `uze-application`, with explicit repaired, available-update, unavailable, and needs-human-action outcomes.
- [x] 1.2 Define the IntegrationPort repair contract so integrations can prove a repair is deterministic and receipt-owned without naming vendors in Core/Application.
- [x] 1.3 Implement planner/executor separation under the existing mutation lock and live re-inspection after every repair.
- [x] 1.4 Confirm [ADR-037](../../../docs/adr/037-adopt-bounded-environment-maintenance.md) remains the permanent decision record.

## 2. Safe convergence

- [x] 2.1 Republish every applicable UZE-owned derived marketplace/view during maintenance, idempotently and without network access.
- [ ] 2.2 Recreate receipt-proven missing UZE-owned attachments for detected integrations.
- [x] 2.3 Preserve Drifted, Conflict, Blocked, unreadable-ledger, and foreign-root cases; return typed human-action outcomes instead of reattaching blindly.
- [x] 2.4 Keep marketplace update discovery and plugin-byte installation out of the maintenance execution path.

## 3. CLI and TUI integration

- [x] 3.1 Invoke bounded maintenance from the existing CLI doctor command and expose repaired versus unresolved outcomes in text and JSON; add no new synchronization command.
- [x] 3.2 Invoke the same maintenance use case from the TUI worker at startup and manual refresh without blocking its rendering/event loop; coalesce overlapping refresh requests into one run.
- [ ] 3.3 Keep the existing header working-status presentation (`Refreshing environment…`) for an in-flight maintenance run, then add transient notifications for repairs and update availability; show only unresolved human-action outcomes as problems.
- [x] 3.5 Treat a receipt-less, externally present native package as a non-failing no-op without taking ownership or adding duplicate fallbacks.
- [x] 3.4 Classify every changed CLI leaf in `src/command_performance.rs`, retain the doctor budget on healthy warm paths, and add a TUI responsiveness budget/test for the asynchronous path.

## 4. Verification

- [ ] 4.1 Add deterministic tests for rebuilding missing symlinks, generated catalogues, and integration-owned derived artifacts.
- [ ] 4.2 Add negative tests proving divergent roots, foreign entries, conflicts, corrupt receipts, executable-capability changes, and network-unavailable sources are never auto-overwritten or auto-acquired.
- [x] 4.3 Add CLI and TUI worker tests proving both presenters use the same maintenance report, the TUI renders while maintenance is in flight, repeated refreshes are coalesced, and repaired drift is not displayed as an error.
- [ ] 4.4 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --no-fail-fast`, and `openspec validate --all --strict`.
