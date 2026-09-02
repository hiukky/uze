## ADDED Requirements

### Requirement: The Antigravity vertical runs the harness signed in
The Antigravity vertical SHALL run the real harness in its signed-in mode against a synthetic identity and synthetic CloudCode endpoints served by the Lab's provider, with zero Internet, so that the harness executes `hooks.json` hooks as it does for a real signed-in account. The identity, token and every served payload SHALL be synthetic and SHALL carry no real account data.

#### Scenario: Vendor control hook executes
- **WHEN** the vertical serves a vendor-format deny hook at the vendor's shared `hooks.json` and scripts a `run_command` call in the signed-in session
- **THEN** the harness denies the command with the hook's reason before any permission prompt
- **AND** `hooks > vendor` passes and the UZE hook checks of the vertical are asserted, not declared

#### Scenario: Model calls go through the signed-in protocol
- **WHEN** the harness sends `v1internal:streamGenerateContent`
- **THEN** the provider answers with the same deterministic content it serves in API-key mode, wrapped in the signed-in response envelope
- **AND** every existing Antigravity check (skills, MCP, TUI) keeps its verdict

#### Scenario: API-key mode stays visible
- **WHEN** the vertical completes
- **THEN** one declared check records that hooks do not execute under an API key, citing the vendor issue, without gating the vertical
