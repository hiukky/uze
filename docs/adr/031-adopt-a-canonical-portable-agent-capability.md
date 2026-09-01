# Adopt a canonical portable Agent capability

Status: Accepted

Refines: [ADR-013 (Native Projection Principle)](013-adopt-native-projection-principle.md),
[ADR-013 (Native Projection)](013-adopt-native-projection-principle.md),
and [ADR-030 (Skill + Invocation Policy)](030-skill-plus-invocation-policy.md).

## Context

`CapabilityKind::Agent` is already recognized during package acquisition, but
the Engine does not compose package `agents/` resources and no integration
routes them. As a result, package authors cannot distribute reusable agent
profiles through UZE even though Claude Code, OpenCode, and Antigravity CLI
now document Markdown agent definitions and their interactive agent managers.
Codex CLI documents standalone TOML custom-agent files, which are a native
consumer format but not a portable package component.

Treating one vendor's agent directory as canonical would leak harness schema
and silently change permission, model, or delegation semantics on other
harnesses. Leaving agents import-only would keep the support matrix false and
make UZE's portable package model incomplete.

## Decision

UZE will adopt `agents/<name>.md` as the canonical portable Agent capability.
Each file is stored byte-for-byte, composes as an independent
`CapabilityKind::Agent` resource, and is delivered at capability level.

- Claude Code, OpenCode, and Antigravity CLI receive their documented native
  Markdown-agent projection once real-harness conformance proves it.
- Codex receives a generated native TOML projection at
  `~/.codex/agents/<name>.toml`. It supplies the documented required
  `name`, `description`, and `developer_instructions` fields from portable
  Markdown frontmatter/body. The route is Native only after real-harness
  conformance proves that Codex loads and selects the generated agent.
- UZE projects only a verified portable frontmatter subset. Vendor-only model,
  permission, hook, MCP, memory, and orchestration fields remain unclaimed;
  an integration either preserves a field explicitly or reports the route as
  adapted/degraded.
- Projection artifacts are derived, live outside the Store, use qualified
  labels where global roots require collision-safe names, and have typed
  receipts subject to inspect-before-detach.

The alternative of a new UZE JSON manifest was rejected because it invents a
second authoring format. Raw copying of a vendor directory was rejected because
it is neither portable nor safe. Marking Codex native through a Skill or
instruction wrapper was rejected because that wrapper cannot create an
agent-selectable/delegable profile; the documented TOML custom-agent surface
is used instead.

## Consequences

Package authors gain one portable Markdown agent surface and the UI/README can
state support accurately. Integrations retain ownership of vendor paths and
schemas, preserving Core neutrality.

Maintainers must keep four projections and conformance scenarios current.
Some canonical metadata will intentionally remain unavailable in one or more
harnesses; route evidence rather than hidden loss communicates that constraint.
The Codex TOML generator adds a vendor projection that must track the
documented schema and real-harness behavior.

## Implementation Plan

- **Affected paths:** `crates/uze-core/src/engine.rs`, `project.rs`,
  `integration.rs`, and capability/resource tests; all four integration
  verticals in `crates/uze-integrations/src/`; `src/ui/view/harnesses.rs` and
  the harness-matrix generator; `tests/integrations/`, `tests/acceptance/`,
  `tests/_fixtures/`, and `conformance/harnesses/{claude,codex,opencode,antigravity}/`.
- **Pattern:** follow AgentSkill/MCP's capability-level `ExposurePlan`,
  `AttachmentReceipt`, inspect, and detach flow. Follow ADR-029 for shared
  discovery-root naming and ADR-013 §5 for derived artifact ownership.
- **Avoid:** vendor names or schemas in `uze-core`/`uze-application`; Store
  writes; universal permission/model translation; native claims without a
  passing real-harness scenario.
- **Dependencies/configuration:** no new dependency or configuration key is
  introduced. The canonical package layout gains only `agents/*.md`.

### Verification

- [ ] Core discovery composes one canonical Agent resource per
      `agents/<name>.md` and preserves Store bytes.
- [ ] Each native target passes route, receipt, lifecycle, collision, and
      drift tests; Codex proves the generated TOML route.
- [ ] The four conformance verticals prove agent discovery in their isolated
      synthetic environments without Internet or provider tokens.
- [ ] The TUI and generated README matrix report all four harnesses as
      native for Agents.
- [ ] `cargo test --no-fail-fast`, `cargo fmt --check`,
      `cargo clippy --all-targets -- -D warnings`, and strict OpenSpec
      validation pass.

Source change: openspec/changes/add-portable-agent-capability/
