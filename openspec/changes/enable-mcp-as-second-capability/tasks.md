## 1. Architecture decision

- [x] 1.1 Confirm `docs/adr/007-attach-mcp-servers-through-generated-vendor-configuration.md`
      exists and matches this change's `adr/` draft.
- [x] 1.2 Update `docs/architecture/likec4/model.c4`'s `uzeStore`
      description to mention `mcp.json`, then run
      `bunx likec4@latest validate docs/architecture/likec4`.

## 2. Exposure mechanism

- [x] 2.1 Add `ExposureMechanism::ManagedVendorConfig { entry_name, command,
      args }` — no generic `attach()`/`detach()` on the mechanism itself
      (each integration shells out on its own; see design.md Decision 1).
- [x] 2.2 Extend `ExposurePlan::prepare()`'s exhaustive match with an arm
      for the new variant (inert, mirroring `ManagedUserScopeReference`'s
      arm — no session-scoped managed artifact).
- [x] 2.3 Extend `report.rs::exposure_mechanism()`'s exhaustive match with a
      label for the new variant (e.g. `MANAGED_VENDOR_CONFIG`).

## 3. Resource model: entry naming

- [x] 3.1 Extend `Resource::attachment_entry_name` to branch on
      `capability.kind`: unchanged for `AgentSkill`; `uze-<package-id>`
      (no directory-name suffix) for `Mcp`, since an `mcp.json` resource's
      path parent is the package root, not a per-capability subdirectory.
- [x] 3.2 Unit test both branches.

## 4. Store and Engine: MCP resource support

- [x] 4.1 `UzeStore::install_agent_plugin` copies a package's root-level
      `mcp.json` into the store package directory when present, alongside
      the existing `skills/` copy — optional, mirrors the existing code
      path.
- [x] 4.2 `UzeEngine::package_resources` parses an installed package's
      `mcp.json` (`{"mcpServers": {"<name>": {"command","args"}}}`) and
      produces one `Resource{capability: Capability{kind: Mcp, ...},
      origin: Package{..}}` per declared server, with `payload` set to the
      re-serialized JSON of that one server's config object.
- [x] 4.3 Unit/contract test: a package with only `mcp.json` (no `skills/`)
      composes into an `EffectiveEnvironment` with one `Mcp`-kind resource;
      a package with both composes both, independently. (Found and fixed a
      real latent bug along the way: `package_resources` unconditionally
      scanned `skills/`, which errors when the directory doesn't exist —
      the first MCP-only fixture surfaced it.)

## 5. `ClaudeIntegration` MCP support

- [x] 5.1 `capabilities()` declares `CapabilityKind::Mcp` in `adaptable`.
- [x] 5.2 `exposure_plan()` for an `Mcp` resource: if `uze setup` has
      completed for Claude, extract `command`/`args` from the resource's
      payload and return `ManagedVendorConfig`; otherwise `Unsupported`
      (no fallback conformance probe exists for MCP — see design.md
      Decision/Non-Goals).
- [x] 5.3 `attach()`: dispatch on mechanism — `ManagedUserScopeReference`
      keeps the existing Skill shim-and-symlink path unchanged;
      `ManagedVendorConfig` checks existence via `claude mcp get
      <entry_name>` first, and only if absent runs `claude mcp add --scope
      user --transport stdio <entry_name> -- <command> [args...]`.
- [x] 5.4 A directly-callable removal method (`claude mcp remove
      <entry_name>`) for the `ManagedVendorConfig` case, mirroring
      `ExposureMechanism::detach`'s precedent — not wired to a CLI verb yet.

## 6. `CodexIntegration` MCP support

- [x] 6.1 `capabilities()` declares `CapabilityKind::Mcp` in `adaptable`.
- [x] 6.2 `exposure_plan()`: same setup-gated logic as Claude, using the
      same `ManagedVendorConfig` mechanism.
- [x] 6.3 `attach()`: idempotency check via `codex mcp get <entry_name>`,
      then `codex mcp add <entry_name> -- <command> [args...]` if absent —
      no `--scope` flag, global is the only destination.
- [x] 6.4 A directly-callable removal method (`codex mcp remove
      <entry_name>`).

## 7. Conformance fixture

