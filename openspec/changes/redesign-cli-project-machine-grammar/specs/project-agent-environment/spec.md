## MODIFIED Requirements

### Requirement: Remove disambiguation
`uze remove <plugin>` SHALL be strictly project-scoped: it SHALL remove `<plugin>` from `agents.lock` when
a lock is present and declares it, and SHALL NOT fall back to removing the plugin from the machine Store
under any circumstance. Removing a plugin from the machine Store SHALL only ever be reachable through
`uze plugin remove <plugin>`.

This supersedes this capability's original "Remove disambiguation" requirement (`openspec/changes/project-agent-environment/specs/project-agent-environment/spec.md`),
which specified an implicit fallback to global removal when no lock was present or the plugin was not
declared in it. See ADR-019 ("CLI Command Grammar: Explicit Project/Machine Boundary") for why that
fallback — intentional at the time, to keep `uze remove` working as a drop-in replacement for the
pre-project-environment global `remove` — is now considered a boundary violation rather than a convenience:
it made `uze remove flow` sometimes delete a machine-wide package other projects may depend on, silently,
based on state (does a lock exist? does it mention this plugin?) invisible at the call site.

#### Scenario: Remove from the project lock
- **WHEN** the user runs `uze remove flow` in a project whose `agents.lock` declares `flow`
- **THEN** `flow` is removed from `agents.lock`; the Store copy is left untouched

#### Scenario: Plugin not declared in the project's lock
- **WHEN** the user runs `uze remove flow` in a project whose `agents.lock` exists but does not declare
  `flow`
- **THEN** the command fails, reporting that `flow` is not used by this project; nothing is removed
  anywhere, and the error suggests `uze plugin remove flow` if the intent was a machine-level removal

#### Scenario: No project lock present
- **WHEN** the user runs `uze remove flow` in a directory with no `agents.lock` anywhere in its ancestry
- **THEN** the command fails, reporting that no project environment was found; nothing is removed, and the
  error suggests `uze plugin remove flow` if the intent was a machine-level removal

#### Scenario: Plugin remains installed on the machine after project removal
- **WHEN** `uze remove flow` succeeds and another project's `agents.lock` also declares `flow`, or `flow`
  was separately installed via `uze plugin install`
- **THEN** `flow` remains fully installed and attached on the machine; only the removing project's lock
  entry is gone

#### Scenario: Machine-level removal still respects existing lifecycle/drift safety
- **WHEN** the user runs `uze plugin remove flow` and `flow`'s harness attachments are not all in a
  `Matched` state (per ADR-009)
- **THEN** removal is blocked exactly as `uze plugin remove` already blocks it today — this requirement
  changes only which command reaches machine-level removal, not the safety rules once it does
