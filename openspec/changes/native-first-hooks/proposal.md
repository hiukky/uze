## Why

A UZE-delivered hook only runs while the `uze` binary is installed at the path baked into the harness's configuration: every native hook entry is a 300-character `uze hook-exec …` command line, so uninstalling or moving UZE turns a `deny` guard into a blocked tool (fail-closed) and a plain hook into a silent failure. The translation between the harness's hook dialect and the author's script happens at every call, in a process the plugin does not own. Meanwhile the investigation of 2026-09-02 established the real per-harness contracts (Claude/Codex/Antigravity: stdin JSON in, decision JSON + exit code out; OpenCode: a V2 plugin) and prototyped the alternative in a local, uncommitted lab (`.labs/native-hooks`, ignored by git): compile the translation at install time into a small, packager-neutral wrapper vendored inside the delivered plugin — `exercise.sh` 7/7 on Claude and Codex payloads, `exercise-opencode.ts` 3/3 — with no `uze` on the execution path.

## What Changes

- **Native-first delivery.** A package's canonical `hooks.json` is compiled, at install time, into each harness's own hook form plus a vendored wrapper (`hooks/exec`, POSIX `sh`) inside the delivered artifact. The native entry is `hooks/exec <effect> <handler>…`; the wrapper reads the harness payload from stdin, exposes the hook context as environment, runs the handlers in manifest order, and answers in the harness's dialect. OpenCode, which has no command hooks, receives a generated V2 plugin that is the same runtime with the package's groups as data. Nothing in the delivered artifact names UZE.
- **BREAKING — handler contract.** Command handlers no longer receive the normalized JSON on stdin nor answer with JSON on stdout. They receive the context as `HOOK_*` environment variables (`HOOK_HARNESS`, `HOOK_EVENT`, `HOOK_TOOL`, `HOOK_TOOL_NATIVE`, `HOOK_CWD`, `HOOK_INPUT`, and the portable fields of the matched alias such as `HOOK_COMMAND`/`HOOK_PATH`) and answer with an exit code: `0` allow, `3` deny with the reason on stderr; any other exit is a handler failure that follows the group's effect (`deny`/`ask` fail-closed, `observe`/`allow` fail-open). The stdin JSON ABI of `add-portable-hooks` is superseded; `transform` (input replacement) is not expressible in this contract and is deferred to its own change.
- **Portable tool vocabulary with fields.** The alias table (`shell`, `file.read`, `file.write`, `file.edit`, `search.files`, `search.web`, …) gains, per alias, the portable fields it guarantees and each harness's native tool and field names. It becomes the single source of truth in `uze-core`; integrations and the generated wrappers are derived from it. Tools matched through `native:<name>` receive only `HOOK_TOOL_NATIVE` and `HOOK_INPUT`.
- **Ordering and safety live in the wrapper.** Harnesses run a group's hooks in parallel (Claude) or leave failure non-blocking (all three command-hook harnesses: a non-blocking exit runs the tool). The wrapper is where manifest order, first-deny-wins, per-handler timeout and fail-closed are enforced — as compiled constants, identical for every plugin on a harness. Missing wrapper dependencies (`jq`) are handled by the same rule: a `deny` group denies with the reason, never opens.
- **`uze hook-exec` becomes the reference and the fallback.** The Rust adapters stay as the executable specification the wrapper templates are generated from and tested against, and remain the delivery route where no template applies (Windows until a PowerShell wrapper exists; `transform`). Existing installs are re-projected on `uze update`/`uze plugin install`; nothing is migrated in place.
- **Conformance proves the vocabulary.** Every alias row is a Lab case per harness: the real tool call, the handler asserting the expected `HOOK_*` values, the native decision relayed. Antigravity's hook execution stays gated by the vendor's server-side account setting and is measured live (`hooks > vendor`) until the synthetic world learns to serve that setting — a separate, evidence-driven step.

Out of scope, each its own change: plugin `requirements` (declaring and installing system dependencies such as `jq`), the `transform` effect under the exit-code contract, and a PowerShell wrapper.

## Capabilities

### New Capabilities

<!-- none -->

### Modified Capabilities

- `portable-hooks`: the command-handler contract changes from stdin/stdout JSON to environment + exit code; delivery must be self-contained (no packager binary on the execution path); the portable alias vocabulary must define per-alias fields; ordering, first-deny-wins and fail-closed are guaranteed by the delivered artifact regardless of how the harness runs its hooks. (The capability's requirements were introduced by the in-progress `add-portable-hooks` change; this change adds requirements that supersede its ABI requirement, recorded in the design.)

## Impact

- `crates/uze-core`: alias vocabulary gains fields and native names (single source); hook IR unchanged.
- `crates/uze-integrations`: `hooks.rs` adapters become template generators (Claude, Codex, Antigravity `sh` wrapper; OpenCode `.ts` plugin); `hook-exec` adapters retained as reference/fallback; matcher translation reads the vocabulary.
- `src/main.rs` (`hook-exec`): unchanged surface, demoted to fallback.
- Generated artifacts: `hooks/exec` inside generated plugins (Claude, Antigravity), a wrapper path in the merged `~/.codex/hooks.json` entry, `hooks-<package>.ts` for OpenCode; receipts cover the new files.
- Fixtures and docs: `tests/_fixtures/canonical/hook-plugin`, `conformance/_fixtures/marketplace/plugins/hook-*` handlers rewritten to the env/exit contract; `docs/capabilities/portable-hooks.md`; ADR-033 amended (or superseded by a new ADR); `.labs/native-hooks` retired once the generator reproduces it.
- Conformance Lab: hook phases assert `HOOK_*` delivery per alias row; the OpenCode `permission.evaluate` hypothesis (deny/ask) verified; registry entries updated accordingly.
