# Capabilities

Five resource kinds come out of a package. Store bytes stay verbatim for all of
them; every harness-specific encoding is a derived artifact.

| Capability | Status | Detail |
|---|---|---|
| **Skills** | Implemented, all four harnesses | The canonical capability. Its portable semantics are its invocation policy — `invoke: {model, user}` in SKILL.md — never a second capability kind (ADR-030). See [skill-invocation-policy.md](skill-invocation-policy.md). |
| **MCP servers** | Implemented, all four | Native mechanism per harness, or folded into the plugin envelope where one exists. |
| **Instructions / project context** | Implemented | One region per contributing package inside the project's `AGENTS.md`. See [context-manager.md](context-manager.md). |
| **Agents** | Implemented | Markdown agent profiles onto each harness's native agent surface, with a generated TOML file for Codex (ADR-031). Only the verified portable frontmatter subset is claimed. |
| **Hooks** | Implemented | One authored `hooks.json` and a command ABI, compiled into the delivered artifact so no uze binary sits on the execution path (ADR-033, ADR-040). See [portable-hooks.md](portable-hooks.md). |
| **Memory** | Future | Would land inside the Context Manager boundary. |

uze ships two Skills of its own — `uze:init` and `uze:worktree` — through this
same pipeline, with no special treatment anywhere. See
[uze-skill.md](uze-skill.md).

Per-harness delivery detail, evidence and limitations live in each integration's
README (`crates/uze-integrations/src/<harness>/README.md`), with the
cross-harness view in `crates/uze-integrations/README.md`. The
[compatibility matrix](../../web/content/docs/harnesses.mdx) is generated from
the integration code itself.

## Fail closed, always

A capability request no harness can honestly satisfy routes PARTIAL or
UNSUPPORTED — never a best-effort translation that silently drops semantics. For
Hooks this is per event and per effect: a `Stop` hook is never represented as a
tool callback, and an `ask` or `transform` effect is only attached where the
harness preserves it. One capability routing Unsupported never suppresses
delivery of the others.

## Vendor limitations the projections inherit

These constrain what a route may honestly claim, and are re-verified by the
Conformance Lab rather than by static notes:

- **OpenCode `permission.ask` does not fire**
  ([anomalyco/opencode#7006](https://github.com/anomalyco/opencode/issues/7006)),
  so `ask` is never claimed there; `tool.execute.before` does not cover
  subagent-issued tool calls
  ([sst/opencode#5894](https://github.com/sst/opencode/issues/5894)), a gap the
  bridge inherits.
- **OpenCode `invoke.user=false` is adapted, not native**: `slash: false`
  withholds a Skill from the `/` catalog, but a mention still expands it. Half
  the policy is carried, and the report says so.

## Trust

`executable_capabilities` (`crates/uze-core/src/trust.rs`) surfaces hook commands
alongside MCP server commands, so the single acquisition trust prompt lists every
process a package can cause to run — not only its MCP servers.
