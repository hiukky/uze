## Purpose
Lets a package declare the executables its scripts need, and lets UZE detect them, show the person the exact command to install what is missing, and keep the gap visible until it is closed — so a delivered capability never silently degrades because a tool is missing on the machine, and UZE never runs an installer.

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

### Requirement: Missing requirements are explained, never installed by UZE
At install and on demand, the system SHALL check every effective requirement against the machine. For each missing or too-old executable it SHALL show the executable, its purpose, and the exact command that installs it, suggested from the package managers the machine provides. The system SHALL NOT run that command or any installer; the person runs it in their own shell with their own privileges.

#### Scenario: Missing requirement is explained in the CLI
- **WHEN** `uze plugin install` finds `jq` missing and `apt` is available
- **THEN** the output lists `jq — hook handlers parse JSON` with the command `sudo apt-get install -y jq`
- **AND** no process is started for it; the install completes with `jq` reported unmet

#### Scenario: No known package manager
- **WHEN** a requirement is missing and the machine offers no package manager UZE has a suggestion for
- **THEN** the output states the requirement must be installed manually and names the executable and version constraint

#### Scenario: TUI hands the command to a shell
- **WHEN** the person acts on an unmet requirement shown on a package in the TUI
- **THEN** a shell tab opens with the install command pre-filled and not executed
- **AND** the requirement is re-checked when the tab closes or the person refreshes, and the issue clears once the executable is found

### Requirement: Unmet requirements keep the package installed and the gap visible
A package whose effective requirements are unmet SHALL still install; the capabilities that depend on the missing executable SHALL be delivered with the requirement reported unmet, and `uze plugin list` and the TUI manage view SHALL show the gap with the command that closes it until the executable is found.

#### Scenario: Unmet requirement is visible after install
- **WHEN** `jq` is missing when a package needing it is installed
- **THEN** the package appears in `uze plugin list` and the TUI manage view with `jq` marked unmet and the install command shown
- **AND** the delivered hook wrapper applies its own rule for the missing dependency (deny groups deny, observe groups proceed)

### Requirement: Requirements are re-verified and never owned
`uze doctor` SHALL re-check every effective requirement of every installed package and report a requirement that became unmet or fell below its constraint, with the install command. The system SHALL NOT record ownership of any executable and removing a package SHALL only drop that package's requirement records.

#### Scenario: Doctor reports drift
- **WHEN** `jq` was present at install and is later removed from the machine
- **THEN** `uze doctor` reports the requirement unmet for every package that needs it, with the install command

#### Scenario: Removing a package touches no tool
- **WHEN** a package that required `jq` is removed
- **THEN** `jq` is untouched and no record about it remains for that package
