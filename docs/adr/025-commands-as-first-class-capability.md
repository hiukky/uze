# Commands as a First-Class Capability

Status: Accepted

Refines: [ADR-013 (Native Projection Principle)](013-adopt-native-projection-principle.md) §2,
[ADR-008 (Plugin First, Capability Aware delivery)](008-adopt-plugin-first-capability-aware-delivery.md).
Supersedes the "do not model Commands" recommendation of the 2026-08-21 M3
research pass (`docs/capabilities/commands.md`), on updated official
evidence.

## Context

The 2026-08-21 M3 capability research (`docs/capabilities/commands.md`,
then) recommended **not** modeling Commands as a UZE capability: Claude
Code's official documentation stated custom commands had been merged into
Skills, and the other harnesses' formats did not converge. That recommendation
carried three conditions that have since been re-examined against current
official documentation:

1. **"Three of four harnesses have Commands; one (Claude) merged them into
   Skills."** The correct reading of Claude's position is narrower than
   "Commands do not exist": Claude's *plugin* surface still ships commands
   as plain markdown files in a `commands/` directory (and via the
   `commands` manifest field), and `.claude/commands/` files still work.
   Claude merged the *user* concept into Skills; it did not delete the
   capability from its plugin format.
2. **OpenCode V2 has a stable, documented native command surface**: `.md`
   files under `~/.config/opencode/commands/` and `.opencode/commands/`,
   plus a config-based `commands` key, with `$ARGUMENTS`/`$1..$N`
   placeholders and frontmatter (`description`, `agent`, `model`,
   `subtask`). The old research's "no convergence" evidence is stale
   relative to this surface.
3. **Gemini CLI likewise**: `~/.gemini/commands/*.toml`, project-level
   commands, extension-packaged `commands/` directories, `{{args}}`
   placeholders, namespacing (`/gcp:sync`), and lowest-precedence
   extension conflict resolution (`/gcp.deploy`).
4. **Codex has no stable native equivalent**: `~/.codex/prompts/*.md` is
   officially deprecated in favor of Skills, and files there are no longer
   registered as commands since codex-cli 0.117.0 (openai/codex#15941). A
   Codex classification must be honest about this.

Meanwhile a concrete product need emerged: users want explicit,
user-invokable prompt/action surfaces (`/name`) that are *distinct* from
model-discoverable Skills, and packages want to ship both with one identity
each (`skills/review/SKILL.md` and `commands/review.md` are different
resources even when logically named the same).

`CapabilityKind` already carried an `Action` variant used by the foreign
importer for `commands/` directories — a validation-only mapping that
nothing persisted — but the variant was never modeled, routed, or delivered
anywhere. The M3 landscape paper itself flagged `Action` as a
"REMOVE_LATER candidate: no harness exposes a capability literally matching
'Action' distinct from Commands".

## Decision

- **Definition of Native (formalized).** A route is Native when the harness
  provides a first-class, officially supported mechanism that preserves the
  canonical semantics of the capability. It does **not** require the same
  vendor name, file format, or physical primitive across vendors — UZE
  models user-visible semantics, not one-to-one vendor type names. The same
  UZE Command is therefore `Native` on Claude (plugin `commands/` files),
  OpenCode (`.md` command), Gemini (`.toml` command), **and** Codex
  (official explicit-invocation-only Skill). This refines ADR-013's
  hierarchy wording without changing it: Generated Native Package / Native
  Capability stays above adaptation, and what counts as "native" is the
  semantics preserved by a supported primitive, not the primitive's name.

- **Canonical model (v0, deliberately minimal).** A command is one flat
  markdown file, `commands/<name>.md`, in a canonical package: optional
  YAML-style frontmatter (only `description` is consumed) plus the prompt
  body, preserved **verbatim**. The file stem is the command's logical name
  (`review.md` → `/review`). No canonical argument placeholder, no model
  override, no agent/subtask fields — nothing that is not proven portable.
- **Discovery (Engine).** `package_resources_at` discovers canonical
  commands as `CapabilityKind::Command` resources (`Representation::Standard`)
  via `engine::command_files` — flat, sorted, symlink-safe, `.md` + valid
  stem. Identity is path-based (`package:<id>:commands/review.md:<name>`),
  so a same-named Skill and Command are distinct by construction. Store
  bytes remain untouched; nothing is parsed at discovery time.
- **Routing.** Commands follow the existing hierarchy (Explicit Native
  Package > Generated Native Package > Native Capability > Safe Adaptation
  > Unsupported) with **no blanket package coverage**: a native envelope
  claims a Command only when that envelope actually represents it (Claude
  `commands/`, generated Gemini extension TOML). A Command is never
  reported Native through a Command→Skill collapse; adaptation is explicit
  and labeled `Adapted`.
- **Delivery per harness.**
  - **Claude Code — NATIVE via package.** Explicit envelope: `commands`
    manifest field or the conventional `commands/` directory. Generated
    envelope: `commands/` symlinked and declared. Capability-level Command
    delivery is not modeled this slice (honest `Unsupported` outside
    package coverage).
  - **OpenCode V2 — NATIVE, direct standard.** Exactly where what the
    harness reads is *the canonical file itself*: a UZE-managed symlink
    `~/.config/opencode/commands/<name>.md` → Store, byte-identical.
  - **Gemini CLI — NATIVE via generated user-scope TOML and generated
    extension commands.** `commands/<name>.md` → deterministic
    `commands/<name>.toml` (`description` + `prompt`), a Derived Artifact
    under the vendor directory (or inside the generated extension). An
    *explicit* extension never claims canonical commands (its commands are
    vendor TOML, not the same artifact).
  - **Codex — NATIVE via the official explicit-invocation-only Skill
    mechanism.** Codex has no stable native *custom-command file format*
    (custom prompts at `~/.codex/prompts/*.md` are deprecated in favor of
    Skills), but it has a first-class, officially supported mechanism for
    explicitly-user-invoked content: a skill with `agents/openai.yaml` →
    `policy.allow_implicit_invocation: false` (documented in Codex's Build
    skills reference; empirically honored by codex-cli 0.149.0 — the skill
    disappears from the model-visible prompt list only when the policy file
    is present and well-formed, verified via `codex debug prompt-input`
    against a malformed-metadata control). UZE therefore delivers a
    canonical Command as a generated user-invokable Skill carrying that
    explicit-only policy: the model cannot auto-select it (proven), the
    user invokes it explicitly (`$name` mention / `/skills` selector, per
    official docs; interactive-only verification tier), body/description/
    identity are preserved, and the canonical `Command` identity is never
    replaced by a Skill identity. Per this ADR's own Native definition (an
    officially supported primitive that preserves the canonical capability
    semantics — not an identical file format or primitive name), this route
    is **Native**, and it is classified Native in `capabilities()` and
    `exposure_plan`.
