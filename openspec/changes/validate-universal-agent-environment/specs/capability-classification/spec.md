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

### Requirement: Classify project/harness capabilities with four outcomes
For every relevant (project capability, selected harness) pair, the system
SHALL emit exactly one of `STANDARD`, `NATIVE`, `ADAPTABLE`, or `UNSUPPORTED`
with a non-empty rationale that names the documented evidence or missing safe
equivalence.

`STANDARD` SHALL mean an applicable open standard can be consumed directly
without transformation. `NATIVE` SHALL mean a harness-specific feature can be
used directly as an optional enhancement without changing the portable core.
`ADAPTABLE` SHALL mean a safe, explicit adapter exists. `UNSUPPORTED` SHALL
mean no verified safe equivalence exists, including when evidence is absent;
its rationale SHALL say `unverified` when that is the reason.

#### Scenario: Portable skill on a compatible harness
- **WHEN** a `SKILL.md` Agent Skill is discovered and the selected harness
  documents direct Agent Skills consumption
- **THEN** the result is `STANDARD` with the standard and documented location
  or discovery behavior named in the rationale

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
