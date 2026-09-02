## Purpose
Lets a package declare the executables its scripts need, and lets UZE detect, propose, install with confirmation, record and re-verify them, so a delivered capability never silently degrades because a tool is missing on the machine.

## ADDED Requirements

### Requirement: A package declares the executables it needs
A package manifest MAY declare `requirements`: a list of executables the package's scripts need on `PATH`, each with a name, an optional minimum version and an optional purpose. The system SHALL validate the declaration at install and SHALL reject a malformed entry before any attachment. The effective requirement set of a package SHALL be the declared set plus every requirement a generated artifact introduces on its own behalf.

#### Scenario: Declared requirement is validated
- **WHEN** a package declares `requirements: [{"executable": "jq", "version": ">=1.6", "purpose": "hook handlers parse JSON"}]`
- **THEN** install accepts the declaration and records it as part of the package's effective requirements

#### Scenario: Packager-introduced requirement joins the set
- **WHEN** a package declares no requirements but its hooks are delivered through a generated wrapper that needs `jq`
- **THEN** the package's effective requirements include `jq` attributed to the wrapper, not to the author

#### Scenario: Malformed declaration is rejected
- **WHEN** a package declares a requirement without an executable name or with an unparseable version constraint
- **THEN** install fails before attaching anything and names the offending entry

### Requirement: Missing requirements are proposed, never installed silently
At install and on demand, the system SHALL check every effective requirement against the machine. For each missing or too-old executable it SHALL present a plan naming the executable, its purpose, and the installer it would use, chosen from what the machine provides. The system SHALL NOT run an installer without an explicit confirmation from the person: interactively in the CLI and the TUI, or through an explicit non-interactive flag.

#### Scenario: Plan is shown and confirmed in the CLI
- **WHEN** `uze plugin install` finds `jq` missing and a package manager is available
- **THEN** the CLI shows the plan (`jq — hook handlers parse JSON — via apt`) and asks for confirmation
- **AND** the installer runs only after the person confirms

#### Scenario: Non-interactive install needs the explicit flag
- **WHEN** `uze plugin install` runs without a terminal and without `--yes`
- **THEN** no installer runs, the package installs with the requirement reported unmet, and the output names the flag that would have allowed it

#### Scenario: No installer available
- **WHEN** a requirement is missing and the machine offers no installer UZE knows how to drive
- **THEN** the plan states that the requirement must be installed manually and the package installs with it reported unmet

### Requirement: Declining keeps the package installed and the gap visible
A package whose effective requirements are unmet SHALL still install; the capabilities that depend on the missing executable SHALL be delivered with the requirement reported unmet, and `uze plugin list` SHALL show the gap with the command that would close it.

#### Scenario: Unmet requirement is visible after install
- **WHEN** the person declines to install `jq` during `uze plugin install`
- **THEN** the package appears in `uze plugin list` with `jq` marked unmet and the install command shown
- **AND** the delivered hook wrapper applies its own rule for the missing dependency (deny groups deny, observe groups proceed)

### Requirement: Installed tools are recorded and re-verified
An executable UZE installed SHALL be recorded by a receipt naming the package that required it and the installer used; an executable the machine already had SHALL be recorded as observed, never as owned. `uze doctor` SHALL re-check every effective requirement of every installed package and report a requirement that became unmet or fell below its constraint. Removing a package SHALL drop its requirement records and SHALL NOT remove any executable.

#### Scenario: Doctor reports drift
- **WHEN** `jq` was present at install and is later removed from the machine
- **THEN** `uze doctor` reports the requirement unmet for every package that needs it, with the command that closes the gap

#### Scenario: Removing a package leaves the tool
- **WHEN** a package whose install brought `jq` is removed
- **THEN** `jq` stays on the machine and `uze doctor` may list it as a UZE-installed tool no package requires
