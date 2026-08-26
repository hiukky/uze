## Why

The prior change (`validate-universal-agent-environment`) proved that UZE can *compose* an effective environment and that Claude Code and Codex can each *consume* a UZE-managed Agent Skill — but only through explicit, per-invocation bridges: Claude via `--plugin-dir <path>` (a flag the user must remember every session) and Codex via a filesystem projection UZE itself prepares immediately before spawning the process in tests. Neither proves the North Star: **One agent environment. Any harness.** A user must be able to run `uze setup` once, `uze add <package>` once, then use `claude` and `codex` normally — no UZE flags, no `uze claude`/`uze codex` launcher, no `uze sync`, no manual per-project vendor config. This change proves or disproves that boundary now, before any further core/package/capability expansion.

## What Changes

- Add `uze setup` (unified, with `uze setup claude` / `uze setup codex` as the internal per-harness slice): machine-level, idempotent detection and one-time integration install for Claude Code and Codex.
- Add a new `ExposureMechanism` variant for a **persistent, user-scope managed symlink** (distinct from the existing per-session `RuntimeBridge` and per-project `FilesystemProjection`), selected as the primary strategy for both `ClaudeIntegration` and `CodexIntegration` when the harness has been set up.
- `uze add <package>` (already installs into the Store once) now also causes each set-up integration to create/refresh its managed symlink so the package's Agent Skill becomes visible to plain `claude`/`codex` invocations without further action.
- Add minimal integration state persistence under `$UZE_HOME/state/integrations.json` (harness id, detected version, strategy, installed flag, managed artifact paths) — no new store engine, no daemon.
- Add a minimal `uze doctor`: read-only report of UZE_HOME/Store/integration state per harness.
- Add opt-in real-harness tests that explicitly separate **setup** (`uze setup` → integration installed, idempotent) from **runtime** (plain `claude`/`codex` invocation with zero UZE arguments and no test-side `prepare()` call immediately before spawn) — both always against temporary, isolated `$HOME`/`$UZE_HOME`, never the developer's real harness configuration.
- Add one new ADR (006) recording the persistent-symlink decision and its evidence basis; add a `research-notes.md` documenting the official-docs findings for both harnesses.

**BREAKING**: none — this only adds a new exposure mechanism and two new CLI subcommands; existing `IntegrationPort`, `ExposurePlan`, and CLI behavior (`uze add`, `uze inspect`) are unchanged.

## Capabilities

### New Capabilities
- `transparent-harness-attachment`: machine-level `uze setup`/`uze doctor`, the persistent user-scope symlink exposure mechanism, and the setup-vs-runtime transparency contract that `uze add` fulfills for Claude Code and Codex without per-session flags, a launcher, or `uze sync`.

### Modified Capabilities
(none — `openspec/specs/` has no capabilities synced yet from the prior change, so there is nothing existing to modify at the spec level)

## Impact

- `src/exposure.rs`: new `ExposureMechanism` variant + its `prepare`/lifecycle handling.
- `src/integrations/claude.rs`, `src/integrations/codex.rs`: new primary exposure strategy, existing per-session bridge kept as a secondary/fallback path.
- `src/integration.rs`: `IntegrationPort` gains only what the research shows is actually needed for setup/detect/status (no speculative methods).
- `src/home.rs` / `src/store.rs`: minimal state-persistence addition under `$UZE_HOME/state/`.
- `src/main.rs`: new `uze setup` and `uze doctor` subcommands.
- `tests/`: new deterministic tests (fake harness capabilities) for the setup lifecycle, plus new opt-in E2E tests split into setup-phase and runtime-phase suites.
- `docs/adr/006-*.md`, `openspec/changes/enable-transparent-harness-attachment/research-notes.md`: new.
- No change to `src/capability.rs`, `src/router.rs`, `src/engine.rs`, `src/project.rs` domain logic; no Cursor/Windsurf work; OpenCode untouched.
