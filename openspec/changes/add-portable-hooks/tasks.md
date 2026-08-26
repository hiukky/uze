## 1. Canonical hook model

- [x] 1.1 Add the vendor-neutral manifest parser, validation, Hook IR, tool aliases, ABI types, and semantic compatibility evidence in `uze-core`.
- [x] 1.2 Discover package and project hook manifests as stable Hook resources without changing Store bytes.
- [ ] 1.3 Add focused parser, validation, alias, order, timeout, decisions, transform, and unsupported-capability tests.

## 2. Lifecycle and runtime dispatch

- [ ] 2.1 Add a narrow command dispatcher that normalizes vendor JSON, invokes command handlers sequentially, enforces bounded output/timeout behavior, and maps decisions back to a target.
- [ ] 2.2 Integrate Hook resources with exposure plans, attachments, receipts, inspect-before-detach, status, doctor, reconcile, and TUI diagnostics.
- [ ] 2.3 Cover idempotence, user-config merge, drift, removal, command/path escaping, and platform-specific command handling.

## 3. Harness projections

- [ ] 3.1 Emit and lifecycle-test Claude Code native `hooks/hooks.json` projection.
- [ ] 3.2 Emit and lifecycle-test Codex native `hooks.json` projection.
- [ ] 3.3 Emit and lifecycle-test Antigravity named-hook projection including aliases and decisions.
- [ ] 3.4 Generate and lifecycle-test the owned OpenCode bridge/config entry, including matcher, normalized payload, transform, deny, reason, sequence, error, timeout, and cleanup.

## 4. Evidence and documentation

- [ ] 4.1 Add canonical fixtures, portable example plugin, schema/ABI/migration documentation, and the README compatibility matrix.
- [ ] 4.2 Add TUI-first conformance scenarios for every native/bridge claim; use CLI only where a harness lacks a slash-command surface.
- [ ] 4.3 Run formatting, clippy, deterministic suite, strict OpenSpec validation, and all four conformance labs; record any verified limitations.
