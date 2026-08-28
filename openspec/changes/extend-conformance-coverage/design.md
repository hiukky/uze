## Design

### Context

The hook phases today script one intercepted tool per scenario (`TOOL_NAME`/
`TOOL_ARGS` + session-final text). Providers already capture every request
into `struct.json`, which is how transform rewrites and PostToolUse markers
can be proven without new interception machinery. The deterministic suite
(after `add-portable-hooks`) owns the unit-level ABI semantics; this change
only grows the E2E surface over real harnesses. See proposal.md — Why.

### Goals / Non-Goals

- Goals: E2E evidence for every semantics the capability profiles claim,
  plus honest records for what a channel cannot express; deep MCP proof per
  harness; CLI-mode and project-context phases per harness.
- Non-Goals: changing the canonical hook model or ABI; force-fixing vendor
  channels (MCP gaps are tracked, and delivery fixes land in their own
  change); expanding beyond the four existing harnesses.

### Decisions

**D1 — Extend the existing `phase_hooks` table rather than new machinery.**
The scenario table gains kinds: `post_tool_use` (provider emits a tool
result; assertion = marker after the result in `struct.json`), `stop` (end
session; marker observed — or registered adaptive where the profile declares
no Stop), `ask` (antigravity only; provider scripts the approval gate),
`transform` (opencode only; provider captures the rewritten input and the
assertion compares it to the pre-rewrite value), `native-matcher` (a group
matched by `native:<tool>` only), `fail-closed-timeout` and `fail-closed-
inexec` (a deny hook whose handler dies; assertion = the tool never executes
and the verdict carries the failure). Every new check routes through the
`harden-conformance-gate` kinds; `Stop`/`ask` cells that cannot be proven
are registered adaptive results, never skipped.

**D2 — Deep MCP: investigate first in the sandbox, then assert.** Use the
sandbox (from `conformance-exploration-sandbox`) to probe the Claude
ToolSearch deferral for a direct-invocation form that reaches the tool; if
found, assert the round-trip like Antigravity's. The Codex MCP inventory gap
is a delivery question: probe config variants (feature flags, config paths)
in the sandbox; if a working variant exists, fold it into
`crates/uze-integrations/src/codex.rs` as a delivery fix and then assert;
otherwise keep the cell as a tracked adaptive gap with its evidence. OpenCode
keeps the existing auto-escalate pattern.

**D3 — CLI-mode as a parallel phase per harness.** New `describe("cli")`
groups using the same provider but a one-shot invocation (`claude -p`,
`codex exec`, `opencode run`), asserting the deterministic marker in stdout
and the attachments in the provider request. The settle-and-quiet contract
adapts to stdout (marker line, then quiet). Harnesses whose CLI mode cannot
reach the synthetic provider (probe in sandbox first) register that honestly.

**D4 — Project-context phase.** In-container disposable project: `uze
market add` + `uze plugin install @` (project scope), then `uze context
inspect`/`reconcile`, then a one-shot or short TUI turn from that cwd. The
assertion is the provider request carrying the project's context markers —
the same `struct.json` evidence channel, no new observability.

### Risks / Trade-offs

- [New kinds multiply flaky surface] → they run through the settled-absence
  contract and the gate registry of `harden-conformance-gate`; experiments
  (from `conformance-exploration-sandbox`) de-risk vendor behavior before
  canonicalization.
- [A `native:<tool>` matcher may name a vendor tool we cannot reliably
  script] → script via the provider's own tool emission; if the harness
  rewrites tool names, record that as an adaptive finding.
- [CLI-mode pages may block or need auth] → sandbox-probe each first; a
  blocked path is recorded honestly, not worked around in code.
- [Project-context phases lengthen each vertical] → one phase per harness,
  bounded waits, reused provider; acceptable on top of TUI phases.

### Migration Plan

Ordered by dependency: change 1 (gate) lands first so new checks register
cleanly; change 2 (sandbox) enables the MCP/CLI investigations; this change
canonicalizes their findings. Each new phase is built as an experiment
first, then promoted through the 3-clean-run rule. Rollback: removals are
independent scenario phases; no shared machinery is altered.

### Open Questions

None that change spec or approach: the ToolSearch and Codex-inventory
results are investigated inside this change's own tasks, and each outcome
has a defined spec-compliant path (assert / registered adaptive with tracked
resolution).