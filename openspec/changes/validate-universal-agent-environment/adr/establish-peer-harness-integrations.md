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

Alternatives rejected: retaining the named-harness core matrix; treating
Claude import as a canonical source pipeline; and requiring ACP or filesystem
projection for every integration.

## Consequences

Easier: core unit and router tests use fake integrations without installed
harnesses; integrations can evolve independently; imports can be added for
foreign formats without contaminating runtime integrations; and removal of an
integration leaves UZE Core intact.

Harder: each integration must publish evidence-backed capability descriptions
and conformance tests before claiming verified exposure. The first increment
does not prove real Claude Code or Codex exposure, does not add Cursor, and
does not introduce filesystem projection, profiles, memory, marketplaces, or
cloud state.

## Implementation Plan

- **Affected paths:** replace core harness rules in `src/capability.rs` and
  `src/project.rs`; add core router and integration-contract modules; add peer
  Claude/Codex integration modules; move Claude plugin recognition from the
  generic bundle boundary; update report, tests, LikeC4, and OpenSpec tasks.
- **Patterns to follow:** core accepts generic capability descriptions; foreign
  importer and runtime integration are distinct modules; tests use fake
  capabilities and integrations; CLI remains read-only.
- **Patterns to avoid:** named harness `match` branches in UZE domain routing,
  source/destination terminology, automatic vendor-directory scanning or
  projection, and ACP use without a concrete Client ↔ Agent boundary.

### Verification

- [ ] Core source contains no named Claude, Codex, Cursor, or OpenCode routing
      rules.
- [ ] Claude and Codex peer integrations route one standard Agent Skill
      through the same core model.
- [ ] Router and contract tests pass without real harness executables.
- [ ] Removing the Claude integration does not break compilation of UZE Core.
- [ ] Adding a fake Cursor integration requires no core modification.
- [ ] Rust, OpenSpec, and LikeC4 validation pass.
