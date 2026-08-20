## 1. Confirm architecture decisions

- [x] 1.1 Confirm `docs/adr/001-adopt-open-standards-over-competing-formats.md`
      remains accepted and `docs/adr/002-scope-capability-model-to-standards-gap.md`
      is marked superseded by ADR 003.
- [x] 1.2 Confirm `docs/adr/003-compose-effective-agent-environments-and-use-acp-at-the-client-agent-boundary.md`
      exists and matches the draft in this change.
- [x] 1.3 Confirm `docs/adr/004-implement-the-uze-core-in-rust.md` exists
      and records Rust as the core implementation language without making ACP
      a required runtime dependency.
- [x] 1.4 Create ADR 005 recording Claude Code and Codex as the first peer
      harness integrations, and record that Claude plugin import is a foreign
      importer concern rather than canonical UZE representation.

## 2. Portable project composition

- [x] 2.1 Initialize the Rust CLI/library workspace and define project
      discovery for applicable `AGENTS.md`, Agent Skills, and MCP
      configuration while preserving standard-native representations.
- [x] 2.2 Implement the effective-environment resolver: portable core first,
      then separately identified optional harness enhancements.
- [x] 2.3 Implement explicit fallback import for declarative bundles, including
      malformed-input and path-traversal rejection with no partial result.
- [x] 2.4 Verify byte-for-byte preservation for fallback-imported standard
      Skill and MCP payloads.

## 3. Capability assessment

- [x] 3.1 Keep named-harness evidence outside UZE domain rules; let the first
      peer Claude Code and Codex integrations supply only the declarations
      needed for the Agent Skill path.
- [x] 3.2 Implement the domain split: ACP-negotiated protocol capabilities
      versus project/harness capabilities.
- [x] 3.3 Separate representation provenance from `NATIVE`, `ADAPTABLE`,
      `DEGRADED`, and `UNSUPPORTED` route outcomes, and represent exposure as
      unverified until a real-harness conformance test exists.
- [x] 3.4 Verify that incompatible hook, permission, command, and subagent
      semantics cannot be promoted by directory generation alone.

## 4. Runtime integration and progressive enhancement

- [x] 4.1 Implement runtime path selection in order: native ACP, reliable ACP
      adapter, minimal explicit adapter, then no integration.
- [ ] 4.2 When ACP is selected, consume the initialization handshake and
      report protocol version/capabilities without duplicating discovery.
- [x] 4.3 Evaluate the official Rust ACP SDK's Proxy and Conductor only for a
      concrete, explicit Client ↔ Agent integration concern; do not make Rust
      or a proxy chain a mandatory dependency.
- [ ] 4.4 Implement optional enhancement application only after reporting the
      selected `NATIVE` or `ADAPTABLE` outcome and target artifact path.
- [x] 4.5 Verify `STANDARD` items are not copied into vendor directories and
      `UNSUPPORTED` items create no vendor artifact.

## 5. Compatibility report and PoC validation

- [x] 5.1 Implement the report with portable-core resolution, runtime path,
      ACP-negotiated protocol facts, optional-enhancement outcomes, and
      `Standards Coverage / Remaining Gap`.
- [x] 5.2 Verify report reproducibility for unchanged project and evidence
      inputs.
- [x] 5.3 Build or select one representative project containing AGENTS.md,
      an Agent Skill, MCP configuration, and one or more optional
      harness-specific enhancements.
- [x] 5.4 Resolve and report that project through peer Claude Code and Codex
      integrations; keep verification evidence-specific pending opt-in
      real-harness conformance tests rather than inferring it from discovery.
- [x] 5.5 Confirm the PoC never requires per-vendor core configuration,
      never silently synchronizes vendor directories, and clearly identifies
      all standards gaps that remain unresolved.

## 6. Architecture model

