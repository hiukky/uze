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

- [x] `uze setup` is idempotent for Claude Code and Codex independently.
- [x] `uze add` produces a working symlink-based attachment for both without
      a separate sync step.
- [x] A setup-phase test and a runtime-phase test exist as distinct,
      separately opt-in suites; a setup-only pass is not reported as
      runtime-verified.
- [x] Real-harness runtime probes run only against temporary, isolated
      `$HOME`/`$UZE_HOME`.
- [x] Quota/auth failures during a runtime probe classify as
      `BLOCKED_BY_ENVIRONMENT`, never as incompatibility.
- [x] Rust, OpenSpec, and LikeC4 validation pass.

Source change: openspec/changes/enable-transparent-harness-attachment/

## More Information

2026-08-20: Implemented `ExposureMechanism::ManagedUserScopeReference`,
per-harness state under `$UZE_HOME/state/integrations.json`, `IntegrationPort`
`detect`/`install`/`status`/`attach`, and `uze setup`/`uze doctor`.
`ClaudeIntegration`/`CodexIntegration::exposure_plan` now prefer the managed
reference once `uze add` has recorded setup for that harness, falling back to
the ADR-005 conformance mechanisms otherwise; existing conformance tests
pass unmodified against that fallback. `cargo test`, `cargo clippy -D
warnings`, `cargo fmt --check`, `openspec validate --strict`, and `likec4
validate` all pass.

2026-08-20: Ran the real, opt-in setup-phase probe (`UZE_E2E_UZE_HARNESSES=
claude,codex`) against a temporary, isolated `$HOME`/`$UZE_HOME`: both real
CLIs (Claude Code 2.1.237, Codex 0.148.0) were detected, `install()` created
`~/.claude/skills/` and `~/.agents/skills/`, and a second run was idempotent
(no duplicate `integrations.json` entries). Also ran `uze setup` + `uze add`
manually against an isolated home outside the test harness: Claude's own
`claude plugin validate`/`claude plugin list` recognized the UZE-produced
symlinked skills-dir plugin as `Status: ✔ loaded`, matching the standalone
research finding — this confirms the production code path, not just the
manual research rig.

2026-08-20: Ran the real, opt-in runtime-phase probe against the same style
of isolated, credential-less home. Codex correctly returned
`BLOCKED_BY_ENVIRONMENT` (HTTP 401 on the OpenAI websocket/HTTPS transport).
Claude Code's response (`"Not logged in · Please run /login"`, `is_error:
true`, `api_error_status: null`) was not caught by the existing structured
or textual environment-block checks and was misclassified `FAILED` — fixed
by adding "not logged in" / "please run /login" / "please log in" to
`conformance::is_environment_block`, with a regression test using the exact
observed response shape. Re-running afterward, Claude also classified
`BLOCKED_BY_ENVIRONMENT`. Neither harness reached a real prompt in this
credential-less environment, so behavioral (`VERIFIED`) proof of the
managed-reference path remains outstanding pending real credentials in an
isolated home — the discovery-level evidence (Claude's own `plugin
validate`/`list`, Codex's documented and independently confirmed
symlink-following) is what currently backs this ADR's decision, not a
successful authenticated run.

2026-08-20: Architectural discovery, recorded before starting the next
tracer bullet (MCP): **transparent harness attachment does not imply a
persistent UZE runtime.** Agent Skill attachment resolves entirely at
`uze setup`/`uze add` time — no UZE process, daemon, hook, or IPC channel
participates once the managed reference exists; the harness's own native
discovery does all the work at its own session start. This generalizes to a
two-category model for how any `Integration` may satisfy a capability:

```
Integration
    │
    ├── Static Attachment   (resolved at setup/install time)
    │      e.g. native discovery + a symlink/reference, or a generated shim
    │      — Agent Skills today: ExposureMechanism::ManagedUserScopeReference
    │
    └── Runtime Attachment  (the harness consults something at its own
           runtime, not just at startup discovery)
           e.g. MCP, IPC, a hook, a bridge — not yet implemented; anticipated
           for the MCP tracer bullet
