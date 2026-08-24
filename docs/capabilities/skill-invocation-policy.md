# Skill Invocation Policy — capability spec

Companion to [landscape.md](landscape.md). Implements
[ADR-030](../adr/030-skill-plus-invocation-policy.md), which supersedes the
Command-as-capability model of [ADR-025](../adr/025-commands-as-first-class-capability.md)
and [commands.md](commands.md) (retained as history).

## Model

The canonical capability set has **one Skill-kind**: `AgentSkill`. Whether a
Skill is background knowledge the model may auto-select, an explicit action
only the user triggers, or both is the Skill's **invocation policy**, not a
second capability kind.

Canonical Skill (v0, minimal by design):

```
plugin/
├── plugin.json
├── skills/
│   └── review/SKILL.md
└── mcp.json                               ← MCP, unchanged
```

`commands/` is **not** canonical. A vendor-administered `commands/`
directory inside an explicit vendor envelope is native delivery the author
shipped; it is never re-discovered canonically.

## Invocation policy

Declared in the canonical SKILL.md frontmatter:

```markdown
---
name: review
description: Review the current changes
invoke:
  model: false
  user: true
---
```

| `model` | `user` | Meaning |
|---|---|---|
| `true` | `true` | default — normal interactive/discoverable Skill |
| `true` | `false` | background/model-only capability |
| `false` | `true` | explicit user action (what Command used to be) |
| `false` | `false` | invalid — nobody can invoke it; UZE never projects it |

- **No `invoke:` block ⇒ default (model + user)** — an existing SKILL.md
  behaves exactly as before, byte-for-byte.
- Only `true`/`false` literals are recognized; a non-boolean value marks
  the block invalid rather than silently degrading to a model-visible or
  user-visible default.
- Unknown `invoke:` sub-keys and all other frontmatter fields are ignored:
  the Store preserves canonical bytes, and no universal Skill schema is
  introduced.

## Semantics per harness (official docs, verified)

| Harness | `model=false` | `user=false` | Route |
|---|---|---|---|
| Claude Code | `disable-model-invocation: true` | `user-invocable: false` | Native (both); explicit-envelope coverage requires the author's own marker (never rewritten) |
| Codex | `agents/openai.yaml` → `policy.allow_implicit_invocation: false` | not expressible | Native (`model=false`); Degraded (`user=false`, stated honestly) |
| OpenCode V2 | `metadata.opencode/autoinvoke: false` | `slash: false` | Native (both); vendor Command primitive never needed |
| Antigravity | not expressible (skills stay model-discoverable) | not expressible (skills stay slash-invocable) | Adapted, with the degradation named in the plan evidence |

A vendor Command may be generated *from* a canonical Skill when that is the
most native representation of its policy — a projection detail, never a
canonical concept.

## Delivery rules

- **Store stays byte-preserving.** A non-default policy is translated only
  into derived artifacts under `$UZE_HOME` (generated wrappers, vendor
  sidecars), never into the Store.
- **Route classification is policy-aware.** A capability-level fallback or
  a package envelope claims a Skill only when the policy is actually
  preserved; degraded delivery is reported, never silently covered.
- **Labels are presentation-only.** `<plugin>:<skill>` (ADR-026) names the
  physical entry; it is not part of canonical identity.
