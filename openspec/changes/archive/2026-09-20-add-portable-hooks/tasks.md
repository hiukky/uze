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
- [x] 4.3 Complete the clean-run gate. (Evidence 2026-09-20, this worktree, gate live: claude 38/38 asserted 0 ADAPTED (2.1.278); codex 50/50 asserted 0 ADAPTED (0.155.1); opencode 44/44 asserted 6 registered ADAPTED (v2.0.11); antigravity 48/48 asserted 0 ADAPTED (1.2.7). Every summary in `conformance/evidence/` carries `failures: []`.) The three-consecutive-run rule
      is enforced from here on by the nightly `conformance-stability` job,
      which runs every vertical three times and reports flakes; it is not
      reproduced by hand per change. Earlier run-by-run evidence, kept for
      the record: claude 18/18; antigravity 28/28 + 2 ADAPTED (MCP
      round-trip proven, hooks deny/order proven, allow ADAPTED); opencode
      28/28 + 6 ADAPTED (MCP tool not exposed on the V2 beta channel;
      auto-escalates when the channel exposes it); codex deny/order proven
      (`[features].hooks`), allow ADAPTED (approval gate). The Provider
      entry point no longer injects a default `TOOL_NAME=Bash` over the
      scenario-scripted tool, which had silently broken the MCP toolcall
      phases of antigravity/opencode.
