## Purpose

Lets UZE select an appropriate runtime integration path and apply optional
harness enhancements explicitly, while keeping portable project resources
native and avoiding a universal harness bridge.

## ADDED Requirements

### Requirement: Select ACP integration in precedence order at the Client ↔ Agent boundary
When a Client ↔ Agent integration is requested, the system SHALL select the
first available supported path in this order: native ACP, official or
demonstrably reliable ACP adapter, minimal explicit integration adapter, then
no integration. It SHALL record the selected path and evidence for its choice.

#### Scenario: Native ACP is available
- **WHEN** the selected client and agent both support compatible ACP versions
- **THEN** the system selects native ACP
- **AND THEN** protocol capabilities are negotiated through ACP initialization
  rather than recreated from harness metadata

#### Scenario: Only a reliable ACP adapter is available
- **WHEN** native ACP is unavailable but a maintained ACP adapter is verified
  for the selected runtime
- **THEN** the system selects that adapter and identifies it in the report
- **AND THEN** it does not add a separate proprietary communication protocol

### Requirement: Keep ACP Proxy and Conductor use explicit and bounded
The system MAY use an ACP Proxy or the official Rust SDK Conductor to compose
an explicit runtime-integration concern. It SHALL identify each proxy concern
and SHALL NOT use a proxy chain to transform AGENTS.md, Agent Skills, MCP
configuration, or non-equivalent harness capabilities without an explicit
adapter outcome.

#### Scenario: Explicit MCP compatibility polyfill in an ACP chain
- **WHEN** an ACP runtime integration needs a documented MCP compatibility
  polyfill for a final agent
- **THEN** the selected proxy chain and its scope are reported explicitly
- **AND THEN** the project portable core is unchanged

### Requirement: Apply optional enhancements only when their outcome is explicit
The system SHALL not write, copy, or synchronize `.claude/`, `.cursor/`,
`.codex/`, or `.opencode/` directories for `STANDARD` items. For a `NATIVE`
or `ADAPTABLE` optional enhancement, it MAY create the minimal documented
artifact only after the selected outcome and artifact path are reported. It
SHALL never create a vendor artifact for an `UNSUPPORTED` enhancement.

#### Scenario: Standard-native skill is already discoverable
- **WHEN** a Skill is classified `STANDARD` for the selected runtime
- **THEN** no harness-specific copy is created
- **AND THEN** the report identifies the standard discovery path or the
  missing documented prerequisite

#### Scenario: Unsupported proprietary hook
- **WHEN** a proprietary hook is classified `UNSUPPORTED` for the selected
  harness
- **THEN** no target hook artifact is created
- **AND THEN** the report explains the unsupported semantics

### Requirement: Keep stored representation separate from exposure
The system SHALL preserve a standard resource's representation independently
from the mechanism by which an integration makes it available. An integration
SHALL choose `DIRECT_NATIVE`, `RUNTIME_BRIDGE`, `FILESYSTEM_PROJECTION`, or
`UNSUPPORTED` explicitly for each applicable resource. The existence of a
`STANDARD` resource in the UZE store SHALL NOT be treated as evidence that a
harness can discover that store path directly.

#### Scenario: Stored Agent Plugin requires a runtime bridge
- **WHEN** an Agent Plugin package is registered in the UZE store and composed
  into an effective environment
- **AND WHEN** a selected harness cannot consume the store location directly
- **THEN** its integration selects an explicit runtime bridge, session-scoped
  filesystem projection, or `UNSUPPORTED`
- **AND THEN** the Agent Skill remains an unchanged standard `SKILL.md`

### Requirement: Keep filesystem fallback explicit, managed, and project-CWD preserving
When an integration selects `FILESYSTEM_PROJECTION`, it SHALL preserve the
caller project as the harness working directory. It MAY create only the
minimal required artifact in that workspace, SHALL record UZE ownership and
lifecycle metadata under `$UZE_HOME/runtime/<integration>/<session>`, and
SHALL remove only the artifact it created. It SHALL NOT copy or virtualize the
caller project into a shadow workspace, and SHALL identify the fallback in the
ExposurePlan.

#### Scenario: Codex receives a stored skill through fallback
- **WHEN** Codex receives a UZE-stored Agent Skill and selects its documented
  `.agents/skills` discovery path as a fallback
- **THEN** the projection is an explicitly UZE-managed temporary artifact for
  the active real project workspace
- **AND THEN** the harness runs with that original project as its CWD
- **AND THEN** cleanup removes the managed artifact and does not remove
  project-owned configuration

### Requirement: Keep normal harness invocation separate from a conformance probe
The system SHALL NOT represent an explicit flag such as Claude Code
`--plugin-dir` as transparent integration. It MAY use that flag only in an
opt-in conformance probe. A normal `claude` or `codex` invocation SHALL be
reported as unproven until a one-time installed/configured integration has been
verified without a UZE wrapper or manual flag.

#### Scenario: Claude receives a package through `--plugin-dir`
- **WHEN** an opt-in conformance probe supplies `--plugin-dir`
- **THEN** the ExposurePlan reports `RUNTIME_BRIDGE`
- **AND THEN** transparent normal Claude invocation remains `UNPROVEN`

### Requirement: Integrate peer harnesses through a contract
The system SHALL expose an integration contract through which a harness
supplies its capability description and receives an effective environment.
The UZE core SHALL NOT name Claude Code, Codex, Cursor, Windsurf, or OpenCode
in its routing rules. Adding a harness that uses existing capability kinds
SHALL require an integration implementation and its tests, not changes to
domain routing rules.

#### Scenario: Claude and Codex assess one Agent Skill
- **WHEN** Claude and Codex integrations receive the same effective
  environment containing one standard Agent Skill
- **THEN** each integration supplies its own capability description to the
  shared router
- **AND THEN** neither integration is treated as a source or destination
