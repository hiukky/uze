# Claude Command Explicit Invocation via Generated Frontmatter

Status: Superseded by [ADR-030 (Skill + Invocation Policy replace the canonical Command)](030-skill-plus-invocation-policy.md)

Refines: [ADR-025 (Commands as a First-Class Capability)](025-commands-as-first-class-capability.md).

This decision is retained as history: its technique (injecting
`disable-model-invocation: true` into generated Claude Skill files, and
byte-auditing an explicit envelope for the same marker before claiming
coverage) was absorbed into ADR-030's Claude mapping and now applies to
canonical Skills carrying `invoke.model: false`.

## Context

ADR-025 classifies a Claude Code `commands/*.md` file as **Native** Command
delivery — both for a UZE-generated envelope (`claude/generate.rs`, no
explicit `.claude-plugin/plugin.json`) and for an author's own explicit
envelope — on the reasoning that Claude's plugin `commands/` directory is a
first-class, officially supported mechanism.

That classification did not verify one property ADR-025's own definition of
`Command` requires: *the model cannot auto-select it*. Claude Code
documents having "merged" custom commands into Skills — a command file and
a skill file both create `/name` and, by default, **both are
model-invocable**. Nothing in the canonical UZE Command model (frontmatter's
only consumed field is `description`) or in the generated envelope
(`materialize_generated_package` symlinked the whole `commands/` directory
verbatim) ever told Claude to keep a Command out of its own auto-selection.
Every UZE-delivered Claude Command was silently model-invocable —
`docs/capabilities/command-skill-exposure.md`'s summary table recorded this
plainly as "Command: visível pro agente: sim" for Claude, the same failure
mode ADR-025 already declares (not silently covers) for Antigravity.

Claude does have a mechanism for this: a `disable-model-invocation: true`
frontmatter field. But the public record on it, specifically for
plugin-scoped files, is unreliable enough that it could not be trusted from
documentation alone:

