## ADDED Requirements

### Requirement: Plugin install reports requirements
`uze plugin install` SHALL validate the package's declared requirements and derive the effective set (declared plus packager-introduced) before attaching any capability, SHALL complete the install regardless of what is missing, and SHALL report each requirement as met or unmet — an unmet one with the command that installs it.

#### Scenario: Install with all requirements met
- **WHEN** every effective requirement of the package is already on the machine
- **THEN** install proceeds and the report marks each requirement as met

#### Scenario: Install with a missing requirement
- **WHEN** an effective requirement is missing
- **THEN** the package's capabilities are attached and the report marks it unmet with the install command for the person to run

### Requirement: Plugin list shows requirement status
`uze plugin list` SHALL show, per installed package, whether its effective requirements are met, and for an unmet one the executable and the command that would install it.

#### Scenario: Unmet requirement in the list
- **WHEN** an installed package has an unmet requirement
- **THEN** the list row for that package names the missing executable and the install command
