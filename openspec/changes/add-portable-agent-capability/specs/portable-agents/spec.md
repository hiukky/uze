## Purpose

Make package-defined agent profiles portable across UZE's supported harnesses
while preserving canonical source bytes and clearly reporting any adaptation.

## ADDED Requirements

### Requirement: Canonical package agent discovery

The system SHALL recognize every Markdown definition at
`agents/<name>.md` in an installed package as an Agent capability, preserve
the definition's bytes in the Store, and compose it independently from Skill,
MCP, instruction, and hook capabilities.

#### Scenario: Package contains an agent definition

- **WHEN** a package contains `agents/reviewer.md`
- **THEN** inspection reports one Agent capability named `reviewer` with the
  canonical file as its source

#### Scenario: Package has no agent directory

- **WHEN** a package contains no `agents/` directory
- **THEN** its existing capabilities and delivery behavior remain unchanged

### Requirement: Per-harness agent delivery

The system SHALL expose each canonical Agent independently to every supported
harness. Claude Code, OpenCode, Antigravity CLI, and Codex SHALL receive their
documented native agent representation. Codex's representation is a
UZE-generated standalone TOML custom-agent file.

#### Scenario: Native harness receives an agent

- **WHEN** a package with an Agent capability is attached to Claude Code,
  OpenCode, or Antigravity CLI
- **THEN** the harness discovers an equivalent agent through its documented
  native agent surface

#### Scenario: Codex receives an agent

- **WHEN** a package with an Agent capability is attached to Codex
- **THEN** UZE exposes a generated TOML custom-agent file without modifying
  Store bytes and reports a Native route

### Requirement: Agent lifecycle safety

The system SHALL record every UZE-managed agent artifact in a typed receipt,
inspect it before destructive detach, and block removal when the artifact or
receipt has drifted.

#### Scenario: Cleanly detached agent

- **WHEN** an attached Agent artifact still matches its receipt
- **THEN** removing its package removes that artifact and leaves Store bytes
  and unrelated harness artifacts intact

#### Scenario: Drifted agent artifact

- **WHEN** a managed Agent artifact differs from its receipt
- **THEN** removal is blocked without deleting the drifted artifact

### Requirement: Capability support reporting

The system SHALL show an Agents row in the TUI and generated README harness
matrix using the same Native, Adapted, Degraded, Unsupported, and roadmap
semantics used for other capabilities.

#### Scenario: Matrix generation

- **WHEN** the support matrix is generated after agent delivery is available
- **THEN** all four supported harnesses are Native in the Agents column

### Requirement: Real-harness conformance evidence

The conformance lab SHALL verify discoverability and isolation for each
harness's Agent delivery route without Internet access or provider tokens.

#### Scenario: Conformance execution

- **WHEN** the agent conformance scenarios execute for a supported harness
- **THEN** they prove the expected agent is discoverable only from the
UZE-managed fixture and report a nonzero failure on an unexpected route