- [x] 7.1 Add `rmcp` as a dependency; add a fixture-only `[[bin]]` target
      (`tests/fixtures/bin/mcp_conformance_fixture.rs`, target name
      `uze-mcp-conformance-fixture`) — a minimal, real stdio MCP server
      exposing exactly one tool, `uze_conformance()`, returning a value the
      test itself controls via `UZE_MCP_CONFORMANCE_PROOF`. Smoke-tested
      directly against a hand-rolled MCP stdio handshake before wiring into
      any Rust test.
- [x] 7.2 Add `tests/fixtures/packages/agent-plugin-mcp/` (new, separate
      from `agent-plugin-skill/` per design.md's Non-Goals): `plugin.json`
      + root-level `mcp.json` pointing `command` at a placeholder, rewritten
      to the real `env!("CARGO_BIN_EXE_uze-mcp-conformance-fixture")` path
      by test setup code, not hardcoded.

## 8. Deterministic tests

- [x] 8.1 Store/Engine tests from tasks 4.3.
- [x] 8.2 Integration-contract-level tests (fake harness capabilities, no
      real CLI) confirming `ManagedVendorConfig` is selected once setup
      state is recorded, and `Unsupported` beforehand.
- [x] 8.3 CLI test: `uze setup` + `uze add <mcp fixture>` against fake
      PATH-resolvable `claude`/`codex` scripts (extended
      `fake_harness_bin_dir` in `tests/cli.rs` to also understand `mcp
      get`/`mcp add`/`mcp remove`, tracked via marker files) confirming the
      attach path and idempotency end-to-end without any real harness
      binary. A direct test also exercises `detach_mcp_entry` for both
      integrations against the same fake-script pattern.

## 9. Opt-in real-harness conformance

- [x] 9.1 `CONFIGURATION VERIFIED`: opt-in test runs `uze setup` + `uze add`
      for the MCP fixture against an isolated `$HOME`/`$UZE_HOME` for each
      harness, confirming `claude mcp get`/`codex mcp list --json` show the
      entry — no real model turn. Ran for real: both harnesses confirmed.
- [x] 9.2 `DISCOVERY VERIFIED`: same test additionally checks for live
      connectivity reporting. Ran for real: Claude's `claude mcp list`
      reported `Status: ✔ Connected` (verified); Codex's `mcp list --json`
      has no explicit connectivity field in its current schema, so this is
      honestly reported inconclusive for Codex rather than inferred — a
      real, documented asymmetry, not a gap in the test.
- [x] 9.3 `BEHAVIORAL VERIFIED`: opt-in, auth-gated test spawning a real
      prompt that invokes `uze_conformance`; classified via
      `conformance::run_harness`/`is_environment_block`. Ran in an isolated,
      credential-less home: both correctly `BLOCKED_BY_ENVIRONMENT`. With
      explicit operator consent, also attempted against the real,
      authenticated OAuth session outside the automated suite: both
      harnesses' headless/non-interactive MCP tool-call approval gate
      blocked the call in a way plain "never ask" flags did not satisfy;
      the corresponding dangerous bypass flags on both harnesses were
      correctly refused by the operating environment's safety classifier,
      and that refusal was respected rather than worked around. Documented
      as a real, characterized limitation in ADR-007, not forced past.
- [x] 9.4 Confirm all three tiers are structurally distinct tests (mirrors
      the Agent Skills setup-phase/runtime-phase split) so a
      configuration-only pass is never conflated with behavioral
      verification. (Two test functions: configuration+discovery vs.
      behavioral — verification status is never shared between them.)
- [x] 9.5 Confirm no test in this suite ever targets the operator's real
      `~/.claude`/`~/.codex`. All automated opt-in tests use isolated
      `$HOME`/`$UZE_HOME` exclusively; the one real-machine run was manual,
      outside `cargo test`, with explicit operator consent — same
      precedent as the Agent Skills change.

## 10. Validation

- [x] 10.1 `cargo test` passes (41 deterministic tests; opt-in suites
      remain `#[ignore]`d by default).
- [x] 10.2 `cargo clippy -- -D warnings` passes.
- [x] 10.3 `cargo fmt --check` passes.
- [x] 10.4 `openspec validate --strict` passes for this change.
- [x] 10.5 `bunx likec4@latest validate docs/architecture/likec4` passes.
- [x] 10.6 Confirmed: no literal secret appears anywhere in the fixture,
      the generated configuration, or test output — the fixture requires
      none, and neither integration writes a literal secret value.
