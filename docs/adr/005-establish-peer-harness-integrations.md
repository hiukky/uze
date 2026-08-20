# Establish peer harness integrations around a harness-agnostic UZE core

Status: Accepted

## Context

The initial Rust inspector encoded Claude Code, Codex, Cursor, and OpenCode
as a core enum and support matrix. It also let a generic bundle module know a
Claude plugin path. Although it did not implement a Claude-to-Codex converter,
continuing that shape would make the first two harnesses architectural roles
instead of validation peers.

UZE's durable concern is an effective agent environment owned by the user.
Claude Code is a useful reference ecosystem and its plugins are useful foreign
fixtures, but neither role makes Claude a canonical representation. Codex is
not a conversion destination. A boundary is needed before the first real
Agent Skill validation path is added.

## Decision

UZE Core will operate on portable capabilities, an effective environment, and
integration-supplied `HarnessCapabilities`. It will not contain named harness
support rules or source/destination semantics. A capability router will return
compatibility and exposure results from those generic inputs.

Claude Code and Codex are the first peer harness integrations. Each supplies
its own capability description and consumes the same effective environment.
Adding a harness that uses existing capability kinds SHALL primarily require a
new integration implementation and its tests, rather than changes to UZE
domain rules.

Foreign representations are handled by specialized importers. A
`ClaudePluginImporter` may recognize `.claude-plugin/plugin.json` and return
core capabilities with foreign provenance. That importer is distinct from
`ClaudeIntegration`, and does not establish Claude as source or Codex as
destination.

Representation/provenance, compatibility route, and exposure/verification are
separate facts. A standard Agent Skill representation is not evidence that it
has been exposed in a particular harness. ACP remains an optional Client ↔
Agent runtime primitive under ADR-003, rather than an integration requirement.

The UZE Store is authoritative only for packages installed by UZE. Project
resources remain project-owned. `UzeEngine` composes both sources into one
effective environment before routing it to peer integrations. Runtime sources
remain empty in this increment.

Normal harness invocation (`claude`, then `codex`, in the real project
directory) is the target DX. A UZE launcher or per-session flag is not the
product architecture. `--plugin-dir` is retained only as a Claude conformance
exposure probe. Filesystem projection is an explicit compatibility fallback:
it may create a minimal UZE-managed artifact in the real caller workspace,
preserves that workspace as the process CWD, and must clean up its own artifact.
It never creates a shadow copy of the project.

Successful exposure is distinct from representation and compatibility. Real
probes report `VERIFIED`, `NOT_EXPOSED`, `UNVERIFIED`, `FAILED`, or
`BLOCKED_BY_ENVIRONMENT`; quota, authentication, missing executables, service
failures, and timeouts are never capability incompatibility.

Alternatives rejected: retaining the named-harness core matrix; treating
Claude import as a canonical source pipeline; and requiring ACP or filesystem
projection for every integration.

## Consequences

Easier: core unit and router tests use fake integrations without installed
harnesses; integrations can evolve independently; imports can be added for
foreign formats without contaminating runtime integrations; and removal of an
integration leaves UZE Core intact.

Harder: each integration must publish evidence-backed capability descriptions
and conformance tests before claiming verified exposure. Transparent integration
for Claude, Codex, and OpenCode is not proven in this increment; a managed
filesystem fallback is not equivalent to it. The increment does not add Cursor,
profiles, memory, marketplaces, or cloud state.

## Implementation Plan

- **Affected paths:** replace core harness rules in `src/capability.rs` and
  `src/project.rs`; add core router and integration-contract modules; add peer
  Claude/Codex integration modules; move Claude plugin recognition from the
  generic bundle boundary; update report, tests, LikeC4, and OpenSpec tasks.
- **Patterns to follow:** core accepts generic capability descriptions; foreign
  importer and runtime integration are distinct modules; UzeHome is resolved in
  the CLI composition root; the engine combines project and Store sources;
  tests use fake capabilities and integrations; real harness probes are opt-in.