- **Arguments.** No universal placeholder is defined in v0. The three
  harnesses use non-equivalent mechanisms ($ARGUMENTS vs {{args}}); any
  translation would be premature portability. Bodies are delivered
  verbatim; argument-less commands work everywhere via each harness's
  default argument-append behavior. Vendor-specific placeholders appearing
  in an author's canonical body are preserved as author evidence, never
  generated by UZE.
- **Security.** UZE never generates shell/file interpolation. OpenCode's
  `` !`cmd` `` and Gemini's `!{...}`/`@{...}` remain vendor semantics
  preserved only when authored. A future Security Gate territory is
  documented, not implemented.

## Consequences

- **Easier**: packages can ship explicit user-invokable actions with
  per-resource identity, and `plugin inspect`/the harness read model
  distinguishes Skills from Commands per route; Codex's deprecated-prompt
  gap is stated honestly instead of forced.
- **Harder**: Gemini and Codex need deterministic format translation
  (markdown → TOML / SKILL.md), each kept vendor-specific; a future
  universal placeholder is deliberately not attempted until a proven,
  safe equivalence exists.
- **Backward compatible**: `skills/`-only packages behave exactly as
  before; no Skill is reinterpreted as a Command; no implicit migration;
  existing MCP/Instructions/Agents behavior unchanged. `Action` is gone
  from the public enum (the only prior reference was a validation-only
  importer mapping for `commands/` directories, which now maps to
  `Command`).
- **Core neutrality preserved**: the new `ManagedFile` artifact
  (`ExposureMechanism::ManagedFile` / `ManagedArtifact::ManagedFile`) is
  opaque, content-agnostic ownership of one whole generated file in a
  vendor directory — no vendor field, no command vocabulary, no template
  language.
- **Codex explicit-only is load-bearing**: the adapted Skill always ships
  `agents/openai.yaml` with `policy.allow_implicit_invocation: false`; a
  Command delivered without it would silently become model-selectable. The
  Codex route is classified **Native** per the definition above, with the
  canonical identity remaining `CapabilityKind::Command` everywhere.
