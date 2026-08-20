## Why

ADR-008 proved Plugin First technically. UZE now needs a safe local product
experience: lifecycle must know every owned harness artifact, package data
must be consumable without UI-specific filesystem logic, and CLI/TUI must
present the same deterministic facts.

## What changes

- Add a package attachment ledger with safe ownership/drift checks.
- Add add/list/inspect/remove/doctor application operations.
- Add CLI commands over that application boundary.
- Add a minimal keyboard-first TUI over the same API.

## Non-goals

Hooks, Agents, Commands, remote marketplaces, accounts, cloud sync, daemon,
runtime proxy, browser dashboard, scoring, and a UZE plugin format.
