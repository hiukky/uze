## 1. Vocabulary (single source in uze-core)

- [x] 1.1 Extend the portable alias table in `uze-core::hook` with, per alias, its portable fields and per harness the native tool name and native field names (`shell.command` → `command` / `CommandLine`; `file.*.path` → `file_path` / `TargetFile` / `filePath`; …); capture Antigravity and OpenCode native field names from the schemas the harnesses declare (`--discovery`), not from memory
  - Core owns the alias set and its portable fields (`portable_tool_vocabulary`, `alias_fields`, `hook_field_variable`); the per-harness native tool/field bindings live in `uze-integrations::hooks` (`vocabulary(target)`) because `uze-core` may name no vendor (`core_never_names_a_vendor_harness`). Core owns the shape (`ToolBinding`/`HarnessToolVocabulary`), the integration owns the names — the same split `HookCapabilities` already uses.
- [x] 1.2 Replace the hand-written `tool_name()` matcher translation in `uze-integrations::hooks` with a lookup on the vocabulary; keep `native:<name>` as pass-through
- [x] 1.3 Unit tests: every alias row resolves on every harness; a `native:` matcher yields no portable fields; the table is exhaustive for the aliases `portable_tool_aliases()` lists

## 2. Handler contract in the reference runtime

- [x] 2.1 Change `uze hook-exec` dispatch to the new contract: `HOOK_*` environment (`HOOK_HARNESS`, `HOOK_EVENT`, `HOOK_TOOL`, `HOOK_TOOL_NATIVE`, `HOOK_CWD`, `HOOK_INPUT`, `PLUGIN_ROOT`, alias fields) instead of stdin JSON; exit code decision (`0` allow, `3` deny + stderr reason, other = failure by effect) instead of stdout JSON; sequential order and first-deny-wins unchanged
- [x] 2.2 Update the ABI types/docs in `uze-core::hook` (`HookCommandInput` becomes the environment set; the decision parser becomes an exit-code mapper) and remove the 64 KiB stdout cap that no longer applies
- [x] 2.3 Rewrite handler fixtures to the new contract: `tests/_fixtures/canonical/hook-plugin/scripts/*`, `conformance/_fixtures/marketplace/plugins/hook-plugin` and `hook-order-plugin` scripts, `playground/` example if any
  - `hook-plugin`'s second handler (`mark`) no longer relays a marker: the exit-code contract gives an allowed handler no channel to speak on. First-deny-wins keeps its Lab proof in `hook-order-plugin` (first handler denies, second handler's reason must never appear) and gains a deterministic one in the wrapper's golden tests.
- [x] 2.4 Unit tests for the reference runtime: each harness dialect in → `HOOK_*` set as expected; exit 3 → native deny (exit 2 + JSON on Claude/Codex, `decision: deny` on AGY); handler failure → deny for `deny`/`ask`, proceed for `observe`/`allow`

## 3. Wrapper templates (Claude, Codex, Antigravity)

- [x] 3.1 Add a `hooks/exec` POSIX `sh` template generator in `uze-integrations`: slots for harness id, payload paths (tool name, input, cwd), alias table (native tool → `HOOK_TOOL` + field extraction), native deny/allow rendering and exit code; constants for order, first-deny-wins, per-handler timeout, fail-closed by effect, `jq` guard
- [x] 3.2 Generate `hooks/exec` into the Claude generated plugin and emit native entries in exec form: `command: ${CLAUDE_PLUGIN_ROOT}/hooks/exec`, `args: [<effect>, <handler>…]`, timeout = sum of handler timeouts + 1
  - The wrapper's first argument is the package root: `exec <plugin-root> <event> <effect> <handler>…`. That keeps the file a per-harness constant (byte-identical for every package), which is what makes content-identity inspection a comparison against the template instead of a copy stored in every receipt. Claude's hooks are delivered through `~/.claude/settings.json`, where `${CLAUDE_PLUGIN_ROOT}` does not exist, so the wrapper is named by absolute path — the exec form (`command` + `args`) is used, so nothing is shell-parsed.
- [x] 3.3 Generate `hooks/exec` for Codex under the derived attachment directory and emit the merged `~/.codex/hooks.json` entry as a shell line with absolute paths (no plugin-root variable on Codex)
- [x] 3.4 Generate `hooks/exec` into the Antigravity generated plugin (named entries at the document root; cwd is the `hooks.json` directory; `toolCall.args` field names; `{"decision"}` rendering)
- [x] 3.5 Receipts: the wrapper file and each native entry are receipt-owned; inspect verifies content identity; detach removes them and never a foreign entry; regeneration is idempotent (same bytes for the same package)
- [x] 3.6 Golden tests: generated wrapper + native entry per harness compared byte-for-byte against fixtures; the same handler fixtures exercised through the generated wrapper with each harness's recorded payload (port `.labs/native-hooks/exercise.sh` cases: deny relayed, allow lets the second handler run, denial stops the chain, handler missing/crash/timeout → by effect, `jq` absent → by effect)
- [x] 3.7 Equivalence tests: for every fixture payload, the generated wrapper and `uze hook-exec` produce the same native decision, exit code and stderr

