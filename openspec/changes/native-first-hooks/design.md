## Context

See proposal.md — Why. What the design has to work with, established on 2026-09-02:

- **The harness contracts are known and narrow.** Claude Code, Codex and Antigravity run a *command*: JSON on stdin (`tool_name`/`tool_input`/`cwd` on the first two, `toolCall.{name,args}`/`workspacePaths` on AGY), a decision back on stdout (`hookSpecificOutput.permissionDecision` / `{"decision"}`) and an exit code (only `2` blocks on Claude/Codex; any other non-zero exit is non-blocking and the tool runs; a timeout does not block). None of them passes tool data as arguments or environment; the only substitutions are path placeholders (`${CLAUDE_PLUGIN_ROOT}` on Claude; none on Codex; the `hooks.json` directory as cwd on AGY). Claude runs a group's hooks **in parallel**. OpenCode has no command hooks: a V2 plugin registers `tool.hook("execute.before"/"execute.after")` (input visible, rewritable, cannot block) and `permission.hook("evaluate")` (can set `effect`/`message`; carries `action` and `resources`, not the tool input).
- **The current runtime works and is the spec.** `uze hook-exec` (`src/main.rs`, `crates/uze-integrations/src/hooks.rs` adapters) already normalizes each dialect, runs handlers in order, applies first-deny-wins and fail-closed, and renders the native decision. Its defect is only *where it lives*. (2026-09-12: it was the spec until the wrapper answered every fixture identically; the answers are now recorded goldens and the runtime is removed — see D6.)
- **Invariants that must hold** (docs/architecture/invariants.md, ADR-013/033): Store bytes verbatim; every managed artifact receipt-owned, inspect-before-detach, regenerable from Store + Engine; `uze-core` names no vendor; matcher translation and compatibility computed from one table.
- **The prototype** in `.labs/native-hooks` is the shape to reproduce: `plugin/` (author), `generated/{claude,codex}/hooks/exec` + native `hooks.json`, `generated/opencode/hooks-<plugin>.ts`, exercised by `exercise.sh` and `exercise-opencode.ts` with the payloads each harness really sends.

## Goals / Non-Goals

**Goals:**
- One handler, written once, runs unchanged on every harness that delivers the hook, with no packager process on the execution path.
- Everything harness-specific is decided at generation time and is readable in the delivered artifact.
- The semantics that harnesses do not provide (order, first-deny-wins, fail-closed, dependency guard) are guaranteed by the artifact, once per harness, not per group.
- The vocabulary (alias → fields → native names) has one owner and is proven row by row in the Lab.

**Non-Goals:**
- `transform` under the exit-code contract (needs a stdout convention; own change).
- Declaring/installing system dependencies of handlers or wrappers (plugin `requirements`; own change).
- A PowerShell wrapper (until then Windows delivers no hooks and says so).
- Making Antigravity execute hooks in the offline Lab (vendor account gate; measured, not simulated, until the synthetic world can serve the real setting).
- Changing the canonical `hooks.json` schema. Groups, matchers, effects, handlers and timeouts stay as ADR-033 defined them.

## Decisions

### D1 — Compile the translation; do not interpret it at call time
The adapter logic moves from a Rust process invoked per call to a wrapper generated per plugin per harness (`hooks/exec`, POSIX `sh`), vendored inside the delivered artifact. Alternatives: (a) keep the runtime and vendor a static copy of the `uze` binary in the plugin — self-contained but ~MBs per plugin and still opaque; (b) generate one runner per group — replicates the semantics N times per plugin and bloats the native config. The wrapper is one file per harness per plugin; groups are one native entry each: `hooks/exec <plugin-root> <event> <effect> <seconds>:<handler>…`.

### D2 — The wrapper's interface is arguments, not a spec file
`exec <effect> <handler>…` carries everything a group needs beyond what the harness already decided (the matcher). No sidecar spec, no registry: the native `hooks.json` entry is readable and reproducible on its own. Claude gets the exec form (`command` + `args`, no shell parsing); Codex and AGY get the equivalent shell line.

