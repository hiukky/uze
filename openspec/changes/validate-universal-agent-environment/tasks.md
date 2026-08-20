## 1. Confirm architecture decisions

- [x] 1.1 Confirm `docs/adr/001-adopt-open-standards-over-competing-formats.md`
      remains accepted and `docs/adr/002-scope-capability-model-to-standards-gap.md`
      is marked superseded by ADR 003.
- [x] 1.2 Confirm `docs/adr/003-compose-effective-agent-environments-and-use-acp-at-the-client-agent-boundary.md`
      exists and matches the draft in this change.
- [x] 1.3 Confirm `docs/adr/004-implement-the-uze-core-in-rust.md` exists
      and records Rust as the core implementation language without making ACP
      a required runtime dependency.

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

- [x] 3.1 Encode evidence for project/harness discovery and optional
      capabilities for Claude Code, Codex, Cursor, and OpenCode; exclude
      Windsurf/Devin Desktop from the active matrix.
- [x] 3.2 Implement the domain split: ACP-negotiated protocol capabilities
      versus project/harness capabilities.
- [x] 3.3 Implement `STANDARD`, `NATIVE`, `ADAPTABLE`, and `UNSUPPORTED`
      outcomes with mandatory evidence-based rationale; represent missing
      verification as `UNSUPPORTED` with an `unverified` rationale.
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
- [x] 5.4 Resolve and report that project against Claude Code, Codex, Cursor,
      and OpenCode; manually review every fallback and unsupported outcome.
- [x] 5.5 Confirm the PoC never requires per-vendor core configuration,
      never silently synchronizes vendor directories, and clearly identifies
      all standards gaps that remain unresolved.

## 6. Architecture model

- [x] 6.1 Run `bunx likec4@latest validate docs/architecture/likec4` (or the
      project's `arch:validate` script, once one exists) after implementation
      changes to the composition layer, runtime integration, or relationships.
