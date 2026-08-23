## Purpose

Gives harness integrations (Claude, Codex, OpenCode, Gemini) a machine-level namespace consistent with
`market` and `plugin`, as a thin re-presentation of data `uze doctor`/`uze setup` already compute — no new
provisioning behavior.

## ADDED Requirements

### Requirement: `harness` is a machine-level namespace over existing detection and provisioning data
The system SHALL provide `uze harness list` and `uze harness inspect <name>`, presenting exactly the
harness detection/setup/provisioning data `uze doctor` already reports, scoped to one harness for
`inspect`. Neither command SHALL perform any provisioning action.

#### Scenario: List mirrors doctor's harness section
- **WHEN** the user runs `uze harness list`
- **THEN** the output shows the same set of harnesses, detection status, and setup state as the
  "Harnesses" section of `uze doctor`, with no additional computation

#### Scenario: Inspect narrows to one harness
- **WHEN** the user runs `uze harness inspect claude`
- **THEN** the output shows `claude`'s detection, setup, and provisioning detail; the command fails with a
  clear error if `claude` is not a registered integration id or alias

### Requirement: `uze harness setup <name>` is the namespaced spelling of provisioning; `uze setup` remains a root convenience
`uze harness setup <name>` SHALL invoke exactly the same provisioning behavior as today's `uze setup
<name>`, with no behavior change. `uze setup` (with or without a harness argument) SHALL continue to exist
at the root as the documented convenience form — it is a diagnostics/bootstrap command, not a project
command, and is not moved under `harness` only because moving it would break the single most-used
onboarding command for no behavioral gain.

#### Scenario: Namespaced and root setup are equivalent
- **WHEN** the user runs `uze harness setup claude` or `uze setup claude`
- **THEN** both invoke the identical provisioning flow and report identical results

### Requirement: No new provisioning verbs are introduced
This capability SHALL NOT add `harness enable`, `harness disable`, or any verb beyond `list`, `inspect`,
and `setup` — there is no defined semantics yet for enabling/disabling a harness independently of
detection and provisioning.

#### Scenario: Enable/disable is absent
- **WHEN** the user runs `uze harness enable claude` or `uze harness disable claude`
- **THEN** the command fails as an unrecognized subcommand — these verbs do not exist