### D3 — Handler contract is environment in, exit code out
Environment variables are the named parameters that exist in every language the author might pick (`sh`, JS, Python, Go). Exit codes are the decision channel every harness can be mapped to and need no parser in the handler: `0` allow, `3` deny (reason on stderr), other = failure. Alternatives: stdin JSON (today; forces `jq`/a parser into every handler and reproduces the harness's own burden) and positional args (order-dependent, breaks on nested input). Consequence: `transform` is not expressible; deferred rather than bolted on.

### D4 — The vocabulary owns fields and native names
`uze-core` extends `portable_tool_aliases()` into a table `{alias → [portable field], harness → (native tool, native field per portable field)}`. Integrations stop hand-writing `tool_name()` matches; wrappers, OpenCode's `ALIASES`, compatibility and the Lab's per-row cases are generated or checked from it. Native field names for Antigravity and OpenCode are taken from the schemas the harness declares to the model (captured with `--discovery`), never from memory. `native:<name>` bypasses the table: raw input only.

### D5 — Ordering, first-deny-wins, timeout and fail-closed are wrapper constants
Because Claude runs a group's hooks in parallel and every command-hook harness treats a failing hook as non-blocking, these guarantees cannot be delegated. They are compiled into the wrapper once per harness; the group's effect and each handler's own deadline are the only variables (its arguments — `<seconds>:<command>` per handler). The deadline is enforced without `timeout(1)` (macOS ships none) and without job control: a cancellable sleeper, then `TERM` and `KILL` to the handler and every process it started. The `jq` dependency of the `sh` wrapper is guarded by the same rule (a `deny` group denies with a literal that needs no parser).

### D6 — The wrapper is the only route; the reference becomes data
The in-binary runtime (`uze hook-exec`) is removed: one contract, one implementation, no `uze` on any hook's execution path. Where no template applies, the hook is **Unsupported** with that reason and nothing is attached — an entry running a second implementation would be a hook the author did not write. What the Rust adapters were the reference for is recorded instead: the answer the wrapper gives for every fixture (native decision document, exit status, reason) is a golden table per harness, taken from the adapters before they were deleted, so a changed answer is a reviewed diff. Nothing is migrated in place: the next `uze plugin install`/`update` re-projects.

### D7 — OpenCode: the plugin is the wrapper
No command hooks, so the generated `.ts` plugin embeds the runtime once and the package's groups as data. `observe`/`allow` on `execute.before`; `deny`/`ask` on `permission.evaluate` **if** the Lab confirms `resources` carries the shell command (hypothesis from the prototype) — otherwise they stay declared unsupported as today. The file is named after the package (`hooks-<package>.ts`), receipt-owned as the current bridge is.

### D8 — Nothing in the artifact names the packager
`HOOK_*` variables, `hooks/exec`, `hooks-<package>.ts`, comments — no `uze`/`UZE`. The convention must be usable by another packager or by hand; UZE is one compiler for it. This is also what lets the contract become a spec of its own later.

## Risks / Trade-offs

- [`jq` becomes a runtime dependency of the delivered `sh` wrapper] → guarded fail-closed/fail-open by effect; surfaced by `uze doctor`; the vendor's own hook examples already assume `jq`; the `requirements` change makes it declarable and installable.
- [Semantics live in N templates instead of one Rust function] → templates are generated from one source and tested against the Rust adapters with shared fixtures; the Lab proves each alias row on the real harness.
- [Handler failure semantics differ from native hooks (native: failure runs the tool)] → intentional and documented: the wrapper is where a `deny` guard becomes fail-closed; `observe`/`allow` keep the native fail-open.
- [Breaking change for existing handlers written against stdin JSON] → pre-1.0, no compatibility layer (project policy); fixtures, examples and docs rewritten; there is exactly one route, so there is exactly one contract.
- [Antigravity cannot be proven in the Lab] → unchanged from today: gate measured live (`hooks > vendor`), declarations registered per version; the synthetic-world step is separate and evidence-driven.
- [OpenCode deny/ask hypothesis may be false] → the Lab decides before the registry changes; the design does not depend on it.

## Migration Plan

1. Land vocabulary + generators behind the existing routes (no behavior change until projection uses them).
2. Switch projection to the wrapper for Claude, Codex, Antigravity, OpenCode; a platform with no template attaches nothing and reports Unsupported.
3. Rewrite fixtures/handlers to the env/exit contract; update docs and ADR; retire `.labs/native-hooks` once `exercise.sh` is reproduced by generated output in tests.
4. Re-projection happens on the next install/update; no in-place migration of existing native entries (receipts detect the old entry as UZE-owned and replace it).
5. Rollback: the wrapper is generated at install time and every artifact it writes is derived, so reverting is a re-projection (`uze plugin install`/`update`), not a migration.

## Open Questions

- Whether Codex honors `ask` (its JSON accepts it; the effect is undocumented) — decided by a Lab run, affects only the compatibility verdict, not the design.
- Whether OpenCode's `permission.evaluate` `resources[0]` is the shell command — decided by a Lab run (D7).
