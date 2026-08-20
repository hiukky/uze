## Research and spike

- [x] Research package, capability, provider, headless and container surfaces
      for Claude Code, Codex, OpenCode, Cursor, Windsurf, Gemini CLI, GitHub
      Copilot CLI, Cline and Roo Code; record classifications in
      `research-notes.md`.
- [x] Select OpenCode as the first routed L2 behavioral route; retain Codex as
      a Responses-protocol spike and Claude Code as an explicitly experimental
      Anthropic-gateway route.
- [x] Build a pinned, disposable harness image and prove installation/headless
      invocation with isolated HOME for Claude Code 2.1.237, Codex 0.148.0
      and OpenCode 1.18.19. The non-privileged tmpfs ownership contract is
      recorded in `tooling/conformance/README.md`.
- [ ] Prove each selected gateway-backed inference route and
      record protocol limitations independently from UZE compatibility.

## Tooling

- [x] Add the standalone Rust conformance runner and deterministic process
      contract tests.
- [x] Add the minimal Docker Compose gateway topology, provider-secret
      contract, and security defaults.
- [x] Reuse the Plugin-First fixture through per-run materialization with
      distinct dynamic Skill/MCP proof channels; canonical Store input is
      immutable.
- [x] Define structured attachment/discovery/behavior evidence states in the
      test-only runner. Harness-specific output adapters and CLI filters remain
      pending.

## Verification

- [ ] Run L2 Skill and MCP paths for every viable harness.
- [ ] Preserve L3 vendor probes and document L2/L3 boundaries.
- [ ] Run Rust, format, lint, OpenSpec, LikeC4, and diff gates.
