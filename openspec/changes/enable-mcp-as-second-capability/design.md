## Context

See proposal.md - Why. `CapabilityKind::Mcp` already exists in
`src/capability.rs` but is only used today by `src/project.rs::discover_mcp`
for project-owned `mcp.json`/`.mcp.json` — no store/engine/integration path
touches it. `ExposureMechanism` (`src/exposure.rs`) has `DirectNative`,
`RuntimeBridge`, `FilesystemProjection`, and `ManagedUserScopeReference`
(added for Agent Skills — a filesystem symlink into a harness's user-scope
skills directory, with generic `attach()`/`detach()` methods reused by both
`ClaudeIntegration` and `CodexIntegration`). `IntegrationPort::attach`'s
default implementation only recognizes `ManagedUserScopeReference`.

Research (see `research-notes.md`) confirmed, for both harnesses, a
non-interactive, scriptable, global/user-scope MCP registration surface:
`claude mcp add --scope user --transport stdio <name> -- <command> [args]`
(writing to `~/.claude.json`'s `mcpServers`) and `codex mcp add <name> --
<command> [args]` (writing to `~/.codex/config.toml`'s `[mcp_servers.*]`,
global-only — no `--scope` flag exists). Both also expose a config-only
`get`/`list` for idempotency checks and a `remove` for cleanup, none of
which require a real model turn.

## Goals / Non-Goals

**Goals:**
- Prove MCP as a second, structurally different capability shape without
  redesigning the resource, package, or router model.
- One new `ExposureMechanism` variant for generated vendor config.
- Store and Engine support for discovering an installed package's
  `mcp.json` (Agent Plugins 1.0 convention) into a `Resource`.
- `ClaudeIntegration`/`CodexIntegration` shell out to each harness's own
  `mcp add`/`get`/`remove`, idempotently, namespaced, secret-free.
- A minimal, deterministic stdio MCP conformance fixture and three
  conformance tiers (configuration / discovery / behavioral), mirroring the
  Agent Skills opt-in test discipline.

**Non-Goals:**
- No secret storage or resolution system. The fixture needs none.
- No `UZE MCP format` — `mcp.json` reuses the Agent Plugins 1.0 shape
  verbatim (a root-level file with an `mcpServers` map, the same shape
  Claude/Codex/Cursor plugins already bundle).
- No multi-server-per-package identity scheme. The fixture declares exactly
  one server; a package's `mcp.json` with more than one server would today
  produce colliding `Resource::identity()` values (same `mcp.json` path for
  each). This is a known, accepted limitation for this tracer bullet, not
  solved here — solving it prematurely for a case the fixture doesn't need
  would be exactly the abstraction this project keeps rejecting.
- No combined Skill+MCP fixture package. Extending the existing
  `tests/fixtures/packages/agent-plugin-skill/` (used by ~10 existing tests
  that assume exactly one resource per package, several picking
  `resources.first()`) would silently change which resource sorts first
  once a second capability kind exists, breaking those tests' assumptions.
  A new, separate `tests/fixtures/packages/agent-plugin-mcp/` avoids this
  entirely. The engine's Skill and MCP discovery are independent functions
  either way — a future combined fixture is a trivial follow-up, not a
  redesign, whenever it's actually needed.
- No `uze remove` CLI command. Removal (`detach`-equivalent) is implemented
  and tested as a direct integration method, not wired to a CLI verb yet —
  same precedent as `ExposureMechanism::detach` for Agent Skills.
- No change to `src/router.rs`'s classification logic.
- No Cursor, Windsurf, or new harness.

## Decisions

### 1. New `ExposureMechanism::ManagedVendorConfig` variant, deliberately non-generic
Shape: `{ entry_name: String, command: PathBuf, args: Vec<String> }`.
Unlike `ManagedUserScopeReference`, this variant carries **no** generic
`attach()`/`detach()` method on `ExposureMechanism` itself — the actual
registration command differs per harness (`claude mcp add --scope user
--transport stdio ... --` vs `codex mcp add ... --`), so there is no shared
filesystem operation to centralize the way there was for a symlink. The
variant exists to (a) describe the plan for `uze inspect` reporting exactly
like every other mechanism, and (b) carry the data (`entry_name`,
`command`, `args`) each integration's own `attach()` override reads to
build its own CLI invocation. `ExposurePlan::prepare()` (the older,
session-scoped RAII helper used by opt-in conformance probes) treats it the
same inert way it treats `ManagedUserScopeReference`: no managed artifact,
because the real attachment path is `attach()`, called once at `uze add`
time, not `prepare()`, called per conformance-probe invocation.

**Alternative considered**: reuse `ManagedUserScopeReference`'s
`discovery_root`/`source` fields, treating the generated config file itself
as the "discovery root" and the entry as a "symlink." Rejected — there is
no symlink, no shared directory scan, and forcing the shape would make
`attach()`'s generic filesystem logic silently wrong for this case (writing
directory entries where a CLI invocation is actually needed).

### 2. `mcp.json` reuses the Agent Plugins 1.0 shape exactly
A package's `mcp.json` sits at the package root (sibling to `plugin.json`
and `skills/`), containing `{ "mcpServers": { "<name>": { "command": "...",
"args": [...] } } }` — identical in shape to what Claude Code and Codex's
own project-level MCP config files already use, and to what their plugin
systems already bundle. `UzeStore::install_agent_plugin` copies it verbatim
when present (optional — a package may have Skills only, MCP only, or
both), mirroring exactly how it already copies `skills/` when present.
`UzeEngine::package_resources` parses it and produces one `Resource` per
declared server, with `capability.payload` set to the re-serialized JSON of
that one server's config object (not the whole file) and `capability.path`
set to the `mcp.json` file's real path — the same byte-preservation
guarantee Skills has does not apply here in the same sense, because MCP
config is structured data assembled from a keyed map, not a flat document
whose bytes go verbatim into a model's context; this is a legitimate
difference in what "the resource" means for this capability kind.

### 3. Entry naming: extend `Resource::attachment_entry_name`, don't invent a parallel scheme
The existing helper (`uze-<package-id>-<parent-dir-name>`) assumes the
capability's `path.parent()` is a meaningfully-named directory — true for
`skills/<skill-name>/SKILL.md`, false for a root-level `mcp.json` (whose
parent is the package root, yielding a redundant `uze-<id>-<id>`). Extended
to branch on `capability.kind`: for `Mcp`, the name is `uze-<package-id>`
(no duplication); for `AgentSkill`, unchanged. This is a small, backward-
compatible extension of an existing core method, not a new naming system.

### 4. Fixture server: a real `[[bin]]` target, not a dev-dependency example
`rmcp` (the official Rust MCP SDK) is added as a regular dependency, used
only by a new fixture-only `[[bin]]` target
(`tests/fixtures/bin/mcp_conformance_fixture.rs` or similar), so integration
tests can locate it reliably via `env!("CARGO_BIN_EXE_<name>")` — the
mechanism Cargo documents specifically for `[[bin]]` targets referenced
from integration tests, with zero extra `cargo test` flags needed.
**Alternative considered**: `[[example]]` + `[dev-dependencies]`, which
would keep `rmcp` fully out of the product's real dependency graph. Rejected
for now — Cargo does not define `CARGO_BIN_EXE_`-equivalent, flag-free path
resolution for examples, and getting that wrong risks flaky test-path
resolution across profiles. The `uze` binary/library source code itself
never imports `rmcp` either way; the trade-off is `rmcp` appearing once in
`Cargo.lock` for a target nothing in the shipped CLI reaches.

### 5. Three conformance tiers, no new `VerificationStatus` variant
`CONFIGURATION VERIFIED`, `DISCOVERY VERIFIED`, and `BEHAVIORAL VERIFIED`
are three separate opt-in test probes, not three new enum variants —
`VerificationStatus` (`Unverified`/`NotExposed`/`Verified`/`Failed`/
`BlockedByEnvironment`) is reused as-is for whichever probe actually ran;
the "tier" is a label in the conformance evidence JSON (mirroring how
`confidence`/`setup_strategy` were added for Skills), not a router-level
concept. This keeps `src/router.rs` untouched, matching the non-goal.

## Risks / Trade-offs

- **[Risk]** Neither harness's `mcp add` overwrite behavior for a
  colliding, differently-configured name was confirmed by research.
  → **Mitigation**: check existence via `get`/`list` before ever calling
  `add`; never call `add` for a name already present. A store package whose
  underlying command/args changed without a name change is a refresh gap
  this change does not solve (same category of limitation already accepted
  for Claude's shim regeneration).
- **[Risk]** `rmcp`'s API surface may shift across versions since MCP
  itself has an unreleased 2026-07-28 spec revision in flight.
  → **Mitigation**: pin the fixture to the current stable spec (2025-11-25)
  behavior only; the fixture's job is minimal determinism, not spec
  coverage.
- **[Trade-off]** No project-scope MCP attachment is offered (only
  global/user scope), matching the Agent Skills precedent exactly and the
  product's own "no per-project config" requirement — intentional, not an
  oversight.
- **[Risk]** `~/.claude.json` is a single file also holding OAuth session
  state and other top-level keys; `claude mcp add`/`remove` is documented to
  merge safely, so UZE never hand-edits this file directly — same posture
  research confirmed for Codex's `config.toml`.

## LikeC4 update

`rmcp` is fixture-only (Decision 4) and introduces no new UZE
container/component — not diagrammed. The one relevant change:
`docs/architecture/likec4/model.c4`'s `uzeStore` component description is
updated to mention it now also preserves `mcp.json` alongside `plugin.json`
and `skills/`, matching what the store code actually does after this
change. No new component, no new relationship.
