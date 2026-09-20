## Research and spike

- [x] Research package, capability, provider, headless and container surfaces
      for Claude Code, Codex, OpenCode, Cursor, Windsurf, Gemini CLI, GitHub
      Copilot CLI, Cline and Roo Code; record classifications in
      `research-notes.md`.
- [x] Select the first behavioral routes; retain Codex as a Responses-protocol
      spike and Claude Code as an explicitly experimental gateway route.
- [x] Build a pinned, disposable harness image and prove installation and
      headless invocation with an isolated HOME.
- [~] Prove each gateway-backed inference route. Abandoned: a real provider
      made every run non-deterministic, credential-bearing and cost-bearing.
      Replaced by a per-vendor synthetic provider speaking the real wire
      protocol on an `--internal` network — zero Internet, zero tokens, zero
      credentials. Real-model behavior became the opt-in L4 tier.

## Tooling

- [x] Add the standalone conformance runner and its deterministic process
      contract.
- [~] Add the Docker Compose gateway topology, provider-secret contract, and
      security defaults. Superseded with the routed provider: there is no
      gateway and no secret, so the contract has nothing to protect. The
      isolation defaults survive in the `--internal` network topology.
- [x] Reuse the fixture through per-run materialization with distinct dynamic
      Skill/MCP proof channels; canonical Store input is immutable.
- [x] Define structured attachment/discovery/behavior evidence states in the
      runner, with per-harness output adapters.

## Verification

- [x] Run the L2 Skill and MCP paths for every viable harness — four
      verticals (`claude`, `codex`, `opencode`, `antigravity`).
- [x] Document the tier boundaries. The original opt-in L3 vendor probes were
      retired rather than preserved: the Lab supersedes them, and their host
      machine dependence is exactly what it removes. `tests/README.md` owns
      the resulting table (L0/L1/L2/L3/L3.5/L4).
- [x] Run the Rust, format, lint and OpenSpec gates.
