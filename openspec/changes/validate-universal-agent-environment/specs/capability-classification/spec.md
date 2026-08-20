## Purpose

Lets UZE explain an effective agent environment without inventing a second
protocol capability model or pretending that harness-specific features are
portable. ACP-negotiated facts stay protocol facts; only the remaining
project/harness surface is classified.

## ADDED Requirements

### Requirement: Separate protocol capabilities from project/harness capabilities
The system SHALL identify whether a discovered capability belongs to the ACP
Client ↔ Agent protocol surface or to the project/harness surface before
classifying it. When an ACP connection is selected, the system SHALL obtain
protocol capabilities from the ACP initialization negotiation and SHALL NOT
derive, rename, or overwrite those capabilities with a UZE-local matrix.

#### Scenario: ACP session capabilities are negotiated
- **WHEN** a selected client and agent complete ACP initialization
- **THEN** the report records their advertised protocol version and
  capabilities as negotiated protocol facts, including only the capabilities
  actually advertised by the endpoints
- **AND THEN** UZE does not assign `STANDARD`, `NATIVE`, `ADAPTABLE`, or
  `UNSUPPORTED` to those protocol facts

#### Scenario: Non-ACP runtime is selected
- **WHEN** the selected runtime has no ACP integration path
- **THEN** the system reports runtime integration as unavailable or as the
  explicitly selected minimal adapter path
- **AND THEN** it does not fabricate ACP capability results

### Requirement: Separate representation from compatibility route and exposure
For every relevant capability, the system SHALL record its representation
provenance separately from its compatibility route and exposure state.
`STANDARD`, `NATIVE`, `UZE`, and `FOREIGN` are representation/provenance facts;
they SHALL NOT by themselves claim that a harness has discovered or exposed a
capability.

For every relevant (capability, integration-supplied harness capabilities)
pair, the system SHALL emit exactly one compatibility route of `NATIVE`,
`ADAPTABLE`, `DEGRADED`, or `UNSUPPORTED` with a non-empty rationale. It SHALL
also report an exposure or verification state when one is known. `ADAPTABLE`
requires a safe explicit adapter; `DEGRADED` names preserved and missing
semantics; `UNSUPPORTED` includes absent evidence and SHALL say `unverified`
when that is the reason.

#### Scenario: Standard skill has not yet been exposed
- **WHEN** a `SKILL.md` Agent Skill is discovered and the selected harness
  documents direct Agent Skills consumption
- **THEN** the capability representation is `STANDARD`
- **AND THEN** its route and exposure state are reported independently rather
  than inferred from the representation

#### Scenario: Blocking hook lacks an equivalent
- **WHEN** a project enhancement requires a hook that can veto execution and
  the selected harness has no documented blocking equivalent
- **THEN** the result is `UNSUPPORTED`
- **AND THEN** the rationale names the missing blocking behavior rather than
  claiming a post-execution hook is equivalent

### Requirement: Do not classify feature parity by translation alone
The system SHALL NOT classify a capability as `ADAPTABLE` merely because a
file can be generated or copied to a harness-specific directory. It SHALL
require evidence that the adapter preserves the required semantics, and SHALL
report any user-visible behavior it cannot preserve.

#### Scenario: Similar commands with incompatible arguments
- **WHEN** a custom command can be represented by a target command file but
  its required argument semantics are not supported by the target
- **THEN** the result is `UNSUPPORTED`, unless a documented adapter preserves
  the required behavior and its explicit activation is recorded

### Requirement: Route through harness-neutral capability descriptions
The system SHALL route a capability using an integration-supplied harness
capability description. UZE domain rules SHALL NOT branch on named harnesses
or contain a fixed support matrix.

#### Scenario: A peer integration supplies skill support
- **WHEN** two integrations independently describe support for standard Agent
  Skills
- **THEN** the same core capability is routed for each integration without
  converting it through either harness representation