- **Patterns to avoid:** named harness `match` branches in UZE domain routing,
  source/destination terminology, automatic vendor-directory scanning or
  projection, and ACP use without a concrete Client ↔ Agent boundary.

### Verification

- [ ] Core source contains no named Claude, Codex, Cursor, or OpenCode routing
      rules.
- [ ] Claude and Codex peer integrations route one physical stored Agent Skill
      through the same composed environment.
- [ ] Router and contract tests pass without real harness executables.
- [ ] Removing the Claude integration does not break compilation of UZE Core.
- [ ] Adding a fake Cursor integration requires no core modification.
- [ ] Rust, OpenSpec, and LikeC4 validation pass.

Source change: openspec/changes/validate-universal-agent-environment/

## More Information

2026-08-20: Implemented with a harness-agnostic library core, external
Claude/Codex integration modules, an explicit Claude plugin importer, and
unit/contract tests. `cargo test --lib` verifies the core without integration
modules; real-harness conformance remains intentionally pending.

2026-08-20: The opt-in Agent Skill conformance probe found Codex CLI 0.148.0
`VERIFIED` for `.agents/skills`. A later Claude Code 2.1.237 probe returned an
API session-limit response, so Claude remains `UNVERIFIED`; API failures are
not capability evidence. OpenCode 1.18.18 is an additional peer integration:
its documentation declares `.agents/skills` discovery, while its local
real-harness probe remains `UNVERIFIED` until a provider is configured.

2026-08-20: The next slice added `UzeHome`, a minimal store for external Agent
Plugin packages, and `ExposurePlan`. `STANDARD` representation is explicitly
separate from an exposure mechanism: Claude selects its per-session
`--plugin-dir` runtime bridge, while Codex selects an explicit session-scoped
filesystem fallback under `$UZE_HOME/runtime`. The Codex end-to-end flow
package → store → engine → environment → integration → harness is verified;
the equivalent Claude flow returned HTTP 429 and remains `UNVERIFIED`.

2026-08-20: The composition correction made the Store authoritative only for
UZE-installed packages, connected `UzeHome::from_env()` to `uze add` and
`uze inspect`, and made the Engine merge project and registered package
resources. A same-store peer probe records one PackageId/path/resource for
Claude and Codex. Codex/OpenCode filesystem exposure now uses a managed
symlink in the real project CWD and a `$UZE_HOME/runtime/.../managed-exposure.json`
record, with explicit cleanup; it does not create a shadow workspace.
`--plugin-dir` remains a Claude probe only. Local Claude CLI help confirms
official plugin lifecycle commands and a global skills directory, so a
one-time transparent connector is possible but unproven and deliberately not
implemented as a workaround. Conformance now classifies environmental blocks
separately from compatibility.

2026-08-20: The opt-in same-store probe verified Codex 0.148.0 against the
real caller project CWD: its managed `.agents/skills` symlink resolved the one
stored Agent Plugin Skill and produced the behavioral proof token. Claude Code
returned its explicit HTTP 429 session-limit response and was recorded as
`BLOCKED_BY_ENVIRONMENT`. OpenCode's configured probe exceeded the five-second
diagnostic timeout with no provider result and was likewise
`BLOCKED_BY_ENVIRONMENT`. Neither result changes the integrations' compatibility
routes or proves transparent normal invocation.

2026-08-20: Re-running the opt-in battery with explicit economical models
verified the UZE Agent Skill exposure probe for all three peer integrations:
Claude Code using `haiku`, Codex using `gpt-5.6-luna`, and OpenCode using
`opencode/deepseek-v4-flash-free`. The native discovery probes for Codex and
OpenCode also passed. This proves the current package → Store → Engine →
Environment → Integration → harness path and its proof token; it does not
prove one-time transparent integration for a normal `claude`, `codex`, or
`opencode` invocation.
