## Why

UZE has deterministic lifecycle tests and opt-in vendor probes, but neither
starts from a machine with an empty harness home nor separates local wiring
evidence from vendor/model conformance. A reproducible local lab is needed to
exercise real harness CLIs after UZE has attached one multi-capability plugin.
The original Claude Code, Codex and OpenCode tracer bullets do not define the
architectural limit; the selected L2 set must follow current evidence.

## What changes

- Establish four explicit evidence tiers: L0 unit, L1 product contract, L2
  isolated real-harness E2E, and opt-in L3 vendor conformance. L2 may use a
  local model or a separately authenticated, test-only gateway route; reports
  identify which route produced the evidence.
- First record an ecosystem/provider spike across the relevant harnesses and
  select only those with an honest headless and protocol path.
- Add test-only Docker and Rust process-runner tooling outside the UZE product
  crate after that gate. It creates disposable HOME, UZE_HOME, and project
  directories, then invokes real harness CLIs.
- Use a pinned LiteLLM gateway for the initial routed L2 reference. The
  gateway is test-only, receives provider credentials alone, and may route to
  a free-tier provider such as Groq. Direct/local routes remain possible, but
  are not a prerequisite for this phase.
- Reuse one installed Agent Plugin fixture containing both Skill and stdio MCP
  resources. The lab records package/store/exposure evidence separately from
  discovery and behavioral evidence.

## Non-goals

No UZE product runtime dependency on Docker, LiteLLM, or a test
runner; no mock harnesses; no remote registry, model downloader, benchmark,
TUI work, capability implementation, or CI pipeline configuration.
