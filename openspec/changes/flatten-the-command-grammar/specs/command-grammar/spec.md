## Purpose

What the `uze` verbs are, how each one decides whether it is acting on this
machine, on this project, or on both, how it reports that, and what counts
as a project at all.

## ADDED Requirements

### Requirement: A directory is a project only on evidence, and may be none

The project root SHALL be resolved by walking up from the working directory
and taking, in this order: the nearest ancestor holding `agents.yaml`; else
the repository root; else the nearest ancestor holding `AGENTS.md`. When
none of the three is found, there SHALL be **no project**, and no command
SHALL treat the working directory as one.

`AGENTS.md` SHALL NOT take precedence over a repository root: it is a file
UZE writes, so its presence in a subdirectory is evidence that something ran
there, not that a project begins there.

#### Scenario: A nested AGENTS.md does not become the project root
- **WHEN** a repository holds `AGENTS.md` in a subdirectory and no
  `agents.yaml` anywhere
- **THEN** the project root is the repository root

#### Scenario: A plain directory is not a project
- **WHEN** a command runs in a directory that has no `agents.yaml`, is not
  inside a repository, and has no `AGENTS.md`
- **THEN** there is no project, and no project file is created anywhere

#### Scenario: A manifest wins over the repository it sits in
- **WHEN** a repository holds `agents.yaml` in a subdirectory
- **THEN** that subdirectory is the project root

#### Scenario: A worktree is a repository
- **WHEN** a command runs inside a Git worktree, where the repository marker
  is a file rather than a directory
- **THEN** the worktree's root is found as a repository root

### Requirement: The root verbs act on the machine, and on the project when there is one

`uze <plugin>@<marketplace>`, `uze remove`, `uze update` and `uze status`
SHALL act on this machine. When they run inside a project they SHALL also
maintain that project's `agents.yaml` and `agents.lock`; when they do not,
they SHALL do the machine half alone.

Every one of them SHALL report which scopes it changed. A command SHALL NOT
change a scope without naming it.

#### Scenario: Using a plugin inside a project declares it
- **WHEN** `uze <plugin>@<marketplace>` runs inside a project
- **THEN** the package is installed and delivered, the project declares it,
  and the report names both

#### Scenario: Using a plugin outside a project declares nothing
- **WHEN** `uze <plugin>@<marketplace>` runs where there is no project
- **THEN** the package is installed and delivered, no project file is
  written anywhere, and the report says nothing was declared

#### Scenario: A plugin installed outside a project still works everywhere
- **WHEN** a package was installed where there was no project
- **THEN** every harness resolves it in every project, because delivery is
  user-scope and belongs to no project

### Requirement: Every root verb has a meaning with no project

`uze update` with no project SHALL update the packages installed on this
machine, re-resolving each from the source it was installed from. `uze
status` with no project SHALL report this machine — what is installed, from
where, and each package's freshness — rather than reporting the absence of a
project as if it were an error.

Neither SHALL report a project's absence as a failure: there is nothing
wrong with a machine that is not inside a project.

#### Scenario: Updating with no project updates the machine
- **WHEN** `uze update` runs where there is no project
- **THEN** every installed package is re-resolved from its own source, and
  no project file is written anywhere

#### Scenario: Status with no project describes the machine
- **WHEN** `uze status` runs where there is no project
- **THEN** it reports the installed packages, their marketplaces and their
  freshness, and says there is no project here

### Requirement: `-m` states machine scope where a command has two

The verbs that act on both scopes SHALL accept `-m` / `--machine`, which
confines them to this machine and leaves every project file untouched. It
SHALL mean the same thing on each verb it appears on, and SHALL be accepted
whether or not a project is present.

It exists for the caller that cannot choose a working directory to express
intent — a script, a container, a test harness — and for the operator who
wants the machine half while standing in a project.

#### Scenario: A scripted install states its scope
- **WHEN** `uze <plugin>@<marketplace> -m` runs inside a project
- **THEN** the package is installed and delivered and no project file is
  written

#### Scenario: The flag is not required outside a project
- **WHEN** a root verb runs where there is no project, with and without `-m`
- **THEN** both behave identically

### Requirement: Removing undeclares; removing from the machine is asked for

`uze remove <plugin>` SHALL remove the plugin from the project that is
present and SHALL leave the package on the machine and in every harness,
because other projects share them.

`uze remove <plugin> -m` SHALL remove it from this machine, inspecting every
managed artifact before detaching it and refusing when any has drifted.

The system SHALL NOT infer whether another project still wants a package.
Nothing on this machine records which projects declare which plugins, and a
removal justified by a guess is worse than one the operator asked for.

#### Scenario: Removing from a project keeps the package
- **WHEN** `uze remove <plugin>` runs inside a project that declares it
- **THEN** the declaration and its lock entry are gone, and the package is
  still installed and still delivered to every harness

#### Scenario: Removing from the machine is refused on drift
- **WHEN** `uze remove <plugin> -m` runs and a managed artifact has drifted
- **THEN** nothing is detached or deleted, and the drift is reported

#### Scenario: Removing a plugin this project does not declare
- **WHEN** `uze remove <plugin>` runs where the plugin is installed but not
  declared here
- **THEN** nothing is removed, and the message names `-m` as the way to take
  it off this machine

### Requirement: One spelling per operation

No operation SHALL have two command spellings. The `plugin` namespace SHALL
NOT exist, and no alias for its removed verbs SHALL be kept.

#### Scenario: The removed namespace is gone
- **WHEN** a command under the removed namespace is invoked
- **THEN** it is not recognized, and the message names the verb that
  replaced it
