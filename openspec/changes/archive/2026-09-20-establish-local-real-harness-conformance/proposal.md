## Why

UZE has deterministic lifecycle tests and opt-in vendor probes, but neither
starts from a machine with an empty harness home, and neither separates
wiring evidence from model behavior. A reproducible lab is needed to
exercise real harness CLIs after UZE has attached one multi-capability
plugin. The original Claude Code, Codex and OpenCode tracer bullets do not
define the architectural limit; the selected set must follow current
evidence.

## What changes

- Establish explicit evidence tiers: L0 unit, L1 product contract, L2
  isolated real-harness conformance, L3 acceptance through the real `uze`
  binary, and L4 opt-in model behavior that never gates CI.
- First record an ecosystem/provider spike across the relevant harnesses and
  select only those with an honest headless and protocol path.
- Add the lab's tooling outside the UZE product crates after that gate. It
  creates disposable HOME, UZE_HOME and project directories, then invokes
  real harness CLIs.
- Give each vertical a **synthetic provider** speaking that vendor's real
  wire protocol on an internal-only network, so a run needs no provider
  credential, no token spend and no Internet, and returns a deterministic
  response.
- Reuse one installed plugin fixture containing both Skill and stdio MCP
  resources. The lab records package/store/exposure evidence separately from
  discovery and behavioral evidence.

## Non-goals

No UZE product runtime dependency on Docker, a provider, or the lab runner;
no mock harnesses; no remote registry, model downloader, benchmark, TUI
work, or capability implementation.
