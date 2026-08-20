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
