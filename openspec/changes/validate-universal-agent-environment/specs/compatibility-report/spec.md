## Purpose

Lets a developer understand the effective agent environment and runtime choice
without learning every harness directory or relying on silent conversions.

## ADDED Requirements

### Requirement: Report portable-core resolution separately from enhancements
The report SHALL list each discovered portable-core item with its standard,
provenance, and resolution outcome separately from each optional harness
enhancement. It SHALL identify the selected runtime and whether an ACP path
was selected.

#### Scenario: Project with portable core and one enhancement
- **WHEN** a project has `AGENTS.md`, an Agent Skill, MCP configuration, and
  one Cursor-specific rule
- **THEN** the report presents the first three as portable-core items and the
  rule as a separate optional enhancement
- **AND THEN** it shows the selected runtime path without representing the
  Cursor rule as portable

### Requirement: Report ACP negotiation without relabeling it
When ACP is selected, the report SHALL show the negotiated protocol version,
advertised protocol capabilities, selected path (native or adapter), and any
explicit proxy/conductor concerns. It SHALL not assign UZE project/harness
classification labels to ACP-negotiated protocol capabilities.

#### Scenario: Session and permission capability advertised through ACP
- **WHEN** ACP initialization advertises session and permission support
- **THEN** the report identifies them as ACP-negotiated protocol capabilities
- **AND THEN** it does not recalculate support from a static harness matrix

### Requirement: Explain representation, route, and exposure independently
For every assessed project/harness capability, the report SHALL show its
representation provenance, compatibility route, exposure or verification
state, rationale, and any selected adapter. It SHALL distinguish unverified
evidence from a confirmed lack of an equivalent and SHALL NOT use a standard
representation as proof of exposure.

#### Scenario: Standard capability is unverified for exposure
- **WHEN** an Agent Skill has a standard representation but no conformance
  test has verified its exposure in an integration
- **THEN** the report identifies the standard representation and an
  `UNVERIFIED` exposure state separately

#### Scenario: Standard representation uses an explicit exposure plan
- **WHEN** a standard Agent Skill is composed from the UZE store
- **THEN** the report keeps `STANDARD` as its representation
- **AND THEN** it identifies the selected `DIRECT_NATIVE`, `RUNTIME_BRIDGE`,
  `FILESYSTEM_PROJECTION`, or `UNSUPPORTED` mechanism separately

#### Scenario: Unsupported unverified enhancement
- **WHEN** no primary documentation verifies a safe equivalent for an
  enhancement
- **THEN** the report marks it `UNSUPPORTED` and includes `unverified` in its
  rationale

### Requirement: Include standards coverage and remaining gaps
The report SHALL include a `Standards Coverage / Remaining Gap` section with
one row for project instructions, reusable capabilities, tools/resources,
Client ↔ Agent, Agent ↔ Agent, project composition, and harness-specific
capabilities. Each row SHALL name the applicable standard if any, its coverage,
and the remaining gap without proposing a new protocol or format.

#### Scenario: Composition has no applicable standard
- **WHEN** rendering the project-composition row
- **THEN** the report names no current standard as the composition authority
- **AND THEN** it identifies resolving and explaining the effective agent
  environment as a remaining gap rather than presenting a UZE format as a
  replacement standard
