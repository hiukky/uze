## Purpose

Lets a UZE-store-owned MCP server become available to Claude Code and Codex
after `uze setup` and `uze add`, through each harness's own generated
configuration, without converting it to or from an Agent Skill and without
requiring a `uze sync` step.

## ADDED Requirements

### Requirement: MCP is a distinct capability, never converted to or from a Skill
The system SHALL represent an MCP server as its own capability kind,
composed in the same `EffectiveEnvironment` as Agent Skills without merging
or converting either representation into the other's format.

#### Scenario: A package with both a Skill and an MCP server
- **WHEN** a package installed in the UZE store declares both an Agent
  Skill and an MCP server
- **THEN** the effective environment contains one resource of each kind
- **AND THEN** neither resource's bytes or generated configuration
  references the other's representation

### Requirement: MCP attachment is generated harness configuration, not filesystem discovery
The system SHALL attach an MCP server to a harness by producing that
harness's own configuration entry (via its documented management surface),
distinct from the filesystem-symlink mechanism used for Agent Skills. The
generated entry SHALL reference the UZE store's copy of the server, and
SHALL NOT duplicate the server as a second permanent installation.

#### Scenario: Same package, two different generated configurations
- **WHEN** the same UZE-store MCP resource is attached to both Claude Code
  and Codex
- **THEN** each harness receives its own native configuration entry
- **AND THEN** both entries reference the same underlying UZE-store-owned
  server executable

### Requirement: MCP attachment requires completed setup, with no fallback probe
Because no per-session MCP conformance mechanism exists (unlike the
`--plugin-dir` fallback for Agent Skills), the system SHALL only attach an
MCP resource to a harness whose `uze setup` has completed. Attempting to
attach before setup SHALL report the resource as unsupported for that
harness rather than fabricate a mechanism.

#### Scenario: Package added before setup for a harness
- **WHEN** `uze add` runs for a harness that has not completed `uze setup`
- **THEN** that harness's MCP exposure plan reports the capability as
  unsupported for it
- **AND THEN** no partial or malformed configuration entry is written

### Requirement: Generated vendor configuration is namespaced, reversible, and safe around unrelated entries
A generated MCP configuration entry SHALL be namespaced and attributable to
UZE, SHALL be created idempotently (a second `uze add` for the same
resource does not duplicate or corrupt the entry), and SHALL be removable
without disturbing other entries the harness's configuration already
contains. The system SHALL NOT overwrite a harness's entire MCP
configuration file to add or remove one entry.

#### Scenario: Idempotent re-attachment
- **WHEN** `uze add` runs a second time for a package whose MCP resource is
  already attached to a harness
- **THEN** no duplicate configuration entry is created
- **AND THEN** the harness's other, unrelated configuration is unchanged

### Requirement: No literal secret is written by UZE into generated configuration
The system SHALL NOT write a literal secret value into any configuration
file or CLI invocation it generates for an MCP server. Where a server needs
credentials, the system MAY reference an environment variable by name using
each harness's own supported mechanism, but SHALL NOT resolve, store, or
transmit the secret's value itself.

#### Scenario: Conformance fixture requires no secret
- **WHEN** the conformance MCP fixture is attached to either harness
- **THEN** its generated configuration entry contains no secret value
- **AND THEN** attachment succeeds without any credential being read by UZE

### Requirement: MCP verification is reported in distinct, non-conflatable tiers
The system SHALL distinguish, in its opt-in conformance evidence, whether an
MCP attachment was confirmed by configuration inspection alone, by live
harness-reported connectivity, or by a real authenticated tool invocation.
An authentication or quota failure during a real invocation SHALL be
reported as an environment block, never as MCP incompatibility.

#### Scenario: Configuration confirmed without a live connection check
- **WHEN** a harness's own configuration listing shows the UZE-generated
  entry
- **THEN** the evidence records configuration-level confirmation
- **AND THEN** this is not reported as equivalent to a confirmed live
  connection or a successful tool invocation

#### Scenario: Authenticated invocation is blocked by environment
- **WHEN** a real, authenticated tool-invocation probe fails due to missing
  credentials or quota
- **THEN** the result is an environment block
- **AND THEN** the capability is not reported as unsupported or
  incompatible