```

No new types were added for this — it is a naming/categorization of what
already exists, kept only in documentation until a second category actually
needs code. `ExposureMechanism::RuntimeBridge` (Claude's `--plugin-dir`) and
the session-scoped `FilesystemProjection` are **not** "Runtime Attachment"
in this sense: both remain conformance-probe mechanisms only (ADR-005),
never the product path, and are unaffected by this categorization. Two
stray `bridge`-worded rationale strings in `ClaudeIntegration::exposure_plan`
that predated the managed-attachment path (and had become misleading once
that path existed) were reworded to "attachment."

**Confidence distinction, preserved for `uze doctor` and conformance
evidence, deliberately not modeled as an enum yet:** Codex's user-scope
Skills discovery and its symlink-following are both stated in Codex's own
official documentation — `DOCUMENTED`. Claude's user-scope skills-dir
discovery is documented (`claude plugin init --help`), but its
symlink-following is known only from this project's own controlled
experiment against the real CLI (§3 above) — `EMPIRICAL`, not a published
contract, and could regress on a future Claude Code release without notice.
The opt-in conformance evidence emitted by
`emit_transparent_attachment_evidence` (`tests/uze_harness_conformance.rs`)
now carries a `confidence` field (`"documented"` for Codex, `"empirical"`
for Claude) alongside `setup_strategy`, so this distinction is auditable
per-run rather than only asserted in prose. A future `uze doctor` can surface
it (e.g. `strategy: user-skill-symlink, confidence: empirical`) without any
design change here — the fact already exists, only the CLI surface is
future work.

2026-08-20: **Behavioral E2E closed for both harnesses, with real, explicit
consent.** The automated isolated-home suite could not authenticate without
either copying real credentials into a throwaway home or breaking isolation
— both rejected. Instead, with the operator's explicit go-ahead, the real
`uze` binary was run once, manually, outside the automated test suite,
directly against the operator's real machine (`$HOME` and default
`$UZE_HOME` — no overrides): `uze setup` then `uze add` on the same fixture
package used everywhere else. Pre-flight check confirmed a clean baseline
(`~/.claude/skills` and `~/.uze` did not exist yet; `~/.agents/skills` had
23 pre-existing, unrelated entries and no `uze-` prefix — confirming the
namespacing invariant holds against real ambient state, not just a
contrived test fixture). Both attachments were created
(`~/.claude/skills/uze-uze-agent-skill-conformance-uze-e2e`,
`~/.agents/skills/uze-uze-agent-skill-conformance-uze-e2e`), and a plain
invocation of each harness's real CLI, using the operator's real,
already-authenticated OAuth session, with zero UZE arguments, in a fresh
project workspace, returned the exact proof token:

- `codex exec` (model `gpt-5.6-luna`, `--sandbox read-only`): resolved the
  skill through `~/.agents/skills/...` → `UZE_E2E_SKILL_PROOF_20260820`.
- `claude -p` (model `haiku`): resolved the skill through
  `~/.claude/skills/...` → `"result":"UZE_E2E_SKILL_PROOF_20260820"`
  (cost: $0.028).

This is genuine `VERIFIED` status, not `BLOCKED_BY_ENVIRONMENT` — Agent
Skills transparent attachment is proven end to end, behaviorally, for both
reference harnesses, closing the one item the isolated automated suite
could not reach. The automated suite's isolation discipline is unchanged
going forward: this was a deliberate, one-time, explicitly-consented
exception outside `cargo test`, not a new pattern the test suite itself
adopts. Separately, `bootstrap_codex_api_key_login` was added to the
automated opt-in suite as an always-available, zero-risk alternative for
future runs: it only activates when `OPENAI_API_KEY` is present in the
operator's own shell, and scopes `codex login --with-api-key` to the same
isolated `$HOME` the suite already uses — never the real one. Claude Code
needs no equivalent: it already falls back to `ANTHROPIC_API_KEY` from the
inherited process environment when no OAuth session file exists in the
isolated home.
