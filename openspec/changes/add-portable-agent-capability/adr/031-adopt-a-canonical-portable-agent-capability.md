# Adopt a canonical portable Agent capability

Status: Accepted

Refines: [ADR-013 (Native Projection Principle)](../../../../docs/adr/013-adopt-native-projection-principle.md),
[ADR-013 §3 (Generated Native Package)](../../../../docs/adr/013-adopt-native-projection-principle.md),
and [ADR-030 (Skill + Invocation Policy)](../../../../docs/adr/030-skill-plus-invocation-policy.md).

## Context

`CapabilityKind::Agent` is already recognized during package acquisition, but
the Engine does not compose package `agents/` resources and no integration
routes them. As a result, package authors cannot distribute reusable agent
profiles through UZE even though Claude Code, OpenCode, and Antigravity CLI
now document Markdown agent definitions and their interactive agent managers.
Codex CLI has no documented native package component for agents.

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
- Codex receives only a generated, non-executable safe adapter. Its route is
  `Adapted`, and its evidence names the absent native agent-selection and
  delegation semantics. It MUST NOT be described as native until a documented
  Codex component and real-harness proof exist.
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
agent-selectable/delegable profile.

## Consequences

Package authors gain one portable Markdown agent surface and the UI/README can
state support accurately. Integrations retain ownership of vendor paths and
schemas, preserving Core neutrality.

Maintainers must keep four projections and conformance scenarios current.
Some canonical metadata will intentionally remain unavailable in one or more
harnesses; route evidence rather than hidden loss communicates that constraint.
Codex's adapter adds maintenance work without completing the full native
agent experience, which is accepted to make the portable capability useful and
honest now.

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
      drift tests; Codex reports `Adapted` with its stated limitation.
- [ ] The four conformance verticals prove agent discovery in their isolated
      synthetic environments without Internet or provider tokens.
- [ ] The TUI and generated README matrix report Claude/OpenCode/Antigravity
      as native and Codex as adapted for Agents.
- [ ] `cargo test --no-fail-fast`, `cargo fmt --check`,
      `cargo clippy --all-targets -- -D warnings`, and strict OpenSpec
      validation pass.