- [anthropics/claude-code#22345](https://github.com/anthropics/claude-code/issues/22345)
  (open, filed Feb 2026 against 2.1.29): the field is silently ignored for
  *plugin* skills — exactly UZE's delivery mechanism.
- [anthropics/claude-code#38969](https://github.com/anthropics/claude-code/issues/38969)
  (closed as duplicate, filed against 2.1.83): the field also blocks
  *explicit* user invocation, which would defeat a Command's entire
  purpose — a workflow only the user can trigger, not one the user
  *cannot* trigger.
- Current official docs (`code.claude.com/docs/en/skills`, fetched live
  during this audit) say the field now works "at any level, including
  plugin skills," and describe `disable-model-invocation: true` as: "Only
  you can invoke the skill... Use this for workflows with side effects...
  You don't want Claude deciding to deploy because your code looks ready."

Given the contradiction between an open bug report and current docs, this
decision is grounded in a live proof against the real binary, not either
source alone.

### Live proof (Claude Code 2.1.241)

Method: `UZE_BYPASS=1` against the real installed binary
(`~/.local/bin/claude`, bypassing UZE's own runtime PATH shim), a
throwaway `--plugin-dir` load (session-only, no persisted state), a
planted Command carrying a unique literal token in its body, and a
plain control Skill. Two runs, everything else held constant, only the
frontmatter marker toggled:

| Setup | Explicit `/name` invocation | Model auto-invocation (real tool-use turn, unprompted by name) |
|---|---|---|
| No `disable-model-invocation` | works | model itself emits `Skill {"skill": "..."}` and returns the planted token |
| `disable-model-invocation: true` | still works — planted token returned | model reports it cannot find any matching tool |

Neither upstream bug reproduces at 2.1.241: explicit invocation is
unaffected by the marker (#38969 not reproduced), and the marker is
honored for a plugin-delivered file (#22345 not reproduced). The proof was
then repeated end-to-end against the actual output of
`materialize_generated_package` for a real UZE package
(`tests/fixtures/packages/workflow`) — not just the hand-built probe
plugin — with the same result: explicit `/workflow:review` works, and a
prompt asking the model to find a skill matching the Command's exact
canonical description returns nothing.

One red herring worth recording so it is not re-discovered: Claude's
`--debug-file` output logs a "Total plugin skills loaded: N" counter that
does **not** move when the marker is toggled — Commands and Skills load
through separate counted pools internally regardless of the flag. That
counter is not evidence either way; only the live tool-call round-trip
above is conclusive. Claude has no equivalent to Codex's `codex debug
prompt-input` (a zero-cost, zero-model-call, deterministic introspection
command) — proving this property on Claude requires an actual model turn.

## Decision

**Claude Code's Command route stays Native (per ADR-025's definition), and
UZE now actively preserves the "model cannot auto-select it" property
instead of leaving it to chance, for both delivery paths.**

- **Generated envelope** (`claude/generate.rs`): `commands/` is no longer a
  whole-directory symlink to the Store's canonical directory. UZE
  materializes one real, UZE-owned file per canonical command
  (`materialize_explicit_only_commands`, mirroring
  `antigravity/commands.rs`'s existing per-file wrapper pattern): the
  canonical `description` (if present), re-serialized as a safely escaped
  YAML double-quoted scalar — never raw-interpolated, so no description
  content (a colon, embedded quotes, an embedded newline, or text shaped
  like another frontmatter key) can ever break out of its own value or
  forge/duplicate a key — followed by an unconditionally injected
  `disable-model-invocation: true`, followed by the canonical body
  preserved verbatim. `skills/` is untouched: still a single
  whole-directory symlink, since Skills must stay model-discoverable.

  A canonical command whose bytes are not valid UTF-8 is skipped
  entirely — never written as an empty file, and excluded from
  `generated_exact_coverage`'s provided set — rather than silently
  degrading into an empty, meaningless Command. This mirrors this same
  module's existing "malformed manifest → empty coverage, nothing
  silently claimed" discipline, extended to Commands.

- **Explicit envelope** (`claude/plugin.rs`'s `claude_exact_coverage`): UZE
  never rewrites an author's explicit-envelope content — that boundary is
  unconditional and unchanged by this ADR. But a path match against the
  manifest's declared `commands` surface is no longer sufficient, by
  itself, to claim a Command as covered: the referenced file's own bytes
  must already carry `disable-model-invocation: true`, checked by a new
  shared helper (`crate::shared::command::has_disable_model_invocation`)
  against the resource's already-discovered payload (no extra I/O). A
  path-matched Command without the marker is **not** claimed as covered —
  it falls through to the same `Unsupported` per-resource fallback as a
  package with no envelope at all
  (`claude_capability_level_command_is_unsupported_outside_package_coverage`).
  This is the same root cause as the generated-envelope leak (Claude's
  `commands/` surface defaults to model-invocable), just unfixable by
  rewriting, since the bytes are the package author's.

## Consequences

- **Easier**: a UZE Command on Claude Code now actually satisfies ADR-025's
  own definition (explicit user invocation preserved, model auto-selection
  blocked) instead of silently failing it — closing the one gap
  `docs/capabilities/command-skill-exposure.md`'s harness comparison had
  flagged as unresolved for Claude alongside Antigravity's already-declared
  one.
- **Harder**: the generated envelope's `commands/` surface is no longer a
  single symlink — it is N real files, each requiring the same care around
  safe serialization the rest of this codebase already applies to
  vendor-facing generated content (see the analogous, still-open, latent
  raw-interpolation risk in `codex/commands.rs` and
  `antigravity/commands.rs`'s own `description:` line — not fixed by this
  ADR, out of scope, but the same class of bug this ADR closes for Claude).
  An explicit envelope's Command coverage is now a real behavioral
  contract on the author (their file must carry the marker itself) rather
  than a structural given — a package that never opts in stays honestly
  `Unsupported` on Claude rather than silently leaking Command semantics.
- **Backward compatible**: `skills/`-only packages and MCP delivery are
  completely unaffected. A package whose explicit envelope's Command
  already happens to carry the marker (or is regenerated as canonical
  through UZE's own generated path) sees no behavior change; one that
  doesn't now correctly reports `Unsupported` instead of a silent leak.
- **No Antigravity change**: Antigravity's Command route stays `Adapted`,
  as ADR-025 already declared — no explicit-only mechanism was found
  there, and none was introduced by this task. Live verification could not
  be extended further without writing into a user's real, persistent
  `~/.gemini/antigravity-cli/skills/` (Antigravity has no `--plugin-dir`-
  style session-only load), so that was not attempted without explicit
  authorization.

## Conformance

`tests/skill_invocation_conformance.rs::claude_generated_package_covers_a_user_only_skill_and_materializes_the_marker`,
`::explicit_envelope_command_without_the_marker_is_not_covered`; unit tests
in `claude::generate::generated_native_tests` (real-file-not-symlink, marker
injection, safe frontmatter escaping against a forging/duplicating
description, non-UTF8 never emptied, deterministic rebuilds, exact
coverage excluding an unrepresentable Command) and
`claude::plugin::claude_native_coverage_tests` (marker present/absent
controls explicit-envelope coverage); `crate::shared::command`'s own unit
tests for `escape_yaml_double_quoted` and `has_disable_model_invocation`.
