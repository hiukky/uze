## Why

UZE has deterministic lifecycle tests and opt-in vendor probes, but neither
starts from a machine with an empty harness home nor separates local wiring
evidence from vendor/model conformance. A reproducible local lab is needed to
exercise real harness CLIs after UZE has attached one multi-capability plugin.
The original Claude Code, Codex and OpenCode tracer bullets do not define the
architectural limit; the selected L2 set must follow current evidence.

## What changes

- Establish four explicit evidence tiers: L0 unit, L1 product contract, L2
  local real-harness E2E, and opt-in L3 vendor conformance.
- First record an ecosystem/provider spike across the relevant harnesses and
  select only those with an honest headless and local-model path.
- Add test-only Docker and Rust process-runner tooling outside the UZE product
  crate after that gate. It creates disposable HOME, UZE_HOME, and project
  directories, then invokes real harness CLIs.
- Pin a llama.cpp server and model contract as the L2 inference reference.
  Direct protocol routes are preferred; a gateway is only a test-only fallback
  when a pinned harness/server combination cannot interoperate directly.
- Reuse one installed Agent Plugin fixture containing both Skill and stdio MCP
  resources. The lab records package/store/exposure evidence separately from
  discovery and behavioral evidence.

## Non-goals

No UZE product runtime dependency on Docker, llama.cpp, LiteLLM, or a test
runner; no mock harnesses; no remote registry, model downloader, benchmark,
TUI work, capability implementation, or CI pipeline configuration.
