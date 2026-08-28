## ADDED Requirements

### Requirement: Cover the full portable-hook semantic surface end-to-end

The Lab SHALL exercise the full portable-hook semantic surface against real
harnesses wherever the harness's capability profile preserves it: every
canonical event (`PreToolUse`, `PostToolUse`, `Stop`), every claimed effect
(`ask`, `transform` where supported), explicit `native:<tool>` matchers, and
runtime failure paths (timeout, non-executable handler, malformed handler
output) with their fail-open/fail-closed consequences. Each exercised path
SHALL be an asserted check or a registered adaptive result per the
adaptive-result registry.

#### Scenario: PostToolUse marker reaches the conversation

- **WHEN** a `PostToolUse` hook is attached and a real tool executes
- **THEN** the hook's marker appears in the conversation evidence after the
  tool result

#### Scenario: Stop hook observed at session end

- **WHEN** the harness supports a `Stop` event and the session ends
- **THEN** the hook's marker is observed; a harness without the semantic
  event records a registered adaptive result instead

#### Scenario: Transform rewrites pre-tool input on the bridge

- **WHEN** a `transform` hook is attached on OpenCode and the real tool fires
- **THEN** the provider-serving request carries the rewritten input, proving
  the bridge applied it

#### Scenario: Fail-closed deny hook that cannot run still denies

- **WHEN** a declared `deny` hook's handler fails to run (timeout, missing
  executable) and the tool fires
- **THEN** the tool never executes and the verdict records the fail-closed
  decision and the underlying failure

### Requirement: Prove deep MCP execution where delivered

The Lab SHALL assert end-to-end MCP tool execution — registration, a real
tool call, and the tool's result reaching the conversation — for every
harness whose delivery supports it. Where the vendor channel or the current
delivery prevents proof, the finding SHALL be recorded as an honest adaptive
or unsupported result with its evidence and a tracked resolution.

#### Scenario: MCP round-trip asserted where supported

- **WHEN** a harness exposes the MCP server's tool during a turn
- **THEN** the tool's proof marker (produced by the real fixture binary)
  reaches the conversation evidence

#### Scenario: Channel-gap recorded, never fabricated

- **WHEN** the harness channel cannot expose the MCP tool in a turn
- **THEN** the cell records the adaptive result with the observed behavior
  and the tracked resolution, and the check escalates automatically once the
  channel exposes the tool

### Requirement: Cover non-interactive CLI mode per harness

The Lab SHALL exercise each harness's non-interactive invocation surface
with the attached fixture, asserting capability availability and hook
behavior in that mode where the harness supports it.

#### Scenario: One-shot CLI turn renders the deterministic marker

- **WHEN** a one-shot CLI invocation completes against the synthetic provider
- **THEN** the deterministic marker is present in the CLI's output and the
  provider request shows the attachments

### Requirement: Exercise project-context projection against real harnesses

The Lab SHALL run `uze context reconcile` in a disposable project directory
and assert that each real harness picks up the projected context — the
`AGENTS.md` baseline and the harness's own bridge — when a turn runs from
that project.

#### Scenario: Real harness reads the projected context

- **WHEN** a turn runs from a reconciled disposable project
- **THEN** the provider-serving request carries the projected context
  markers for that harness's bridge