# Skill + Invocation Policy replace the canonical Command

Status: Accepted

Supersedes: [ADR-025 (Commands as a First-Class Capability)](025-commands-as-first-class-capability.md),
ADR-028 (Claude Command explicit invocation via generated frontmatter).
Refines: [ADR-013 (Native Projection Principle)](013-adopt-native-projection-principle.md) §2,
[ADR-026 (Stable Namespaced Invocation Labels)](026-stable-namespaced-invocation-labels.md).
Related: [ADR-029 (Projection Conflicts at Naming Time)](029-projection-conflicts-at-naming-time.md)
(kept, narrowed to its residual cases).

## Context

ADR-025 modeled explicit user actions as a second canonical capability
(`CapabilityKind::Command`, `commands/<name>.md`) alongside `AgentSkill`.
Its premise was that "explicit user action" and "model-discoverable
knowledge" are different *capabilities* — different resource kinds with
independent identities, each with its own native delivery per harness.
Also, Claude had merged `/commands` into Skills, Codex had no
custom-command format at all (only explicit-invocation-only Skills),
Antigravity converts commands to Skills at load, and OpenCode kept a
separate native Command primitive.

That premise has not held. Auditing the four harnesses in 2026 (Claude Code
skill docs, Codex Build skills docs, OpenCode V2 skills docs, agy 1.1.19
dogfood in `docs/architecture/antigravity-compatibility.md`) shows the
*only* portable semantic dimension is invocation: each harness can express
"the model may auto-select this" and "the user may invoke this" through its
own means:

- **Claude Code**: `disable-model-invocation: true` (user-only) and
  `user-invocable: false` (model-only) — both native SKILL.md frontmatter.
- **Codex**: `agents/openai.yaml` →
  `policy.allow_implicit_invocation: false` (user-only). No way to hide a
  skill from explicit `$skill` invocation (model-only is not expressible).
- **OpenCode V2**: `metadata.opencode/autoinvoke: false` (user-only) and
  `slash: false` (model-only) — both native SKILL.md frontmatter.
- **Antigravity**: neither — Skills are model-discoverable *and*
  slash-invocable, with no documented switch for either half.

A canonical Command was thus a vendor-taxonomy projection that happened to
match Claude's legacy `commands/` directory and OpenCode's custom Command
— not a portable semantic. It also created artificial canonical collisions
(`skills/commit` + `commands/commit` needing two identities) and forced
per-integration "Command vs Skill" special-casing everywhere.

## Decision

1. **One Skill capability, one invocation policy.** `CapabilityKind` has no
   `Command`. `AgentSkill` is the only Skill-family capability; the question
   "who may invoke it" is answered by the Skill's own `invoke:` frontmatter
   block (canonical, vendor-neutral):

   ```markdown
   ---
   name: review
   description: Review the current changes
   invoke:
     model: false
     user: true
   ---
   ```

   - `model: true, user: true` — default; a normal interactive Skill.
   - `model: true, user: false` — background/model-only.
   - `model: false, user: true` — explicit user action (was `Command`).
   - `model: false, user: false` — invalid; nobody can invoke it. UZE never
     projects it, never defaults it to something worse.

2. **Defaults preserve existing behavior.** No `invoke:` block ⇒ the
   default (model + user), discovered as no policy (`None`), delivered
   byte-preserving exactly as before. Hard backward-compatibility gate.

3. **Package surface.** `commands/` is no longer canonical. The canonical
   package is `plugin.json` + `skills/<name>/SKILL.md`; a vendor-authored
   `commands/` directory survives only inside an explicit vendor envelope
   the author shipped, delivered natively, never re-discovered.

4. **Integrations translate semantics, never copy vendor field names.**
   Canonical metadata owns semantic intent only (`invoke: model/user`).
   Each Integration emits its vendor's own encoding only where the policy
   is non-default; the Store keeps canonical SKILL.md bytes verbatim
   (wrappers/sidecars live under `$UZE_HOME` as Derived Artifacts).

5. **Route classification is policy-aware.** A vendor `Command` may be
   generated *from* a canonical Skill when that is the most native
   representation of its policy — that is a projection detail, never a
   canonical concept.

6. **Package exact coverage is semantic-aware.** A package plan claims a
   Skill only when the policy is actually preserved by the envelope; a
   Skill whose policy degrades (Antigravity non-default policies; Codex
   `user: false`) falls through to capability-level delivery, which reports
   the Degradation/Adaptation honestly.

7. **Shared `~/.agents/skills` root.** Codex + OpenCode keep the existing
   deterministic naming/reuse machinery (ADR-029). With one canonical
   Skill kind, same-name Skill/Command collisions are structurally gone;
   the residual conflict is a reused artifact that lacks the reusing
   harness's invocation encoding — detected deterministically before
   attach (`ProjectionConflict`), never a silent semantic loss.

## Consequences

- `CapabilityKind::Command`, `commands/` discovery, `command_files`,
  command-specific receipts/read models/coverage and the same-name
  Skill+Command collision code are removed.
- Claude: user-only via `disable-model-invocation: true`, model-only via
  `user-invocable: false` (marker injected into generated envelopes; an
  explicit envelope is only claimed as covered when the author's own bytes
  already carry the marker — UZE never rewrites explicit vendor content).
- Codex: user-only via the `agents/openai.yaml` policy sidecar (native);
  model-only is Degraded — Codex cannot disable explicit `$skill`.
- OpenCode: every valid combination is Native via SKILL.md metadata; the
  vendor Command primitive is not needed for a canonical Skill.
- Antigravity: non-default policies are Adapted — the exact degradation is
  named in the plan evidence, never silently covered.
- Invocation labels (`<plugin>:<skill>`, ADR-026) are presentation-only and
  unchanged: label never becomes part of canonical Resource identity.
- `ManagedFile` (`ExposureMechanism`/`ManagedArtifact`) was removed with
  the Command work: its only consumers were Command projections, so it
  became unreachable dead code (ADR-030's reachability rule). The generic
  receipt machinery, `ManagedTextRegion` and `ManagedUserScopeReference`
  stay — they are harness-agnostic primitives with live consumers.
- ADR-025/ADR-028 are superseded as decisions; kept as history.

## Proof

`tests/skill_invocation_conformance.rs` (the invocation-policy matrix:
routes, physical representations, receipts, lifecycle, Store immutability,
invalid policy, backward compatibility, shared-root reuse/conflict),
`tests/invocation_labels.rs`, `tests/integration_conformance.rs`, and the
per-integration unit suites (`claude/generate.rs`,
`claude/plugin.rs`, `codex/skills.rs`, `opencode/skills.rs`,
`antigravity/skills.rs`).