- [x] 6.1 Run `bunx likec4@latest validate docs/architecture/likec4` (or the
      project's `arch:validate` script, once one exists) after implementation
      changes to the composition layer, runtime integration, or relationships.

## 7. Peer-integration boundary correction

- [x] 7.1 Extract a harness-agnostic core model for project resources,
      effective environment, capability provenance, compatibility routing, and
      exposure/verification.
- [x] 7.2 Replace the fixed `Harness` enum and support matrix with an
      integration-supplied harness capability description.
- [x] 7.3 Move foreign Claude plugin recognition to a specialized importer;
      preserve the explicit compatibility-import CLI behavior.
- [x] 7.4 Make the project resolver discover standard project resources only;
      do not scan named vendor directories in the core.
- [x] 7.5 Add peer Claude, Codex, and OpenCode integration implementations
      for the standard Agent Skill validation path without a source/destination
      flow.
- [x] 7.6 Add router unit tests and integration-contract tests using fake
      harness capabilities; keep them independent of real executables.
- [x] 7.7 Preserve and update CLI tests to confirm inspection remains
      read-only and standard resources are not copied.
- [x] 7.8 Update LikeC4 to show a harness-agnostic UZE Core and peer Claude /
      Codex integrations; validate the model.
- [x] 7.9 Run Rust, OpenSpec, and LikeC4 validation and document which claims
      remain unverified until opt-in real-harness conformance tests exist.

## 8. Real-harness conformance

- [x] 8.1 Separate native-discovery fixtures from the Agent Plugin package
      fixture. Native probes contain `.agents/skills` only to measure a
      harness; UZE integration probes do not.
- [x] 8.2 Introduce `UZE_HOME` with one path authority for store, state,
      cache, and runtime; use an explicit temporary home in deterministic
      tests.
- [x] 8.3 Install/register the Agent Plugin once in the UZE store and compose
      its standard skill into an effective environment without a UZE manifest.
- [x] 8.4 Add `ExposurePlan`, keeping representation separate from concrete
      direct, runtime-bridge, filesystem-fallback, and unsupported mechanisms.
- [x] 8.5 Add opt-in UZE integration conformance through
      package → store → engine → environment → integration → Codex. Codex is
      verified through an explicit UZE-managed temporary projection in the real
      project CWD; cleanup returns the caller workspace to clean state.
- [x] 8.6 Run the equivalent Claude Code UZE integration probe with the
      economical `haiku` model. The proof token verified the Store → Engine →
      Integration path; this is not transparent normal invocation evidence.
- [x] 8.7 Run the separate OpenCode native and UZE integration probes with
      `opencode/deepseek-v4-flash-free`; both returned the proof token.

## 9. Composition and transparent-integration boundary correction

- [x] 9.1 Make the UZE Store authoritative only for UZE-installed packages;
      compose those with project-owned standard resources in one
      `EffectiveEnvironment`.
- [x] 9.2 Connect `UzeHome::from_env()` to a minimal real CLI flow: `uze add`
      installs a local Agent Plugin and `uze inspect` composes registered
      packages with the selected project.
- [x] 9.3 Refine `ExposurePlan` so strategy and verification are separate;
      report `VERIFIED`, `NOT_EXPOSED`, `UNVERIFIED`, `FAILED`, and
      `BLOCKED_BY_ENVIRONMENT` without treating environmental blocks as
      incompatibility.
- [x] 9.4 Replace the Codex/OpenCode shadow workspace with a minimal managed
      artifact in the real caller workspace, preserve the process CWD, and
      provide explicit/RAII cleanup plus runtime ownership metadata.
- [x] 9.5 Add deterministic same-store peer contracts and an opt-in shared
      Claude/Codex probe that records one home, PackageId, package path, skill
      path, and resource identity.
- [x] 9.6 Keep native conformance separate, move Claude Plugin recognition
      behind an importer module boundary, and document that normal invocation
      remains an unproven product requirement rather than a launcher feature.
- [x] 9.7 Configure inexpensive, overridable real-harness probe defaults:
      Claude `haiku`, Codex `gpt-5.6-luna`, and OpenCode
      `opencode/deepseek-v4-flash-free`.
