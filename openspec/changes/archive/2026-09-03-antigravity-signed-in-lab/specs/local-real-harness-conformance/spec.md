## ADDED Requirements

### Requirement: The Antigravity vertical runs the harness signed in
The Antigravity vertical SHALL run the real harness in its signed-in mode against a synthetic identity and synthetic CloudCode endpoints served by the Lab's provider, with zero Internet, so that the harness executes `hooks.json` hooks as it does for a real signed-in account. The identity, token and every served payload SHALL be synthetic and SHALL carry no real account data.

#### Scenario: Vendor control hook executes
- **WHEN** the vertical serves a vendor-format deny hook at the vendor's shared `hooks.json` and scripts a `run_command` call in the signed-in session
- **THEN** the harness denies the command with the hook's reason before any permission prompt
- **AND** `hooks > vendor` passes, so the mode in which the vendor executes hooks is proven rather than assumed

#### Scenario: Delivery is a second live precondition
- **WHEN** the vertical starts a session with UZE's hook package installed
- **THEN** it records from the harness's own log how many `hooks.json` files the harness read
- **AND** `hooks > delivery` passes only if the harness loaded the hooks UZE delivered; while it does not, the UZE hook checks are declared against that measurement, never against a stale reason and never silently dropped

#### Scenario: UZE's own hooks are asserted
- **WHEN** the vertical scripts the intercepted tool against UZE's delivered hook groups in the signed-in session
- **THEN** the denial reason reaches the conversation, the intercepted tool never executes, the handler's portable vocabulary (`tool=shell`) is relayed from the harness's own payload, a second handler after a denial never runs, and an allowed call executes

#### Scenario: Model calls go through the signed-in protocol
- **WHEN** the harness sends `v1internal:streamGenerateContent`
- **THEN** the provider answers with the same deterministic content it serves in API-key mode, wrapped in the signed-in response envelope
- **AND** every existing Antigravity check (skills, MCP, TUI) keeps its verdict

#### Scenario: API-key mode stays visible
- **WHEN** the vertical completes
- **THEN** one declared check records that hooks do not execute under an API key, citing the vendor issue, without gating the vertical