## 4. OpenCode plugin

- [x] 4.1 Regenerate the OpenCode bridge as `hooks-<package>.ts`: the runtime once (spawn handlers with `HOOK_*` environment, exit-code decision, order, first-deny-wins, fail-closed by effect) and the package's groups as data; no packager reference in the file
- [x] 4.2 `observe`/`allow` on `tool.hook("execute.before")` and `execute.after`; `deny`/`ask` on `permission.hook("evaluate")` only if task 6.3 confirms `resources` carries the input — otherwise keep them declared unsupported
  - Kept declared unsupported: task 6.3's measurement is not in yet. `transform` also leaves OpenCode's effect set — the exit-code contract has no channel for a handler to answer a rewrite on, so a `transform` group now degrades on every harness rather than attaching as an observation.
- [x] 4.3 Golden test for the generated plugin and a runtime test under Bun with a fake plugin context (port `.labs/native-hooks/exercise-opencode.ts`)

## 5. Routing, fallback and diagnostics

- [x] 5.1 Compatibility assessment reports the route per hook: `native` (wrapper), `adapted` (fallback `uze hook-exec` with the reason: platform without template, `transform`), `unsupported`
  - `transform` is not one of the fallback's reasons: the exit-code contract has no channel for a handler to answer a rewrite on, so a `transform` group degrades rather than routing anywhere (its own change lifts this).
- [x] 5.2 Fallback route: on platforms without a template, the native entry invokes `uze hook-exec` with the new contract; test that both routes satisfy the same fixtures
- [x] 5.3 `uze doctor`: reports a delivered wrapper whose dependency (`jq`) is missing, and hooks delivered through the fallback
- [x] 5.4 Re-projection: installing/updating a package with hooks replaces the previous `hook-exec` entries (receipt-owned) with the wrapper form; test that a foreign entry beside them is untouched

## 6. Conformance Lab

- [ ] 6.1 Rewrite the hook fixtures' handlers (`guard`, `mark`, `order-1/2`) to the env/exit contract and make the markers assert `HOOK_*` values (`HOOK_TOOL`, `HOOK_COMMAND`, `HOOK_CWD`) in the relayed reason, so the vertical proves the vocabulary row, not only the denial
- [ ] 6.2 Add one Lab case per vocabulary row each harness delivers (`shell` today; `file.write`/`file.read` next) on Claude, Codex, Antigravity (gated: `hooks > vendor` precondition unchanged) and OpenCode
- [ ] 6.3 OpenCode: measure whether `permission.evaluate` carries the shell command in `resources`; if yes, assert `deny`/`ask` and retire the corresponding registry entries; if not, keep the declarations with the observed reason
- [ ] 6.4 Codex: measure whether `ask` has an effect; record the verdict (native or declared) in the registry with the version
- [ ] 6.5 Run all five verticals green; update `conformance/evidence/expected.json` and `DECISIONS.md`

## 7. Documentation and retirement

- [x] 7.1 Rewrite `docs/capabilities/portable-hooks.md` around the new contract: `HOOK_*` table, exit codes, wrapper, per-harness delivery matrix (native / adapted / declared), the `jq` note and fail-closed rule
- [x] 7.2 Amend ADR-033 (or add the superseding ADR) recording the decision: compile at install, wrapper vendored in the artifact, environment + exit-code contract, `hook-exec` as reference/fallback, `transform` deferred
  - A superseding ADR (`docs/adr/040-compile-portable-hooks-into-the-delivered-artifact.md`), not an amendment: ADR-033 is already pushed, and only its ABI and dispatcher change — the rest of it still holds, which the new ADR and a note on 033 both say.
- [x] 7.3 Update `crates/uze-integrations/src/<harness>/README.md` hook sections and the marketplace example plugin (`plugins/`) if it ships a hook
- [ ] 7.4 Retire `.labs/native-hooks` once tasks 3.6 and 4.3 reproduce its exercises in the test suite; keep its README's compatibility matrix in `docs/capabilities/portable-hooks.md`
  - Left unchecked deliberately: `.labs/native-hooks` lives outside this worktree and is git-ignored, so it is not this change's to delete. Its exercises are reproduced by `hooks::wrapper_tests` and `hooks::opencode_runtime_tests`, and its compatibility matrix now lives in `docs/capabilities/portable-hooks.md`, so removing the directory is a one-line follow-up for whoever owns that scratch space.
- [ ] 7.5 Full gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --no-fail-fast`, `openspec validate --all --strict`, `ruff` on `conformance/`
