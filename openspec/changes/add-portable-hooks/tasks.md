## 1. Canonical hook model

- [x] 1.1 Add the vendor-neutral manifest parser, validation, Hook IR, tool aliases, ABI types, and semantic compatibility evidence in `uze-core`.
- [x] 1.2 Discover package and project hook manifests as stable Hook resources without changing Store bytes.
- [x] 1.3 Add focused parser, validation, alias, order, timeout, decisions, transform, and unsupported-capability tests.

## 2. Lifecycle and runtime dispatch

- [x] 2.1 Add a narrow command dispatcher that normalizes vendor JSON, invokes command handlers sequentially, enforces bounded output/timeout behavior, and maps decisions back to a target.
- [x] 2.2 Integrate Hook resources with exposure plans, attachments, receipts, inspect-before-detach, status, doctor, reconcile, and TUI diagnostics.
- [x] 2.3 Cover idempotence, user-config merge, drift, removal, command/path escaping, and platform-specific command handling.

## 3. Harness projections

- [x] 3.1 Emit and lifecycle-test Claude Code native `hooks/hooks.json` projection.
- [x] 3.2 Emit and lifecycle-test Codex native `hooks.json` projection.
- [x] 3.3 Emit and lifecycle-test Antigravity named-hook projection including aliases and decisions.
- [x] 3.4 Generate and lifecycle-test the owned OpenCode bridge/config entry, including matcher, normalized payload, transform, deny, reason, sequence, error, timeout, and cleanup.

## 4. Evidence and documentation

- [x] 4.1 Add canonical fixtures, portable example plugin, schema/ABI/migration documentation, and the README compatibility matrix.
- [x] 4.2 Add TUI-first conformance scenarios for every native/bridge claim; use CLI only where a harness lacks a slash-command surface. Scenarios are grouped `describe`/`test`-style and waits abort immediately on a dead harness process.
- [ ] 4.3 Complete the 3x clean-run gate. Recorded real executions so far (run-by-run): claude 18/18 PASS; opencode 27/28 + 2 ADAPTED (1 pre-existing base-phase MCP FAIL); antigravity 27/28 + 2 ADAPTED (1 pre-existing MCP FAIL; hooks deny/order proven, allow recorded ADAPTED); codex deny/order proven (feature flag `[features].hooks`), allow recorded ADAPTED (approval gate), turn marker pending. The gate also requires formatting, clippy, deterministic suite, and strict OpenSpec validation — all green in the deterministic half so far.