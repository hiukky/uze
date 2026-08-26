# Tasks

## Audit (read-only, before production code)

- [x] Produce `docs/architecture/antigravity-compatibility.md`: comparison
      map (detection, provisioning, config paths, workspace
      root, context, skills, commands, MCP, native package model,
      agents/hooks, manifests, lifecycle, list/inspect/remove, generated
      projection, coverage, naming, runtime, receipts, publication),
      classified IDENTICAL / REUSABLE_WITH_PATH_CHANGE /
      REUSABLE_WITH_FORMAT_CHANGE / DIFFERENT / NOT_SUPPORTED / UNKNOWN.
- [x] Verify official docs against real `agy` 1.1.19 in an isolated `$HOME`
      (plugin validate/install/uninstall/list, mcp add/list/remove/disable,
      legacy import, symlink dereference, merge-on-reinstall, JSON shapes).
- [x] Record the module map and reuse classification
      (SHARE_HELPER / KEEP_SEPARATE / MOVE_TO_GENERIC_INTEGRATION_HELPER).

## Antigravity integration

- [x] Implement `AntigravityIntegration` and submodules
      (`provision`, `plugin`, `generate`, `skills`, `commands`, `mcp`).
- [x] Detection: `agy --version` (bare token), resolved past `$UZE_HOME/shims`.
- [x] Provisioning: official installer (invoked exactly as documented;
      post-install verification resolves the documented `~/.local/bin/agy`
      destination) for install, `agy update` for update, via the shared
      `provision_cli`.
- [x] Native package: explicit route when the canonical `plugin.json` is a
      valid vendor manifest and no canonical MCP exists; generated route
      otherwise (MCP translation into `mcp_config.json`, symlinked
      skills/commands).
- [x] Exact coverage (structural + declared) with partial-coverage tests;
      no blanket coverage, no resource disappearance, no duplicate delivery.
- [x] Skills: native via plugin; capability fallback = managed
      `~/.gemini/antigravity-cli/skills` reference with stable namespaced
      label.
- [x] Commands: Adapted via generated SKILL.md (vendor conversion
      semantics); explicit-only property degradation documented and
      declared.
- [x] MCP: `agy mcp add` (global config) fallback; inspection reads
      `~/.gemini/config/mcp_config.json`; drift/conflict/blocked states.
- [x] Lifecycle: attach → inspect MATCHED → detach → Missing; drift blocks
      destructive action; Store bytes unchanged; foreign-name refusal.

## Gemini replacement

- [x] Remove the Gemini CLI integration (module, tests, fixtures, e2e spec,
      composition, bridge entry, docs) — no legacy code path remains.
- [x] Keep historical records intact: ADRs 001–026 untouched; OpenSpec
      specs untouched; the migration audit
      (`docs/architecture/antigravity-compatibility.md`) kept as the
      evidence record.

## Conformance

- [x] Shared suite additions/updates: identity/capabilities, coverage
      full+subset, malformed, precedence, lifecycle+drift, shared-root
      exclusion, shim-boundary, North Star, invocation labels, command
      conformance.
- [x] All tests green (`cargo test --workspace --no-fail-fast`).

## Real dogfood

- [x] Real `agy` 1.1.19 in isolated HOME/UZE_HOME: install flow package →
      `agy plugin list` registration → `uze inspect` MATCHED → remove →
      reinstall MATCHED; generated MCP package → translated mcp_config.json
      → vendor validate OK → MATCHED; tamper → DRIFTED → blocked removal →
      clean uninstall → reinstall.

## Documentation

- [x] README compatibility table (Antigravity as Google-family v0).
- [x] Integration README matrix + `src/antigravity/README.md`.
- [x] `docs/architecture/antigravity-compatibility.md`, ADR-027,
      landscape/context/commands capability docs, AGENTS.md, e2e README.
- [x] OpenSpec change (this one).

## Verification

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo test --workspace --no-fail-fast`
- [x] `cargo llvm-cov` coverage stays above the CI floors
- [x] `openspec validate --all --strict`
- [x] `git diff --check`
