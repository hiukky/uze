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

### Requirement: Explain every project/harness outcome
For every assessed optional project/harness capability, the report SHALL show
its `STANDARD`, `NATIVE`, `ADAPTABLE`, or `UNSUPPORTED` outcome, rationale,
and any generated artifact or selected adapter. It SHALL distinguish
unverified evidence from a confirmed lack of an equivalent.

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
