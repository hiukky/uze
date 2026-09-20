## MODIFIED Requirements

### Requirement: Plugin records and inspects standard capabilities
The system SHALL record an installed package's capabilities from its stored
bytes and expose them through inspection. Standard capabilities include Agent
Skills, MCP servers, portable Agents, portable Hooks, and optional project
instructions; capability discovery SHALL not require a harness executable.

#### Scenario: A package's standard components are visible in inspection
- **WHEN** the operator inspects a package containing a skill, a portable
  agent, an MCP server, and a valid `hooks.json`
- **THEN** inspection lists each component with its kind and source path
- **AND** Hook compatibility remains a per-harness projection concern
