## Purpose

Lets UZE discover a project-owned portable core and resolve its effective
agent environment without converting standards into a UZE-specific format.
Declarative bundle import remains a compatibility fallback, not the canonical
project model.

## ADDED Requirements

### Requirement: Discover the portable project core directly
Given a project root, the system SHALL discover applicable `AGENTS.md` files,
Agent Skills, and MCP configuration using documented project/runtime discovery
rules. It SHALL retain each standard-native item in its original
representation or reference and SHALL NOT re-serialize it into a proprietary
UZE equivalent.

#### Scenario: Project with instructions, skills, and MCP
- **WHEN** a project contains an applicable `AGENTS.md`, one valid Agent
  Skill, and an MCP configuration
- **THEN** the resolved portable core identifies all three items with their
  provenance and standard type
- **AND THEN** their canonical contents remain the project-owned standard
  representations

### Requirement: Resolve project resources into an extensible effective environment
The system SHALL resolve project-owned standard resources into an effective
environment before any optional enhancement is considered. The initial
environment SHALL contain project resources and SHALL permit future global,
user, package, and runtime layers without making them part of this increment.
An optional enhancement SHALL NOT alter portable resource contents.

#### Scenario: Project has portable resources
- **WHEN** the project contains applicable instructions, a Skill, and MCP
  configuration
- **THEN** they are resolved as project resources in the effective environment
- **AND THEN** no vendor directory is required to form the portable core

### Requirement: Import declarative bundles only as an explicit compatibility fallback
The system MAY parse a declarative plugin bundle only after the caller selects
that import path. It SHALL preserve standard-native contents byte-for-byte,
tag non-standard contents as optional enhancements, reject malformed or unsafe
paths without returning a partial result, and state that the bundle is not the
canonical project representation.

#### Scenario: Fallback import of a plugin bundle
- **WHEN** a caller explicitly imports a plugin directory with a valid
  manifest, a skill, and a command
- **THEN** the skill is preserved unchanged as a standard-native item and
  the command is recorded as an optional enhancement
- **AND THEN** the result states that import was a compatibility fallback

#### Scenario: Unsafe bundle reference
- **WHEN** a bundle manifest references a file outside the bundle root
- **THEN** import fails with an explicit diagnostic naming the offending
  reference
- **AND THEN** no partial environment is returned

### Requirement: Keep foreign importers separate from harness integrations
The system SHALL use a specialized importer for a foreign format. An importer
MAY know vendor-specific paths and manifests, but a harness integration SHALL
consume the effective environment rather than act as a source or destination
of a conversion.

#### Scenario: Claude plugin is imported
- **WHEN** a caller selects the Claude plugin importer for a Claude plugin
- **THEN** the importer returns portable and optional capabilities with foreign
  provenance
- **AND THEN** Claude and Codex integrations remain peer consumers of the
  resulting environment
