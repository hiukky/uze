## ADDED Requirements

### Requirement: Doctor reports package requirements
`uze doctor` SHALL re-verify every effective requirement of every installed package against the machine and SHALL report each unmet or drifted requirement with the package that needs it, its purpose, and the command that would close the gap. It SHALL also list executables UZE installed that no installed package requires anymore.

#### Scenario: Requirement removed after install
- **WHEN** an executable an installed package requires is no longer on `PATH` or is below the declared minimum version
- **THEN** `uze doctor` reports it under that package with the closing command

#### Scenario: Orphaned UZE-installed tool
- **WHEN** UZE installed an executable for a package that was later removed
- **THEN** `uze doctor` lists the executable as UZE-installed and unrequired, and does not remove it
