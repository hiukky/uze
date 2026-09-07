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
The system SHALL report drift across the whole chain a project's
environment passes through — declared (`agents.yaml`), locked
(`agents.lock`), installed (the Store), delivered (the projected
`AGENTS.md` region and the harness bridges it implies) — as a normal,
reportable state, not an error and not collapsed into a generic
"unhealthy" signal. A drift no surface names is a drift nobody can act
on, which is what makes the manifest feel inert after an edit.

#### Scenario: Status reports a missing locked plugin
- **WHEN** the user runs `uze status` in a project whose lock declares a
  plugin not present in the local Store
- **THEN** the report includes that plugin's lock entry with its
  installed state, distinguishable from an installed one

#### Scenario: Status reports a declaration the lock does not answer for
- **WHEN** the user runs `uze status` in a project whose `agents.yaml`
  declares a plugin the lock has no entry for
- **THEN** the report names that plugin as declared and unresolved, and
  names the command that resolves it

#### Scenario: Status reports what the manifest no longer declares
- **WHEN** the user runs `uze status` in a project whose lock carries a
  plugin the manifest no longer declares
- **THEN** the report names that plugin as surplus, distinguishable from
  a plugin that is merely missing

#### Scenario: Status reports a projection the policy has outrun
- **WHEN** the user runs `uze status` after `worktrees.completion` was
  changed in `agents.yaml` and nothing has reconciled the context since
- **THEN** the report names the projected `AGENTS.md` region as stale,
  because agents are still reading the previous instruction

#### Scenario: Reporting drift writes nothing
- **WHEN** any of the above is reported
- **THEN** no file in the project and nothing in the Store is created,
  modified or removed

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

### Requirement: The plan is founded on the manifest
The project environment plan SHALL be computed from `agents.yaml` as its
starting point, not from `agents.lock`. The lock records only what
resolving the manifest produced, so a plan founded on it cannot see a
declaration nobody has resolved yet — the most common reason a person
edits the file at all.

#### Scenario: A manifest edit is visible to the plan
- **WHEN** a plugin is added to `agents.yaml` and the plan is computed
  before anything is installed
- **THEN** the plan reports that declaration as pending work

#### Scenario: The plan is read-only
- **WHEN** the plan is computed in any state, drifted or not
- **THEN** it performs no filesystem write — no lock persisted, no
  package acquired or ingested

### Requirement: Install converges what the manifest no longer declares
`uze install` SHALL converge removals as well as additions: a plugin the
manifest no longer declares is removed from the lock and detached, so a
declarative manifest is declarative in both directions. Because removal
is destructive it SHALL be confirmed before it is performed, and it SHALL
inspect current state before detaching — drift blocks the removal rather
than authorizing it.

#### Scenario: An undeclared plugin is removed
- **WHEN** the user removes a plugin from `agents.yaml` and runs `uze
  install`, confirming the removal
- **THEN** the plugin is removed from `agents.lock` and detached from
  every harness that received it

#### Scenario: A refused confirmation changes nothing
- **WHEN** the same run is not confirmed
- **THEN** the lock and every attachment are left exactly as they were,
  and the drift is still reported

#### Scenario: Drift refuses the detach
- **WHEN** a surplus plugin's managed artifact no longer matches its
  receipt
- **THEN** the detach is refused and reported, and no artifact UZE did
  not write is removed

### Requirement: Installing is one act
`uze install` SHALL leave the project context reconciled when it
finishes, so declaring an environment and projecting it are one intention
rather than two commands. It SHALL also be reachable as `uze i`.

#### Scenario: Install leaves the context reconciled
- **WHEN** `uze install` completes having changed what the project's
  packages contribute
- **THEN** the project's `AGENTS.md` and the harness bridges it implies
  are reconciled in the same run, and the report says so

#### Scenario: Install reconciles a policy change
- **WHEN** `uze install` runs after `worktrees.completion` changed and
  nothing else did
- **THEN** the projected region is rewritten to the new policy's text

#### Scenario: The alias is the same command
- **WHEN** the user runs `uze i`
- **THEN** it behaves exactly as `uze install`, including its arguments
  and its report

### Requirement: The client reports drift and never applies it
The workspace client SHALL surface project environment drift and offer
the action that resolves it, and SHALL NOT apply it on its own. Opening
the client must write nothing into a repository somebody is only looking
at, and that rule does not weaken because the write would be a helpful
one.

#### Scenario: Drift is surfaced with the action beside it
- **WHEN** the client is open on a project whose `agents.yaml` has
  changed since the last install
- **THEN** the drift is named where the project is described, with the
  action that applies it

#### Scenario: Opening the client applies nothing
- **WHEN** the client is opened on a drifted project and no action is
  taken
- **THEN** no project file, no lock entry and no attachment is written

#### Scenario: Reading drift costs no network
- **WHEN** the client computes the drift signal
- **THEN** it is answered from the project's own files and the Store's
  index alone, with no acquisition and no remote read
