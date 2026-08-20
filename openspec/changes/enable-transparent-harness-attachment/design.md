## Context

See proposal.md - Why. The prior change built `IntegrationPort`, `ExposurePlan` /
`ExposureMechanism` (`DirectNative`, `RuntimeBridge`, `FilesystemProjection`,
`Unsupported`), `UzeHome` (store/state/cache/runtime paths), and `UzeStore`
(single install per package). Today `ClaudeIntegration::exposure_plan` always
returns `RuntimeBridge { bridge: "Claude Code --plugin-dir", .. }` and
`CodexIntegration::exposure_plan` always returns `FilesystemProjection`
scoped to a caller workspace and a runtime session directory
(`$UZE_HOME/runtime/<integration>/<session>`). Both are correct as
conformance-probe mechanisms but neither is installed once and left in
place — `prepare()` is called immediately before each probe spawns the
harness.

Official-docs research (see `research-notes.md`) found, and this session
empirically verified against the real `claude` and `codex` CLIs in an
isolated `$HOME` (no production `~/.claude`/`~/.codex`/`~/.agents` touched):

- **Claude Code**: `~/.claude/skills/<name>/` (a `.claude-plugin/plugin.json`
  with `"skills": ["./"]`, plus `SKILL.md` at that same root) is a
  user-scope, cwd-independent "skills-dir plugin" that Claude auto-loads at
  the start of every future session — confirmed via `claude plugin init
  --help`. A symlinked version of that same directory pointing outside
  `~/.claude` was validated and listed as `Status: ✔ loaded` by `claude
  plugin validate`/`claude plugin list`, byte-identical to a control run
  against a real, non-symlinked scaffold. Classified `PROVEN` at the
  discovery/loading level; full behavioral proof (a real headless prompt
  returning the skill's content) is deferred to an opt-in, auth-gated
  conformance test, consistent with how `--plugin-dir` conformance already
  works in this codebase.
- **Codex CLI**: `$HOME/.agents/skills/<name>` is a first-class, user-scope
  discovery location distinct from the per-project `.agents/skills`
  repo-root walk, and official docs state symlinked skill folders are
  followed there. Classified `PROVEN`.
- MCP was evaluated and rejected on both harnesses for this capability: it
  is a different primitive (Tools, not `SKILL.md` Agent Skills) and its
  configuration is static, not dynamically cwd- or session-aware.
- Neither harness has a documented mechanism for a hook to *register* a new
  invocable Skill at runtime; hooks only inject text context. This rules out
  a hook-driven dynamic-registration design for the baseline requirement on
  both harnesses.

## Goals / Non-Goals

**Goals:**
- One new `ExposureMechanism` variant modeling a persistent, UZE-owned,
  user-scope reference, distinct from the existing session-scoped
  mechanisms.
- `uze setup` (unified command, `uze setup claude`/`uze setup codex` as the
  internal per-harness slice) that is idempotent and harness-independent.
- `uze add` causes every already-set-up integration to create/refresh its
  managed attachment as part of the same operation — no separate sync step.
- Minimal, secret-free integration state under `$UZE_HOME/state/`.
- A minimal `uze doctor` sufficient to diagnose this change's own setup and
  E2E, not a general-purpose feature.
- Real-harness E2E that structurally cannot conflate "setup succeeded" with
  "runtime transparency verified," always against isolated homes.

**Non-Goals:**
- No daemon. Both selected mechanisms are pure filesystem discovery
  evaluated by the harness itself at its own session start; UZE never needs
  to be running or listening for that discovery to work. This is revisited
  only if a future need (e.g. live drift-repair) turns out to require one —
  none has surfaced in this research pass.
- No `uze claude`/`uze codex` launcher and no process wrapper on PATH.
- No expansion of `src/capability.rs`, `src/router.rs`, `src/engine.rs`
  domain logic, and no new capability kinds beyond Agent Skill.
- No Cursor or Windsurf work. OpenCode's existing `FilesystemProjection`
  conformance path is unchanged.
- No change to the existing `RuntimeBridge`/`FilesystemProjection`
  conformance-probe mechanisms — they remain as documented, opt-in,
  auth-gated behavioral verification paths, now clearly secondary to the
  transparent mechanism rather than the only mechanism.

## Decisions

### 1. New `ExposureMechanism::ManagedUserScopeReference` variant
Rather than overloading `FilesystemProjection` (which is explicitly
session/workspace-scoped, with a `runtime_session_dir` and
project-workspace-relative target), add a distinct variant for a reference
that lives at the harness's *user* scope, is not tied to any one project
workspace or session, and is expected to persist across `uze add`
operations rather than being created and torn down per invocation.

Shape: `{ discovery_root: PathBuf, entry_name: String, source: PathBuf }`
where `discovery_root` is the harness's user-scope skills directory
(`~/.claude/skills` or `~/.agents/skills`), `entry_name` is the
UZE-namespaced entry (see Decision 3), and `source` is the path inside the
UZE store the reference points at.

**Alternative considered**: reuse `FilesystemProjection` with a workspace of
`$HOME` instead of the project workspace. Rejected — it conflates two
different lifecycles (per-session managed artifact with RAII cleanup vs.
a persistent reference that outlives any single harness invocation) and
would make `prepare()`'s workspace parameter lie about what "workspace"
means for this mechanism.

### 2. `uze setup` is per-harness and idempotent by construction
Each integration exposes what setup actually needs (see IntegrationPort
change below): `detect()` (is the harness present, what version) and
`install()` (ensure the discovery root exists, record state). `install()`
checks existing state before writing; a second call recognizes the
integration is already installed and only refreshes state facts (e.g.
detected version), never recreates the discovery root or duplicates a
registry entry.

### 3. Namespacing prevents collision with pre-existing, unrelated entries
Both harnesses' user-scope skills directories are shared, ambient state —
this machine's real `~/.agents/skills` already has an unrelated
`.skill-lock.json`-managed entry that predates UZE. Every UZE-managed entry
uses a `uze-<package-id>` (or equivalent stable, namespaced) name. UZE only
ever creates, refreshes, or removes entries whose name it owns; it never
enumerates or mutates the discovery root's other contents.

### 4. `IntegrationPort` grows only what the research showed is needed
Derived from the research, not designed speculatively: `detect()` (harness
presence/version), `install()` (idempotent one-time setup, returns what it
did/would do), and `status()` (current installed/managed state for
`uze doctor`). No `uninstall()` speculative method is added in this slice
unless `uze setup --repair`/removal is implemented as part of this change's
tasks; if included, it mirrors `install()`'s idempotency. `exposure_plan()`
is unchanged in signature; each integration's implementation now prefers
`ManagedUserScopeReference` when its setup has completed, and falls back to
the existing `RuntimeBridge`/`FilesystemProjection` conformance mechanism
otherwise (so the existing opt-in conformance tests keep working
unmodified).

### 5. Setup-phase and runtime-phase E2E are structurally separate tests
Mirrors the spec requirement directly: one test suite proves `uze setup`
produces the expected on-disk state and is idempotent (no real harness
process spawned). A second, separately opt-in suite spawns the real harness
with zero UZE arguments and no test-authored preparation call immediately
before the spawn — only what `uze setup`/`uze add` already left in place.
Both use a temporary `$HOME` and `$UZE_HOME` created by the test, never the
operator's real configuration.

## Risks / Trade-offs

- **[Risk]** Claude's symlink support is confirmed only through `claude
  plugin validate`/`list`/`details`, not a real behavioral prompt (no
  `ANTHROPIC_API_KEY` was available in this isolated-HOME research pass, and
  copying real credentials into a throwaway home was deliberately avoided).
  → **Mitigation**: keep `VerificationStatus` at `Unverified` for the
  managed reference until an opt-in, authenticated runtime-phase test
  actually runs and observes the proof token; do not report `Verified`
  from discovery-level evidence alone.
- **[Risk]** Both mechanisms depend on undocumented-but-observed or
  documented-but-versioned CLI behavior (`claude plugin` subcommands,
  Codex's `~/.agents/skills` USER scope) that could change in a future
  harness release. → **Mitigation**: `uze doctor` re-checks state on demand
  rather than trusting a one-time install forever; version is recorded per
  harness precisely so a future regression is diagnosable.
- **[Risk]** A symlink pointing into `$UZE_HOME` becomes dangling if
  `$UZE_HOME` moves (env var change, reinstall). → **Mitigation**: this is
  strictly safer than a project-local projection (no project directory
  deletion can break it), and `uze doctor`/`uze setup` re-verification is
  the designed repair path — not a live watcher.
- **[Trade-off]** Attachment is global per user, not scoped to a project —
  every UZE-managed skill becomes visible in every project for that user
  once attached. This is what "transparent, no per-project config" requires
  by construction; project-scoped opt-out/opt-in is out of scope for this
  change and not precluded by this design.
