## Why

UZE has proven transparent attachment for exactly one capability shape so far: a static resource (`SKILL.md`) discovered by each harness's own filesystem scan, closed end-to-end — discovery *and* behavioral — for Claude Code and Codex (`enable-transparent-harness-attachment`, committed). That leaves an open question the North Star depends on: is UZE a general environment composer, or only a Skill installer? MCP is the right next tracer bullet because it is structurally different — a live server process each harness must be told about through its own generated configuration, not something a shared filesystem location can expose. Proving it now, before touching a third (asymmetric) capability, shows the model generalizes without redesign.

## What Changes

- Add a new `ExposureMechanism` variant for **generated vendor config** — the "Runtime Attachment" category ADR-006 already named in documentation but did not implement. Distinct from the existing filesystem-symlink `ManagedUserScopeReference`: no symlink, no shared discovery directory: each integration shells out to the harness's own `mcp add`/`mcp remove`/`mcp get`/`mcp list` commands.
- `UzeStore::install_agent_plugin` also copies a package's `mcp.json` (Agent Plugins 1.0 convention) alongside the existing `skills/` copy.
- `UzeEngine::package_resources` also discovers `mcp.json` into a `Resource` with the already-existing `CapabilityKind::Mcp`.
- `ClaudeIntegration`/`CodexIntegration` gain MCP support: `capabilities()` declares `CapabilityKind::Mcp` adaptable; `exposure_plan()`/`attach()` register a stdio MCP server at each harness's global/user scope once that harness's `uze setup` has completed, falling back to `Unsupported` otherwise (no pre-setup MCP conformance probe existed before this change, unlike Skills — there is nothing to fall back *to*).
- A new, minimal stdio MCP conformance fixture (one tool, deterministic response) bundled alongside the existing Agent Skill fixture package, built with the official `rmcp` Rust SDK as a dev-dependency.
- New opt-in conformance tiers mirroring the Skills precedent: `CONFIGURATION VERIFIED` (config file/CLI shows the entry), `DISCOVERY VERIFIED` (harness reports live stdio connectivity), `BEHAVIORAL VERIFIED` / `BLOCKED_BY_ENVIRONMENT` (real authenticated prompt).
- One new ADR recording the Runtime Attachment decision for MCP.

**BREAKING**: none. `uze add` already calls `integration.attach(resource)` generically for every resource in the composed environment; no CLI surface changes are required for a second capability kind to flow through it.

## Capabilities

### New Capabilities
- `mcp-as-second-capability`: transparent, harness-generated-config attachment of a UZE-store-owned MCP server to Claude Code and Codex, without converting it to or from an Agent Skill, without secrets, and without a `uze sync` step.

### Modified Capabilities
(none — `openspec/specs/` has no capabilities synced from prior changes yet, so there is nothing existing to modify at the spec level)

## Impact

- `src/exposure.rs`: new `ExposureMechanism` variant + its attach/detach-equivalent lifecycle.
- `src/store.rs`: `install_agent_plugin` copies `mcp.json` when present.
- `src/engine.rs`: `package_resources` discovers MCP resources.
- `src/integrations/claude.rs`, `src/integrations/codex.rs`: new MCP exposure strategy.
- `src/integration.rs`: `IntegrationPort::attach` default extended (or left as-is if per-integration overrides suffice — decided in design.md).
- `Cargo.toml`: `rmcp` added as a regular dependency, used only by a new fixture-only `[[bin]]` target (`CARGO_BIN_EXE_<name>` is only defined for real `[[bin]]` targets referenced from integration tests, not for `[[example]]`, and this keeps `cargo test` needing zero extra flags) — the `uze` binary/library source itself never imports it.
- `tests/fixtures/packages/agent-plugin-skill/` (or a new sibling fixture — decided in design.md) gains an `mcp.json` + a small fixture server binary.
- `tests/uze_harness_conformance.rs`: new deterministic and opt-in MCP conformance tests, reusing existing fixture/evidence helpers.
- `docs/adr/007-*.md`, `openspec/changes/enable-mcp-as-second-capability/research-notes.md`: new.
- No change to `src/capability.rs` (kind already exists), `src/router.rs` (classification logic unchanged), `src/main.rs` (CLI already generalizes), `src/home.rs`, `src/state.rs`. No Cursor/Windsurf/new-harness work; OpenCode untouched.
