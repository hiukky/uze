## ADDED Requirements

### Requirement: Doctor reports package requirements
`uze doctor` SHALL re-verify every effective requirement of every installed package against the machine and SHALL report each unmet or drifted requirement with the package that needs it, its purpose, and the command that closes the gap, for the person to run.

#### Scenario: Requirement removed after install
- **WHEN** an executable an installed package requires is no longer on `PATH` or is below the declared minimum version
- **THEN** `uze doctor` reports it under that package with the closing command
