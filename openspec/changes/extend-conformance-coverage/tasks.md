## 1. Hook semantic surface

- [ ] 1.1 Extend `phase_hooks` with kinds: `post_tool_use`, `stop`, `ask`,
      `transform`, `native-matcher`, `fail-closed-timeout`,
      `fail-closed-inexec`; script each via the provider's tool emission and
      capture the evidence in `struct.json`. **Started**: the first slice —
      the **fail-closed** path — passed live 3/3 on claude
      (`experiments/claude/fail-closed.py` + the `hook-fail-plugin` fixture:
      a declared `deny` hook whose handler script is missing, asserting the
      intercepted tool never executes and the failure reason is recorded).
      Promotion into the canonical table follows the 3-consecutive-clean-run
      gate; the remaining kinds route through the same experiment-first
      path (sandbox probes de-risk each vendor's surface before
      canonicalization).
- [ ] 1.2 Register adaptive cells in `expected.json` (per
      `harden-conformance-gate`): `Stop`/`ask` where the profile cannot
      preserve them. **Done for the opencode channel recovery**: the
      `mcp-tool-model-exposed` entry was removed (escalated — the V2 beta
      channel now exposes the MCP tool in the model request; the assert
      passes 28/28) and `hooks-allow-tool-executed` was registered with its
      recorded reason.
- [ ] 1.3 Run a claude vertical with the new kinds and verify the
      settled-absence contract holds for the new failure-path checks.
      (Covered for the fail-closed slice once promoted.)
- [x] 1.4 **Codex 0.150.1 investigation — RESOLVED (22/22 asserted, 0
      ADAPTED, 3 consecutive clean-run trial)**: six distinct causes
      isolated with evidence:
      (a) `ansi_strip` — state-machine replacement of the regex strip
      (OSC 8/ST + per-keystroke `\x1b[0 q`; the corrupted "Working"
      spinner), 7 unit tests;
      (b) startup hooks-review screen — official automation flag
      `codex --dangerously-bypass-hook-trust`;
      (c) Responses API over a **real WebSocket** — minimal RFC 6455
      server (`shared/websocket.py`, 7 tests) + per-message evidence
      recording (WS turn bodies invisible to HTTP-only reads);
      (d) WS frames carry **one JSON event each** (`ResponsesStreamEvent`
      deserializes per frame; SSE framing is HTTP-only) — verified against
      openai/codex's `responses_websocket.rs`/`sse/responses.rs`;
      (e) the shell tool surface changed: `Bash`{command} died; 0.150.1
      dispatches **`exec_command`{cmd}** ("unsupported call: Bash" vs
      "missing field `cmd`") — scenarios + fixture matchers updated
      (`native:exec_command`);
      (f) bubblewrap user namespaces blocked by the Lab's default seccomp
      ("No permissions to create new namespace") — `docker_base` runs
      harness containers with `seccomp=unconfined` (disposable/rootless
      topology, documented). With the sandbox fixed, the **allow path
      executes the tool for real** ("Ran echo plain output └ plain
      output") — the `hooks-allow-approval-gate` ADAPTED record was
      removed and the allow is now asserted evidence.

## 2. Deep MCP execution

- [ ] 2.1 Sandbox-probe Claude ToolSearch for a direct tool invocation form;
      if found, add the round-trip assertion (registration → call → proof
      marker in conversation).
- [ ] 2.2 Sandbox-probe the Codex MCP inventory gap; if a config variant
      works, land the delivery fix in `crates/uze-integrations/src/codex.rs`
      with deterministic coverage, then assert the round-trip; otherwise
      record the tracked adaptive cell with its evidence.
- [ ] 2.3 Keep the opencode MCP cell on the auto-escalate pattern and verify
      it escalates when the channel exposes the tool.

## 3. Non-interactive CLI mode

- [ ] 3.1 Sandbox-probe `claude -p`, `codex exec`, `opencode run` against the
      synthetic provider; record reachable surfaces.
- [ ] 3.2 Add `describe("cli")` phases per harness: one-shot invocation,
      deterministic marker in stdout, attachments observed in the provider
      request; adapt settle-and-quiet to stdout.
- [ ] 3.3 Verify a full vertical per harness (TUI + cli) passes 3 consecutive
      runs.

## 4. Project-context projection

- [ ] 4.1 Add the in-container disposable project phase: project-scoped
      install, `uze context inspect`/`reconcile`, turn from the project cwd.
- [ ] 4.2 Assert each harness's provider request carries the projected
      context markers (AGENTS.md baseline + the harness's own bridge).
- [ ] 4.3 Verify the phase per harness and record evidence.

## 5. Gate, acceptance, and docs

- [ ] 5.1 All new checks pass the adaptive-result registry rules
      (`harden-conformance-gate`).
- [x] 5.2 Complete the 3-consecutive-clean-run gate for all four harnesses
      with the extended surface, updating the run-by-run evidence records.
      (Evidence 2026-08-27/28: claude 18/18 (2.1.247); antigravity 28/28 +
      2 ADAPTED (1.1.22); codex 22/22 asserted, 0 ADAPTED (0.150.1), 3×
      clean; opencode 28/28 + 6 ADAPTED (v0.0.0-beta-18387). The nightly
      `conformance-stability` job enforces the gate going forward.)
- [ ] 5.3 Update `conformance/README.md` evidence matrix and the
      `docs/capabilities/portable-hooks.md` per-harness delivery table where
      new evidence changes a claim.