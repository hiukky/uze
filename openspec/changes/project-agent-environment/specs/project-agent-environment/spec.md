## Purpose

Lets a project declare its desired agent environment (marketplaces +
plugins) in a project-scoped, Git-versionable file (`agents.lock`), so a
fresh clone can reproduce that environment with `uze install` instead of
depending on undocumented global machine state.

## ADDED Requirements

### Requirement: Project-scoped desired state
The system SHALL provide a project-scoped file (`agents.lock`) declaring
the desired agent environment: marketplaces and plugins, with whatever
source/resolution facts are available.

#### Scenario: Shorthand creates a lock entry
- **WHEN** the author runs `uze flow@ai` in a project
- **THEN** `agents.lock` is created or updated with a `flow` plugin entry
  referencing the `ai` marketplace

### Requirement: Global vs project separation
Global machine state (`~/.uze/*`) SHALL remain separate from project
desired state (`agents.lock`): global commands never write the lock, and
project commands never write global state.

#### Scenario: Global admin command does not touch the project lock
- **WHEN** the user runs `uze marketplace add <source>` or `uze plugin
  install <plugin>@<marketplace>`
- **THEN** `agents.lock` is not created or modified, if a project is
  present

### Requirement: Project shorthand requires `@`
The `uze <plugin>@<marketplace>` shorthand SHALL require an explicit
`@marketplace` segment.

#### Scenario: Shorthand without `@` is rejected
- **WHEN** the user runs `uze flow` (no `@`)
- **THEN** the command fails with an error indicating `@marketplace` is
  required, and no lock is written

### Requirement: Fresh-machine reproducibility
`uze install` SHALL reconstruct the environment described by `agents.lock`
on a machine with no prior `uze marketplace add` or `uze add`/`uze
<plugin>@<marketplace>` history for that project.

#### Scenario: Fresh machine with only agents.lock
- **WHEN** the user runs `uze install` in a project whose `agents.lock`
  lists plugins not yet in the local Store
- **THEN** each missing plugin's marketplace source is resolved directly
  from the lock (not from the global marketplace registry), acquired, and
  installed through the same lifecycle `uze add` uses

#### Scenario: Nothing to do
- **WHEN** the user runs `uze install` and every locked plugin is already
  installed
- **THEN** the command reports no changes and performs no acquisition

### Requirement: Trust boundary preserved
`agents.lock` SHALL NOT grant trust on its own. Installing a locked
plugin SHALL go through the same authorization decision as `uze add`.

#### Scenario: Install honors the trust authority passed to it
- **WHEN** the user runs `uze install` (which resolves to a trust
  authority based on the `--trust` flag)
- **THEN** each locked plugin's installation is authorized through that
  same authority, not silently trusted because it came from a lock file

### Requirement: Vendor neutrality
Lock parsing and serialization SHALL live in the vendor-neutral core,
never in a peer harness integration.

#### Scenario: Core has no integration dependency
- **WHEN** `uze-core` is compiled
- **THEN** it does not import any integration-specific code (Claude,
  Codex, Gemini, or OpenCode)

### Requirement: Project root resolution
The system SHALL deterministically resolve a project's root by walking
upward from the working directory, preferring `agents.lock`, then
`AGENTS.md`, then `.git`, falling back to the working directory itself.

#### Scenario: Resolution from a subdirectory
- **WHEN** the user runs a project command from `<root>/subdir`, and
  `<root>` contains `agents.lock`
- **THEN** the project root resolves to `<root>`

### Requirement: `desired ≠ actual` is a reportable, non-error state
A locked plugin that is not yet installed SHALL be a normal, reportable
state — not an error and not collapsed into a generic "unhealthy" signal.

#### Scenario: Status reports a missing locked plugin
- **WHEN** the user runs `uze status` in a project whose lock declares a
  plugin not present in the local Store
- **THEN** the report includes that plugin's lock entry with its
  installed state, distinguishable from an installed one

### Requirement: Remove disambiguation
`uze remove <plugin>` SHALL remove from the project lock when a lock is
present and the plugin is declared there; otherwise it SHALL fall back to
the existing global removal behavior.

#### Scenario: Remove from the project lock
- **WHEN** the user runs `uze remove flow` in a project whose
  `agents.lock` declares `flow`
- **THEN** `flow` is removed from `agents.lock`; the Store copy is left
  untouched

#### Scenario: Remove from the global Store
- **WHEN** the user runs `uze remove flow` in a project with no lock, or
  whose lock does not declare `flow`
- **THEN** `flow` is removed from the global Store, exactly as it would
  be without any project lock involved

### Requirement: Application API surface
The application layer SHALL expose `project_environment()`,
`plan_project_environment()`, `add_project_plugin()`,
`remove_project_plugin()`, and `install_project_environment()` as the
complete surface project-aware presentation layers (CLI, TUI) build on.

#### Scenario: Plan is read-only
- **WHEN** `plan_project_environment(root)` is called
- **THEN** it performs no filesystem write — no lock persisted, no
  package acquired or ingested — regardless of what it finds

#### Scenario: Install reconciles the environment
- **WHEN** `install_project_environment(root, authority)` is called and
  the lock declares plugins not yet installed
- **THEN** each is acquired and installed through the standard lifecycle
  (authorize, prepare, ingest, republish, attach)

### Requirement: Lock persistence ordering
`add_project_plugin` SHALL persist `agents.lock` only after the plugin has
been successfully ingested into the Store — never a lock entry pointing
at a package that was never installed.

#### Scenario: Ingest failure leaves the lock untouched
- **WHEN** `add_project_plugin` is called and the underlying `install
  Materialized` call fails (e.g. a package conflict)
- **THEN** `agents.lock` is not modified
