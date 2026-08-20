# Attach UZE packages through persistent user-scope skill references

Status: Accepted

## Context

ADR-005 established Claude Code and Codex as peer integrations and proved
each can *consume* a UZE-stored Agent Skill through an explicit,
per-invocation mechanism: Claude's `--plugin-dir <path>` flag and a
UZE-prepared filesystem projection for Codex, created immediately before
each conformance probe spawns the harness. ADR-005 already noted that
"local Claude CLI help confirms official plugin lifecycle commands and a
global skills directory, so a one-time transparent connector is possible
but unproven" — this decision resolves that open question.

The product target is `uze setup` once, `uze add <package>` once, then
plain `claude`/`codex` invocations with the capability already available —
no `uze claude`/`uze codex` launcher, no `uze sync`, no manual per-project
vendor configuration, no process wrapper on PATH. Official-docs research for
both harnesses (recorded in `research-notes.md`) found no mechanism by which
a hook or MCP server can dynamically register a new invocable Skill at
session start on either harness — hooks only inject text context, and MCP
exposes a different primitive (Tools). This ruled out a hook-driven design
for the baseline requirement.

The same research found, and this change's author empirically verified
against the real `claude` and `codex` CLIs in a fully isolated `$HOME` (the
operator's real `~/.claude`/`~/.codex`/`~/.agents` were never touched):
Claude Code auto-loads a "skills-dir plugin" from `~/.claude/skills/<name>/`
at the start of every session, and a **symlink** at that path pointing
outside `~/.claude` was validated and listed as loaded by Claude's own
`plugin validate`/`plugin list` commands, indistinguishable from a real,
non-symlinked directory in a controlled comparison. Codex CLI documents
`$HOME/.agents/skills/<name>` as a first-class, cwd-independent user-scope
discovery location and explicitly states symlinked skill folders are
followed there.

## Decision

UZE will attach a package's Agent Skill to Claude Code and Codex through a
**persistent, UZE-managed symlink placed once in each harness's user-scope
skill discovery directory** (`~/.claude/skills/<name>/` for Claude,
`~/.agents/skills/<name>` for Codex), pointing at the skill's location
inside the UZE store. `uze setup` creates the discovery directory and
records per-harness integration state; `uze add` creates or refreshes the
symlink for every harness whose setup has completed. No harness-specific
launcher, wrapper, or per-session sync command is introduced. Both harnesses
converge on the same mechanism shape (a user-scope managed symlink) by
coincidence of what each harness's own documented surface actually offers,
not because UZE forced a shared strategy — MCP and hook-based alternatives
were evaluated and rejected on both for lacking dynamic Skill registration,
and a process wrapper was rejected outright per the product's own
constraint against replacing the harness executable on PATH.

The existing `--plugin-dir` and filesystem-projection mechanisms from
ADR-005 are retained, unchanged, as secondary/fallback conformance
mechanisms — not replaced. Claude's transparent path is not yet claimed
`VERIFIED`: discovery-level evidence (Claude's own validate/list tooling
treating the symlink as loaded) is not behavioral evidence (a real prompt
returning the skill's content), and no `ANTHROPIC_API_KEY` was available in
the isolated research environment to close that gap without copying real
credentials out of the operator's production home, which was deliberately
avoided.

Alternatives rejected: a background daemon (neither mechanism needs one —
both are pure filesystem discovery the harness itself evaluates at its own
session start); reusing the existing `FilesystemProjection` mechanism
in-place (it is explicitly session/workspace-scoped with RAII cleanup,
which is the wrong lifecycle for a reference meant to outlive any single
invocation); and a `uze claude`/`uze codex` launcher or PATH wrapper (both
explicitly rejected by the product's own constraints, not evaluated as
serious contenders).

## Consequences

Easier: normal `claude`/`codex` invocation becomes the actual product
experience for an installed package, matching the North Star; the store
remains the single source of truth (a symlink, not a copy, so store updates
propagate without UZE recreating anything); attachment state is diagnosable
through a minimal `uze doctor` without exposing harness credentials.

Harder: attachment is per-user, not per-project — every UZE-managed skill
becomes visible in every project for that user once attached, which is
inherent to using a user-scope discovery location rather than a
per-project one; project-level opt-out is not addressed by this decision.
Both mechanisms depend on harness-internal, partly undocumented behavior
(Claude's symlink-following was empirically observed, not found written
down anywhere) that a future harness release could change without notice —
`uze doctor` re-verification, not a live watcher, is the accepted mitigation.
Claude's transparent path carries an explicit `Unverified` behavioral status
until an authenticated opt-in runtime probe actually observes the proof
token.

## Implementation Plan

- **Affected paths:** `src/exposure.rs` (new `ExposureMechanism` variant),
  `src/integration.rs` (`detect`/`install`/`status` on `IntegrationPort`),
  `src/integrations/claude.rs`, `src/integrations/codex.rs`, `src/home.rs`
  or a new `src/state.rs` (per-harness integration state under
  `$UZE_HOME/state/`), `src/main.rs` (`uze setup`, `uze doctor`), tests
  (deterministic setup-lifecycle tests with fake harnesses, plus opt-in
  setup-phase and runtime-phase real-harness E2E against isolated homes).
- **Patterns to follow:** the store remains the only place package content
  lives; integrations own only references and their own lifecycle; state
  records operational facts only, never secrets; real-harness verification
  stays opt-in and structurally separate from deterministic tests.
- **Patterns to avoid:** copying package content into a second permanent
  location; a shared/global mutable "current attachment" concept that
  forces symmetry between harnesses; claiming `VERIFIED` from discovery-level
  evidence alone.

### Verification

- [ ] `uze setup` is idempotent for Claude Code and Codex independently.
- [ ] `uze add` produces a working symlink-based attachment for both without
      a separate sync step.
- [ ] A setup-phase test and a runtime-phase test exist as distinct,
      separately opt-in suites; a setup-only pass is not reported as
      runtime-verified.
- [ ] Real-harness runtime probes run only against temporary, isolated
      `$HOME`/`$UZE_HOME`.
- [ ] Quota/auth failures during a runtime probe classify as
      `BLOCKED_BY_ENVIRONMENT`, never as incompatibility.
- [ ] Rust, OpenSpec, and LikeC4 validation pass.

Source change: openspec/changes/enable-transparent-harness-attachment/
