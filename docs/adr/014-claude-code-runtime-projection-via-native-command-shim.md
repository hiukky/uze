# Claude Code Runtime Projection via Native Command Shim

Status: Accepted

## Context

UZE keeps one canonical, portable project context — `AGENTS.md` (and `.agents/`
where a harness defines that namespace) — with `$UZE_HOME`/the Store as the
source of truth for anything UZE derives on top of it. Codex and OpenCode
read `AGENTS.md` natively. Claude Code and Gemini CLI do not: each reads its
own file (`CLAUDE.md`, `GEMINI.md`).

UZE already has one answer to that gap: `uze context reconcile` maintains a
persistent, `text_region`-owned bridge — a managed `@AGENTS.md` import inside
`CLAUDE.md`/`GEMINI.md` at the project root (`crates/uze-application/src/
application.rs`'s `BRIDGE_INTEGRATIONS`; see `docs/capabilities/
instructions-design.md` and `docs/capabilities/context-manager.md`). That
mechanism is real, tested, and unaffected by this decision. It also has a
cost this ADR exists to address: it requires every UZE-managed project to
carry a vendor-specific file, forever, in its own working tree, just to
satisfy Claude Code — one more file to commit or ignore, and the first of
what would become one near-duplicate file per harness that doesn't read
`AGENTS.md` natively.

The product goal is that a user keeps typing exactly `claude` — no wrapper
alias, no manually-supplied UZE flags, no change of habit — while UZE
delivers `AGENTS.md`'s content into that session without writing anything
into the project.

## Decision

Adopt **Runtime Projection**, delivered through a **Rust-native command
shim**:

A small, generic dispatch layer inside the `uze` binary intercepts an
invocation of `claude` via `argv[0]`, resolves the real Claude executable,
asks `ClaudeIntegration` for a runtime contribution, and `exec`s the real
binary with that contribution applied — the user's own argv untouched. For
a project with an `AGENTS.md`, the contribution is a small derived file
under `$UZE_HOME/runtime/projects/<project-id>/claude-code/CLAUDE.md`
containing only `@<absolute-path-to-AGENTS.md>`, delivered via
`--add-dir <that-dir>` plus `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1`.

This is additive, not a replacement: the persistent bridge above still
exists, still runs by default through `uze context reconcile`, and this
decision does not deprecate it. Which of the two becomes the default (or
whether both stay) for Claude specifically is explicitly **not** decided
here — see Known Limitations.

## Architecture

```
uze setup claude
  └─ UzeApplication::ensure_runtime_shim   (crates/uze-application/src/application.rs)
       ├─ creates/refreshes ~/.uze/shims/claude -> uze     (refresh_shim_symlink)
       └─ adds shims_dir to PATH in the detected shell rc  (shell_path::ensure_path_line)

$ claude ...
  └─ src/main.rs::main() — argv[0] checked before clap parsing
       └─ shim::detect() matches "claude" → shim::run()    (src/shim.rs)
            ├─ UZE_BYPASS=1?                → exec real binary, argv untouched, done
            ├─ harness_runtime::resolve_real_executable    (excludes shims_dir — no recursion)
            ├─ ClaudeIntegration::runtime_contribution(ctx) (crates/uze-integrations/src/claude.rs)
            │     └─ claude_runtime_projection() — discover_project_agents_md, project_id_for,
            │        UzeHome::runtime_projection_dir, write_atomic CLAUDE.md
            └─ exec_or_die → CommandExt::exec (Unix): --add-dir + env, then original argv
```

The vendor-neutral half (`crates/uze-core/src/harness_runtime.rs`:
`resolve_real_executable`, `discover_project_agents_md`, `project_id_for`,
the `HarnessRuntimeContribution`/`RuntimeContext` types) knows no harness by
name. The only Claude-specific code is `claude_runtime_projection` and
`ClaudeIntegration::runtime_contribution`/`supports_runtime_integration` in
`crates/uze-integrations/src/claude.rs` — reached through
`IntegrationPort::runtime_contribution` (default: passthrough) and
`IntegrationPort::supports_runtime_integration` (default: `false`; only
`ClaudeIntegration` returns `true` today). Codex, OpenCode, and Gemini get
the dispatch/exec/bypass machinery for free and currently do nothing with it.

There is no separate enabled/disabled flag anywhere. The `~/.uze/shims/
claude` symlink's own existence is the complete opt-in state — created once,
as an ordinary part of `uze setup claude`, no extra flag. Removing that
symlink is how it is turned back off.

## Why `--add-dir` + `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD`

Empirically determined against a real, unmodified Claude Code `v2.1.239`, in
an isolated `HOME`, using a canary token planted in `AGENTS.md`:

- `claude --add-dir <dir>` alone: the model reports the canary as unknown.
  `--add-dir` grants tool/file access to that directory; it does not load
  its `CLAUDE.md` as instructions.
- `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1 claude --add-dir <dir>`:
  the model recites the exact canary token. Repeated with `--tools ""`
  (disabling all tools) to rule out the model simply reading the file on its
  own initiative — it still succeeds, which is only possible if the content
  reached the model as loaded context, not as something it fetched itself.

This is the smallest verified combination that delivers `AGENTS.md` content
into a Claude Code session without writing into the project. The env var is
not listed in `claude --help`; its presence and effect were confirmed by
direct experiment, not by vendor documentation — see Known Limitations for
what that implies going forward.

## Why a Rust-native shim

- One binary, `argv[0]`-dispatched (`src/shim.rs::detect`), rather than N
  build targets or a per-harness wrapper — the "smallest safe solution"
  among the alternatives considered below.
- `std::os::unix::process::CommandExt::exec` (process image replacement, not
  spawn-and-wait) makes PTY, signals, and exit code passthrough a property
  of the mechanism rather than code that has to reproduce them. Verified
  empirically: a real interactive session driven through the shim under a
  PTY rendered Claude's workspace-trust dialog and the full main TUI
  correctly, and a deliberately invalid `--model` value's non-zero exit code
  passed through unchanged.
- Recursion is structurally excluded, not merely avoided by convention:
  `resolve_real_executable` explicitly skips any `PATH` entry that
  canonicalizes to `UzeHome::shims_dir()`, and the shim never falls back to
  a bare `Command::new(name)`/PATH re-search of its own invoked name.

## Canonical vs. Derived Artifacts

**Canonical** — never overwritten, never treated as replaceable: `AGENTS.md`
(and `.agents/` where a harness defines it), package bytes in the Store.

**Derived** — owned by UZE, live under `$UZE_HOME/runtime/projects/
<project-id>/claude-code/`, and must be: rebuildable from the canonical source
alone; non-authoritative (deleting one loses nothing — it is regenerated on
next launch); content-deterministic (`@<canonical-agents-md-path>`, nothing
else); safely disposable; never a second source of truth. The projected
`CLAUDE.md` is idempotent by construction — `claude_runtime_projection`
compares existing content before writing and skips the write when it
already matches, verified by both a unit test and a live two-concurrent-
session dogfood run that produced exactly one runtime directory.

## Alternatives Considered

| # | Approach | Working tree mutation | Native Claude mechanism | Transparent `claude` UX | Selected |
|---|---|---|---|---|---|
| A | Persistent `CLAUDE.md` (`@AGENTS.md`) in the project | Yes, permanent | Yes | Yes | Rejected as default |
| B | `ManagedTextRegion` inside `CLAUDE.md` | Yes, permanent | Yes | Yes | Rejected for this purpose |
| C | Create `CLAUDE.md` at session start, delete at exit | Yes, transient | Yes | Yes | Rejected |
| D | A Claude hook materializes `CLAUDE.md` | Yes, transient | Partially (hook-dependent) | Yes | Rejected |
| E | Shell alias/function wrapping `claude` | No | Yes | No (shell-specific) | Rejected |
| F | User runs `uze claude` / sets flags manually | No | Yes | No | Valid debug/fallback surface, not default UX |
| G | Global Claude Code settings mutation | No | Unproven | Yes (if it existed) | Not selected — no equivalent surface found |
| H | `--add-dir` + external runtime projection directory | No | Yes | Yes | **Selected** |

**A — Persistent `CLAUDE.md` in the project.** Simple, and it is exactly
what `uze context reconcile`'s existing bridge already does — kept, not
removed, by this decision (see Context). Rejected as the *only or default*
mechanism this ADR adds: it pollutes the working tree with a vendor-specific
file per harness that doesn't read `AGENTS.md` natively, and multiplying
near-identical bridge files was the exact problem statement.

**B — `ManagedTextRegion` inside `CLAUDE.md`.** This is the mechanism the
existing bridge already uses (`text_region.rs`). Considered here specifically
as *the* mechanism for project-context delivery and rejected for that
narrower purpose: it still requires a persistent project-tree artifact,
mixes UZE-owned and user-owned content in one file, and inherits ADR-009's
full drift/ownership lifecycle for something that could instead be a
disposable, external, rebuildable artifact. Runtime Projection removes the
need for a project-tree managed region for Claude instructions specifically,
without touching the region mechanism itself or its other users.

**C — Create-then-delete `CLAUDE.md` around the session.** Rejected: a
crash or `SIGKILL` leaves the artifact behind in the project; concurrent
sessions on the same project race on the same file; Git/IDE/file-watchers
observe a file that exists only sometimes; cleanup becomes a correctness
requirement instead of an optimization.

**D — A Claude hook creates `CLAUDE.md` at startup.** Rejected: still
mutates the working tree; timing between "hook runs" and "Claude resolves
its own memory/context" is not something UZE controls or has verified;
couples project-context bootstrap to a session already being constructed by
the very system that needs the context.

**E — Shell alias/function.** Rejected outright, and explicitly excluded by
the project's own Rust-native constraint: shell-specific syntax (bash/zsh/
fish/PowerShell all differ), no clean install/uninstall/reconciliation
story, argv/quoting hazards, and not implementable as anything the `uze`
binary itself owns.

**F — Explicit user invocation** (`uze claude`, or hand-typing the env var
and `--add-dir`). Kept as a real, working surface — this is literally what
`UZE_BYPASS` disables back down to, and what a user can always do by hand —
but rejected as the default UX: it breaks the existing `claude` habit,
requires every script/tool that already invokes `claude` to be rewritten,
and stops UZE from being transparent.

**G — Global Claude Code configuration.** Investigated, not selected: no
discovered settings surface resolves this per-project with the same
semantics as `--add-dir`, and a static global mutation is the wrong shape
for state that is inherently per-project and derived — it would make a
runtime fact into standing configuration.

**H — `--add-dir` plus an external, UZE-owned runtime directory.**
Selected. Native Claude mechanism, empirically proven (see above), zero
working-tree mutation, the projected directory is rebuildable and safely
shareable across concurrent sessions on the same project, and it composes
with future Claude-specific runtime artifacts without committing to what
those are yet (see Known Limitations on `.claude/`).

## Lifecycle

```
`claude` invoked
      │
      ▼
Rust-native UZE shim (src/shim.rs)
      │
      ├── UZE_BYPASS set? ── yes ──> exec real Claude, argv untouched
      │                              (skips everything below)
      ├── detect project (discover_project_agents_md from cwd)
      │
      ├── compute runtime contribution (ClaudeIntegration::runtime_contribution)
      │
      ├── materialize/reconcile runtime projection (idempotent, content-compared)
      │
      ├── resolve real Claude binary (excluding shims_dir)
      │
      ├── inject env (CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1)
      │   + argv (--add-dir <runtime-dir>, prepended before original argv)
      │
      ▼
real Claude Code (exec — same process, same pid)
      │
      ▼
normal interactive session
```

The shim does **not** delete the runtime projection on exit. Because the
artifact is rebuildable, non-authoritative, and lives under `$UZE_HOME` (not
the project), keeping it between sessions is safer than mandatory per-session
cleanup: it avoids a delete/recreate race between concurrent sessions on the
same project, and there is nothing to leak into the project either way.

The GC policy this left open is now decided, and deliberately not the shim's
job: each project's runtime directory records the canonical root it was built
for (`harness_runtime::PROJECTION_MARKER`), and
`harness_runtime::prune_projections` sweeps every one whose root no longer
exists — most of them the checkout of an agent UZE placed, removed once its
work was delivered or discarded. It runs from the workspace client's
occupancy pass (`Health::prune_runtime_projections`), never from the shim,
whose hot path stays free of directory scans. Existence of the root is the
only criterion; mtime would be wrong, because a projection already current is
skipped rather than rewritten, so its mtime says when the project last
changed rather than when it was last used.

## Failure / Bypass Semantics

**Decision, and current implementation:**

- Fail-open is structural, not conventional: `HarnessRuntimeContribution`
  has no `Err` variant. `ClaudeIntegration::runtime_contribution` can only
  return a real contribution or `passthrough`/`passthrough_with_note` — a
  bug or environment problem in the Claude-specific path cannot propagate an
  error that blocks the launch. Verified: a runtime directory blocked by an
  unremovable file in its place produces a one-line `stderr` note
  (`uze: runtime projection unavailable (...); launching claude without
  portable context.`) and Claude still starts, exit 0.
- `UZE_BYPASS` (any value other than `"0"`) is checked first in
  `shim::run`, before any project detection or state read, and short-
  circuits to resolving the real binary and `exec`ing the caller's argv
  completely unmodified.
- Recursion avoidance (`resolve_real_executable` excluding `shims_dir`) is
  unconditional, not a failure-path special case.

**One documented exception to "always fail open":** if `UzeHome::from_env()`
itself fails (`$HOME`/`$UZE_HOME` cannot be resolved), the shim exits with a
clear error instead of guessing. Without a resolved `shims_dir` to exclude,
a bare PATH search could resolve back to the shim itself — in this one case,
continuing would risk exactly the recursion the mechanism exists to prevent,
so failing closed is the safer choice. This is implemented, not aspirational
(`src/shim.rs`, the `Err(error) => die(...)` arm in `run`).

## Security and Ownership

- **PATH hijacking / recursion:** closed structurally by excluding
  `shims_dir` from resolution, not by ordering convention.
- **Project identity:** `project_id_for` hashes the *canonicalized* project
  root (FNV-1a, non-cryptographic — an identifier for cache-directory
  naming, not a security boundary) — resolves symlinks before hashing, so a
  symlinked project path and its real path collide into the same, correct
  runtime directory rather than silently diverging.
- **Untrusted `AGENTS.md`:** the shim never parses or executes `AGENTS.md`
  content — the generated `CLAUDE.md` contains only a literal `@<path>`
  import line. Whatever risk exists in Claude's own handling of imported
  Markdown is identical to the risk already accepted by the persistent
  bridge (Alternative A) and by a user manually placing the same import in
  a project `CLAUDE.md` — this decision does not introduce new content-
  execution surface.
- **Argv/environment:** the caller's argv is never reparsed, only prepended
  to; environment additions are applied on top of the inherited environment,
  never via `env_clear()` — nothing the caller already set can be silently
  dropped.
- **Concurrent sessions:** same project → same `project_id` → same runtime
  directory → same content; verified live with two simultaneous `-p`
  sessions against one project, producing exactly one runtime directory and
  no corruption.
- **Ownership boundary (ADR-009):** `$UZE_HOME/runtime/projects/*` is
  UZE-owned, entirely outside the project's working tree,
  which stays user-owned throughout. Runtime projection creates and updates
  files only inside its own UZE-owned subtree; it never writes into, deletes
  from, or overwrites anything under the project root. This separation —
  not a receipt/ledger, unlike ADR-009's package attachments — is what makes
  destructive-removal safety moot here: there is nothing project-owned to
  protect against, only a UZE-owned cache to regenerate or discard.

## Relationship to Native Plugin Projection

Orthogonal to ADR-013 (Native Projection Principle) by design, and confirmed
compatible by direct dogfood (`REPORT_CLAUDE_NATIVE_PACKAGE_TRACER.md`,
item 18 "runtime shim coexistence", stop condition 11 "runtime shim precisa
mudança: Não").

```
Plugin delivery (ADR-013):
  UZE Store → derived marketplace.json → `claude plugin install` → Claude-owned cache

Project context (this ADR):
  project AGENTS.md → UZE runtime projection CLAUDE.md → --add-dir → Claude context
```

ADR-013's hierarchy — Native Package > Native Capability > Safe Adaptation >
Unsupported — governs *capability* delivery (Skills, MCP, a whole plugin
bundle). It does not apply to *project-context* delivery, which this ADR
covers instead: Runtime Projection is not a fallback beneath a higher-
priority native-package option for context — no such native option exists
for an externally-scoped `AGENTS.md`, so there is nothing to fall back from.
The two mechanisms touch different Claude surfaces (`~/.claude/plugins/
cache/...` vs. an `--add-dir`-supplied `CLAUDE.md`) and were verified not to
interfere with each other.

## Consequences

**Easier:** a UZE-managed project's working tree never needs a Claude-
specific file to give Claude Code its `AGENTS.md` content; the projected
artifact is trivially inspectable, rebuildable, and shareable across
sessions without any coordination logic; the same generic shim/dispatch/
bypass/fail-open machinery is already in place for any future harness that
needs an equivalent mechanism, at zero marginal cost to Codex/OpenCode/
Gemini today.

**Harder:** two mechanisms now deliver the same underlying fact (Claude
reading `AGENTS.md`) — the persistent bridge and runtime projection — and
this ADR does not resolve which one is authoritative going forward, so that
ambiguity is now a real, live property of the system, not a hypothetical.
Runtime projection depends on an env var Claude Code does not document,
which could change or disappear in a future release without notice; `uze
doctor`-style re-verification, not a live watcher, is the accepted posture
already established for comparable empirical findings (ADR-006). PATH
integration now touches the operator's shell rc file, which is more
consequential than a purely in-repo change and needs its own review
attention whenever it's touched.

## Known Limitations

Distinguishing **Decision** (what was chosen), **Current implementation**
(what the code does today), and what remains **unverified** — none of the
following should be read as a stronger guarantee than stated:

- **`/compact` retention is not conclusively verified.** A scripted
  interactive probe attempting to confirm the projected context survives a
  `/compact` pass was inconclusive (timing/rendering ambiguity in the
  automation, not a negative result) — this needs a short manual check
  before runtime projection is treated as equivalent to native memory across
  a full session lifecycle.
- **`CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD` is undocumented** in
  `claude --help` and vendor docs. Its behavior was established by direct
  experiment against `v2.1.239` only; cross-version stability is unverified.
- **Only the single-file `CLAUDE.md` + `@import` path is proven.** Whether
  other Claude surfaces (`.claude/` skills, agents, hooks, settings,
  commands) would be discovered the same way from a runtime-projected
  directory via `--add-dir` is **not tested** and must not be assumed —
  proving `CLAUDE.md` import does not constitute proof for `.claude/` in
  general. Any future extension into those surfaces needs its own empirical
  validation per capability before being called Native or Adapted.
- **Windows/WSL is unverified.** The `argv[0]`-dispatch-plus-symlink
  mechanism and the `exec`-based launch are Unix-verified only; a spawn-
  and-wait fallback exists in code (`src/shim.rs`, `#[cfg(not(unix))]`) to
  keep the crate portable to compile, but it has not been exercised or
  proven correct on a non-Unix target.
- **The bridge-vs-projection question is open**, not answered by this ADR
  (see Consequences).

## Future Work

- Close the `/compact` verification gap; if satisfactory alongside the
  other interactive checks already closed, revisit whether runtime
  projection should become the default (or only) Claude context-delivery
  path, with an explicit migration/deprecation decision for the persistent
  bridge — a separate ADR, not an amendment to this one.
- If a future need arises to project `.claude/` subdirectory content (Skills,
  agents, hooks) the same way, treat each as its own empirical question, not
  an extrapolation from this ADR's `CLAUDE.md` proof.
- Extend `IntegrationPort::supports_runtime_integration`/
  `runtime_contribution` to Codex, OpenCode, or Gemini only if and when one
  of them has a real runtime-projection need of its own — nothing here
  requires or anticipates that.
- Once the interactive gate above closes, fold the durable invariants this
  decision establishes (ownership boundary, fail-open guarantee, recursion
  exclusion) into `docs/architecture/invariants.md`, each backed by its own
  test reference, matching how every other invariant in that document is
  recorded.

## Empirical Evidence

All of the following were run against a real, unmodified Claude Code
`v2.1.239`, in isolated `HOME`/`UZE_HOME` environments (the operator's real
`~/.claude`/`~/.uze` were not touched), and are reproducible:

- Canary-token test distinguishing `--add-dir` alone (fails) from
  `--add-dir` + the env var (succeeds), including a `--tools ""` control run
  to rule out the model reading the file on its own.
- `/context`'s own accounting listed the projected content under a distinct
  **"Memory files"** category (2 files, ~101 tokens) rather than as generic
  tool-accessible content — with no pre-existing native `CLAUDE.md`
  anywhere in the test machine's real or isolated `HOME` that could explain
  those files otherwise.
- A full interactive PTY session driven through the shim rendered Claude's
  workspace-trust dialog and main TUI correctly (prompt box, model/effort
  indicator, session percentage, MCP-auth warning) — direct evidence that
  `exec`-based launch preserves raw-mode terminal behavior.
- A deliberately invalid `--model` value produced Claude's own non-zero
  exit code, passed through by the shim unchanged.
- Two concurrent `-p` sessions against the same project both succeeded and
  produced exactly one shared runtime directory, no corruption.
- Editing `AGENTS.md` between two launches changed the answer on the very
  next launch — the idempotent content-compare correctly detected the diff
  and rewrote.
- A project with no `AGENTS.md` produced pure passthrough (no `--add-dir`
  added) — verified with `--tools ""` to make the negative result
  unambiguous.
- `UZE_BYPASS=1` with `--tools ""` reliably reproduced the no-context
  baseline even with `AGENTS.md` present, confirming bypass genuinely
  disables the projection rather than merely hiding it.
- A runtime directory blocked by an unremovable file in its place produced
  the documented `stderr` fail-open note and Claude still launched
  successfully, exit 0.
- Isolated `--version` timing showed the shim's own overhead is
  immeasurable versus a direct launch; the ~220ms added when a project has
  `AGENTS.md` was isolated to Claude Code's own cost of honoring
  `--add-dir` + the env var, reproduced by calling the real binary directly
  with the same flags, shim not involved at all.
