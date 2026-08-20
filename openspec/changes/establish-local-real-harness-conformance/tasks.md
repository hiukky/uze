## Research and spike

- [x] Research package, capability, provider, headless and container surfaces
      for Claude Code, Codex, OpenCode, Cursor, Windsurf, Gemini CLI, GitHub
      Copilot CLI, Cline and Roo Code; record classifications in
      `research-notes.md`.
- [x] Select OpenCode as the first L2 behavioral route; retain Codex as a
      Responses-protocol spike and Claude Code as an explicitly experimental
      local-gateway route.
- [x] Build a pinned, disposable harness image and prove installation/headless
      invocation with isolated HOME for Claude Code 2.1.237, Codex 0.148.0
      and OpenCode 1.18.19. The non-privileged tmpfs ownership contract is
      recorded in `tooling/conformance/README.md`.
- [ ] Prove each selected direct or gateway-backed local inference route and
      record protocol limitations independently from UZE compatibility.

## Tooling

- [ ] Add the standalone Rust conformance runner and its deterministic process
      contract tests.
- [ ] Add the minimal Docker Compose topology, read-only model mount contract,
      and security defaults.
- [ ] Reuse the Plugin-First fixture for dynamic Skill and MCP proof tokens.
- [ ] Add structured attachment/discovery/behavioral evidence and filters.

## Verification

- [ ] Run L2 Skill and MCP paths for every viable harness.
- [ ] Preserve L3 vendor probes and document L2/L3 boundaries.
- [ ] Run Rust, format, lint, OpenSpec, LikeC4, and diff gates.
