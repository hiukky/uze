## Why

The Lab's scenario surface is narrower than the delivered semantics: hooks
are proven only for `deny`/`allow`/`order` on `PreToolUse`, MCP deep
execution is proven only on Antigravity, non-interactive CLI mode is
untouched, and project-context projection (`uze context reconcile`) has never
been exercised against a real harness. The compatibility claims that matter
to a multi-harness user deserve end-to-end evidence for happy and degraded
paths alike.

## What Changes

- Cover the **full portable-hook semantic surface** end-to-end where each
  harness preserves it: `PostToolUse`, `Stop` (claude/codex/antigravity),
  `ask` (antigravity), `transform` (opencode bridge), explicit `native:<tool>`
  matchers, and fail-closed/fail-open runtime failures (timeout,
  non-executable handler, malformed handler output) — each asserted or
  registered as an honest adaptive result per the harness capability
  profiles.
- Prove **deep MCP execution** (registration → tool call → tool result
  reaches the conversation) for every harness whose delivery supports it;
  investigate the Claude ToolSearch deferral and the Codex inventory gap in
  the sandbox and record tracked, actionable gaps where delivery blocks
  proof.
- Exercise **non-interactive CLI mode** per harness (`claude -p`,
  `codex exec`, `opencode run`) with the attached fixture.
- Exercise **project-context projection** end-to-end: `uze context
  reconcile` in a disposable project, then assert the real harness picks up
  the projected context in its provider request.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `local-real-harness-conformance`: new requirements — full hook semantic
  surface coverage, deep MCP execution evidence, non-interactive CLI-mode
  coverage, and project-context projection against real harnesses.

## Impact

- `conformance/harnesses/*/scenarios.py` — new/extension scenario kinds and
  phases (hooks surface, cli mode, project context).
- `conformance/harnesses/*/provider.py` — scripted responses for the new
  kinds (transform rewrite capture, PostToolUse marker, stop).
- `conformance/evidence/expected.json` — registrations for `Stop`/`ask`
  adaptive results (depends on `harden-conformance-gate`).
- Sandbox usage for the MCP investigations (depends on
  `conformance-exploration-sandbox`).
- `conformance/README.md` — updated evidence matrix.
- Delivery fix for the Codex MCP inventory gap (`crates/uze-integrations/
  src/codex.rs`) if the investigation finds a config variant that works.