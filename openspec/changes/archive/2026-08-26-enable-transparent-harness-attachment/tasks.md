## 1. Architecture decision

- [x] 1.1 Confirm `docs/adr/006-attach-uze-packages-through-persistent-user-scope-skill-references.md`
      exists and matches this change's `adr/` draft.
- [x] 1.2 Update `docs/architecture/likec4/model.c4` for the new
      `Integration State` component and the setup/attachment relationships,
      then run `bunx likec4@latest validate docs/architecture/likec4` before
      considering the change done.

## 2. Exposure mechanism

- [x] 2.1 Add `ExposureMechanism::ManagedUserScopeReference` (or equivalent
      name) with `discovery_root`, `entry_name`, `source` fields, distinct
      from the existing session-scoped `RuntimeBridge`/`FilesystemProjection`
      variants.
- [x] 2.2 Implement its `prepare`/create-or-refresh lifecycle: idempotent
      symlink creation, no error on "already correct," namespaced entry
      name so unrelated pre-existing entries in the harness's discovery root
      are never touched.
- [x] 2.3 Implement removal (on package removal / integration uninstall):
      remove only the UZE-owned symlink, never the discovery root itself or
      any entry UZE did not create.
- [x] 2.4 Unit-test creation, idempotent refresh, and removal against a
      fake discovery root — no real harness involved.

## 3. Integration state

- [x] 3.1 Add minimal per-harness integration state persistence under
      `$UZE_HOME/state/` (harness id, detected version, strategy, installed
      flag, managed artifact paths). Reuse `UzeHome`'s existing state-path
      conventions; no harness secrets are ever recorded.
- [x] 3.2 Unit-test read/write/idempotent-update of this state against a
      temporary `UzeHome`.

## 4. `IntegrationPort` setup surface

- [x] 4.1 Add `detect()` and `install()` to `IntegrationPort`, derived only
      from what `uze setup` needs (harness presence/version; idempotent
      one-time setup ensuring the discovery root exists and state is
      recorded). Do not add speculative methods.
- [x] 4.2 Add `status()` sufficient for `uze doctor` reporting
      (installed/unverified, installed/verified, not configured).
- [x] 4.3 Implement `detect`/`install`/`status` for `ClaudeIntegration`
      (`~/.claude/skills/`) and `CodexIntegration` (`~/.agents/skills/`).
- [x] 4.4 Update `ClaudeIntegration::exposure_plan` and
      `CodexIntegration::exposure_plan` to prefer
      `ManagedUserScopeReference` when that integration's setup has
      completed, falling back to the existing `RuntimeBridge`/
      `FilesystemProjection` conformance mechanism otherwise. Existing
      opt-in conformance tests for the fallback mechanisms must keep
      passing unmodified.
- [x] 4.5 Router/integration-contract unit tests using fake harness
      capabilities confirm the preference order without any real
      executable.

## 5. CLI: `uze setup` and `uze doctor`

- [x] 5.1 Add `uze setup` (running detect+install for every known
      integration) with `uze setup claude` / `uze setup codex` as the
      internal per-harness slice.
- [x] 5.2 Add `uze doctor`: read-only report of `UZE_HOME`/Store readiness
      and per-harness integration status, without printing credential
      material.
- [x] 5.3 Wire `uze add <package>` to also refresh the managed attachment
      for every harness whose setup has completed, in the same command
      invocation — no new `uze sync` command is introduced.
- [x] 5.4 CLI tests confirm `uze setup` run twice produces equivalent state
      (no duplication) and that `uze add` alone (after setup) is sufficient
      to produce the attachment, using a temporary `UZE_HOME` and a fake or
      temp-directory discovery root — no real harness required.

## 6. Deterministic setup-lifecycle tests

- [x] 6.1 Add fake-harness/fake-integration test surfaces sufficient to
      exercise the full `uze setup` → `uze add` → attachment lifecycle
      without any real `claude`/`codex` binary, per the project's existing
      TDD pattern (core, setup planner, integration contract, managed
      artifacts all testable in isolation).

## 7. Opt-in real-harness conformance: setup phase

- [x] 7.1 Add an opt-in test that runs `uze setup` (and `uze add` for the
      shared fixture package) against a temporary, isolated `$HOME` and
      `$UZE_HOME` for Claude Code, and asserts the expected on-disk state
      (`~/.claude/skills/<entry>` symlink, `.claude-plugin/plugin.json`,
      `SKILL.md`) without spawning `claude` itself. Confirm idempotency by
      running setup a second time.
- [x] 7.2 Add the equivalent opt-in setup-phase test for Codex
      (`~/.agents/skills/<entry>` symlink) against the same style of
      isolated home.
- [x] 7.3 Confirm neither setup-phase test ever targets the operator's real
      `~/.claude`, `~/.codex`, or `~/.agents`.

## 8. Opt-in real-harness conformance: runtime phase

- [x] 8.1 Add an opt-in runtime-phase test for Claude Code: after the
      setup-phase state exists (from task 7.1, or recreated fresh in the
      same isolated home), spawn plain `claude -p ...` with the proof-token
      prompt, zero UZE-specific arguments, and no test-authored preparation
      call immediately before the spawn. Classify the result `VERIFIED`,
      `NOT_EXPOSED`, or `BLOCKED_BY_ENVIRONMENT` using the existing
      `conformance::run_harness` classifier. Document in the test and its
      evidence output that authentication may be required and unavailable,
      and that a `BLOCKED_BY_ENVIRONMENT` result leaves the question
      inconclusive rather than failing the suite.
- [x] 8.2 Add the equivalent opt-in runtime-phase test for Codex: plain
      `codex exec ...` with zero UZE-specific arguments.
- [x] 8.3 Confirm the runtime-phase tests are structurally distinct from
      the setup-phase tests (separate `#[ignore]`d test functions/env-var
      gates) so a setup-only pass can never be reported as runtime-verified.

## 9. Evidence and reporting

- [x] 9.1 Extend the conformance evidence emitted by opt-in tests (already
      JSON via `emit_evidence`) to include: harness, detected version,
      setup strategy, invocation command, cwd, package id, resource id,
      exposure mechanism, and verification — enough to audit how a result
      was obtained, without inventing a new persistent struct beyond what
      the tests need.
- [x] 9.2 Update `research-notes.md` in this change with the official-docs
      findings and the empirical symlink-validation evidence already
      gathered (Claude `plugin validate`/`list`/`details` control
      comparison; Codex USER-scope symlink documentation).

## 10. Validation

- [x] 10.1 `cargo test` passes (deterministic suite; opt-in suites remain
      `#[ignore]`d by default).
- [x] 10.2 `cargo clippy -- -D warnings` passes.
- [x] 10.3 `cargo fmt --check` passes.
- [x] 10.4 `openspec validate --strict` passes for this change.
- [x] 10.5 `bunx likec4@latest validate docs/architecture/likec4` passes.
- [x] 10.6 If run with real harness credentials available, execute the
      opt-in setup-phase and runtime-phase suites for Claude Code and Codex
      against isolated homes and record the resulting verification status
      (including any `BLOCKED_BY_ENVIRONMENT` outcome) in
      `docs/adr/006-*.md`'s "More Information" log, matching the pattern
      used in `docs/adr/005-*.md`.
