## Why

`uze setup` currently prepares UZE-owned attachment prerequisites only after a
harness is already installed. That makes a fresh environment require users to
learn each vendor's installer before UZE can compose their agent environment.

UZE should make `setup` the explicit, reproducible bootstrap for supported CLI
harnesses while keeping plugin distribution independent from executable
provisioning.

## What Changes

- Make `uze setup [harness]` ensure the selected harness is installed at its
  official latest-stable channel, update an existing supported installation,
  verify the executable, then run the existing UZE preparation and package
  exposure steps.
- Introduce an integration-owned provisioning contract and secret-free
  provisioning record. The Application orchestrates it but never embeds
  vendor installer URLs, command syntax, or platform-specific schemas.
- Retain `uze add <plugin>` as a non-provisioning path: it prepares and
  attaches to harnesses already detected, but never downloads or upgrades a
  harness implicitly.
- Start with Claude Code, Codex, OpenCode, and Gemini CLI using documented
  official installation/update routes. An unavailable official route on a
  platform is reported clearly rather than replaced with an unofficial one.
- Record only UZE-initiated provisioning provenance for future safe harness
  removal. Harness removal is not introduced by this change.

## Capabilities

### New Capabilities

- `harness-provisioning`: Explicit setup can install, update, verify, and
  prepare supported CLI harnesses through their official vendor routes.

### Modified Capabilities

- None.

## Impact

- `UzeApplication::setup`, CLI/TUI setup presentation, `IntegrationPort` and
  peer integrations gain a narrow provisioning path.
- `$UZE_HOME/state` gains secret-free provisioning provenance distinct from
  attachment ownership.
- Tests need a fake official-command runner and isolated process contracts;
  no real vendor installer runs in ordinary `cargo test`.
- README, LikeC4, ADRs, and this OpenSpec change describe UZE as a
  compatibility/distribution layer with an explicit harness bootstrap layer,
  not a general SDK/version manager.
