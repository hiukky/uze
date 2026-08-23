# Stable Namespaced Invocation Labels

Status: Accepted

Refines: [ADR-025 (Commands as a First-Class Capability)](025-commands-as-first-class-capability.md),
[ADR-013 (Native Projection Principle)](013-adopt-native-projection-principle.md).

## Context

Until now UZE projected Skill and Command capabilities with a
*bare-first / qualified-on-collision* naming strategy: a first package's
`review` was exposed as `review`, and only when a second package's `review`
appeared did one of them fall back to a hyphenated package-qualified name
(`beta-review`).

That strategy made the exposed name depend on global state:

- the visible name changed when another plugin was installed;
- `beta-review` is not structurally a namespace (a `-` inside one flat word);
- the capability's origin is not immediately visible;
- collision resolution leaked into the user's invocation UX.

A package's capabilities should instead be addressable by a **stable,
plugin-qualified invocation label** that is deterministic by construction and
independent of which other plugins are installed.

## Decision

**Plugin capabilities exposed through UZE use a stable plugin-qualified
invocation label.**

Semantic form:

```
<plugin>:<capability>
```

Examples: `flow:review`, `flow:commit`, `openspec:proposal`,
`security:audit`.

Properties:

- **Presentation concern.** The label is what a harness user invokes
  (`/name`, `$name`, or the harness's equivalent). It never replaces the
  canonical Resource identity (`package:flow:commands/review.md`), the
  package layout, the canonical plugin format, `CapabilityKind`, or the
  capability body.
- **Deterministic and predictable.** One candidate per capability, derived
  only from `(plugin id, logical capability name)` — never from which other
  plugins are installed.
- **Independent of installation order.** Installing another plugin (even one
  with the same capability name) never renames an existing capability.
- **No bare aliases in v0.** `review` alone is not exposed; a bare-alias
  layer (ambiguity, alias lifecycle, extra receipts) is explicitly future
  work.
- **Vendor owns physical syntax.** The harness integration encodes the
  semantic label in the physical representation the vendor actually
  supports, and documents the encoding. `:` is used verbatim where the
  vendor accepts it; a nested vendor path (Gemini converts `/` → `:` at
  invocation) is used where the vendor's own namespacing mechanism is
  path-based.
- **Canonical name ≠ invocation label ≠ physical artifact name.** These
  three concepts are distinct and may legitimately differ (canonical
  `review`; label `flow:review`; physical `flow/review.toml` for Gemini).

Per-harness physical encodings (each verified against current official
behavior and, where possible, the real CLI):

| Harness | Semantic label | Physical representation | Vendor namespaces? | Evidence |
|---|---|---|---|---|
| Claude Code | `flow:review` | plugin declares plain `review` (plugin named `flow`); capability-level shim directory `flow:review` with manifest plugin name `flow` | Yes — plugin skills/commands are namespaced `plugin-name:skill-name` by Claude itself | official skills docs (`/my-plugin:review`) |
| Codex | `flow:review` | user-scope skill directory and SKILL.md `name` = `flow:review` (generated wrapper for Skills and Commands) | No — UZE encodes; Codex uses frontmatter `name` for the model-visible label (verified: dir rename alone is insufficient) | codex-cli 0.149.0 `codex debug prompt-input` |
| OpenCode V2 | `flow:review` | `~/.config/opencode/commands/flow:review.md`; `~/.agents/skills/flow:review/` | No — UZE encodes; command names and skill IDs are path-derived verbatim, no name regex enforced | official docs |
| Gemini CLI | `flow:review` | `~/.gemini/commands/flow/review.toml` (nested path) | Yes — Gemini converts the path separator to a colon (`/git:commit`) | official custom-commands docs |

The Codex explicit-only Command delivery (ADR-025) is preserved exactly:
namespace never affects the invocation policy — a namespaced Command keeps
`agents/openai.yaml` → `policy.allow_implicit_invocation: false`, and a
namespaced normal Skill stays model-discoverable (no policy file).

Package coverage is untouched: `provided_resource_identities` keeps the
canonical `package:<id>:<path>` identities; invocation naming and resource
identity remain separate concerns.

## Consequences

- **Easier**: labels are predictable and inspectable; two plugins shipping
  the same capability name are independently addressable without any
  collision-derived renaming; read models can show origin immediately.
- **Harder**: the old bare-first behavior is intentionally removed —
  existing users see fully-qualified labels even without collisions; the
  Codex skill delivery now generates wrapper artifacts (frontmatter `name`
  is the vendor's label source, so renaming the directory alone was
  insufficient); per-vendor encodings must stay documented.
- **Known vendor constraint (same-name Skill + Command).** One package may
  legitimately ship `skills/review` and `commands/review` — same label
  `flow:review`, distinct canonical identities. Not all harnesses can
  represent both under one label:
  - Claude Code: vendor merges commands into skills; the skill takes
    precedence over a same-named command file (vendor-documented), so the
    command slot is shadowed within one plugin;
  - Codex: one user-scope skill registry; both artifacts want one physical
    entry. Package-delivered skills stay inside the plugin (no collision);
    decomposed same-name Skill+Command is blocked deterministically
    (`ManagedEntryConflict`) rather than silently renamed;
  - OpenCode V2 and Gemini CLI: commands and skills are separate registries,
    so both coexist naturally.
  UZE deliberately invents **no** canonical disambiguation such as
  `flow:review-skill`/`flow:review-command`; this is reported rather than
  papered over, and a canonical resolution would be a separate design
  decision.
- **Residual risk, documented.** OpenCode command/skill naming has no
  documented character restrictions beyond the recommended kebab-case regex
  (not enforced in V2); Gemini skills in the shared root are directory-
  name-derived (headless listing unavailable on the validated build). If a
  future vendor build rejects `:`, its integration falls back to a
  deterministic documented encoding (e.g. nested path) without touching
  Core.
