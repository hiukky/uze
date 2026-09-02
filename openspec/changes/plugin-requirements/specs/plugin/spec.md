## ADDED Requirements

### Requirement: Plugin install resolves requirements before attachment
`uze plugin install` SHALL validate the package's declared requirements and derive the effective set (declared plus packager-introduced) before attaching any capability, SHALL present a plan for every missing requirement and wait for confirmation, and SHALL complete the install whether or not the plan was accepted, reporting each requirement as met, installed, or unmet.

#### Scenario: Install with all requirements met
- **WHEN** every effective requirement of the package is already on the machine
- **THEN** install proceeds without any prompt and the report marks each requirement as met

#### Scenario: Install continues after a declined plan
- **WHEN** the person declines the requirement plan
- **THEN** the package's capabilities are attached and the report marks the declined requirements as unmet

### Requirement: Plugin list shows requirement status
`uze plugin list` SHALL show, per installed package, whether its effective requirements are met, and for an unmet one the executable and the command that would install it.

#### Scenario: Unmet requirement in the list
- **WHEN** an installed package has an unmet requirement
- **THEN** the list row for that package names the missing executable and the install command
