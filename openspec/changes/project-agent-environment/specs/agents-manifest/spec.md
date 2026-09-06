## Purpose

Defines `agents.yaml`: the hand-authored declaration of a project's agent
environment. It is the only file a person edits, and the only input
`agents.lock` is derived from. ADR-017 originally folded intent and
resolution into one file and recorded the split as deferred; this
capability is that split, and `agents-lock` is now the derived half.

## ADDED Requirements

### Requirement: The manifest is the authored source of intent
`agents.yaml` SHALL hold everything a person decides about a project's
agent environment — which marketplaces it draws from, which plugins it
wants, and its isolation policy — and SHALL be the only file UZE asks a
person to write. No intent SHALL exist only in `agents.lock`.

#### Scenario: Deleting the lock loses no declaration
- **WHEN** `agents.lock` is deleted and the environment is resolved again
  from `agents.yaml` alone
- **THEN** every marketplace, plugin and policy the project declared is
  still declared, and the regenerated lock is equivalent to the deleted
  one

#### Scenario: The lock alone is not a project declaration
- **WHEN** a directory contains `agents.lock` and no `agents.yaml`
- **THEN** the lock is reported as orphaned rather than treated as the
  project's declaration

### Requirement: Manifest is YAML and preserves comments
The manifest SHALL be YAML, and SHALL be edited in place rather than
rewritten wholesale, so a comment a person wrote next to a declaration
survives a command that changes an unrelated entry.

#### Scenario: A comment survives an unrelated edit
- **WHEN** a manifest carrying a comment above one plugin has a second
  plugin added by `uze <plugin>@<marketplace>`
- **THEN** the comment is still present, attached to the same entry

### Requirement: Manifest declares marketplaces by name and source
A marketplace entry SHALL carry the name the project refers to it by and
the source it is acquired from — a Git remote with an optional reference
and subdirectory, a local path, or the embedded snapshot. The name is UX;
the source is identity.

#### Scenario: Each source form parses to its own variant
- **WHEN** a manifest declares a marketplace as a Git remote, as a local
  path, or as the embedded snapshot
- **THEN** it parses into the corresponding source variant carrying that
  variant's own fields

### Requirement: Manifest declares plugins as requests, not resolutions
A plugin entry SHALL express what the project wants — a name within a
declared marketplace, or a direct Git source, with an optional reference —
and SHALL NOT carry a resolved revision, a version, or an integrity
value. Those belong to the lock.

#### Scenario: A resolved field in the manifest is rejected
- **WHEN** a manifest plugin entry carries a `revision` or `integrity`
  field
- **THEN** the manifest is reported as malformed, naming the field and
  stating that resolution belongs to `agents.lock`

### Requirement: Manifest carries the worktree isolation policy
The isolation policy — target branch, completion behavior, linked files,
setup command, gate command and concurrent-checkout cap — SHALL be
declared in the manifest, since every field of it is a decision a person
makes rather than a resolution UZE computes.

#### Scenario: The policy round-trips with the rest of the manifest
- **WHEN** a manifest declaring marketplaces, plugins and a full policy
  block is written and read back
- **THEN** every declaration is preserved

#### Scenario: A manifest without a policy block still loads
- **WHEN** a manifest declares marketplaces and plugins and no policy
  block
- **THEN** the environment loads with the default completion behavior and
  no linked files, setup, gate or cap

### Requirement: Unknown manifest fields are rejected by name
A manifest naming a field the schema does not define SHALL be reported as
malformed, naming that field, rather than parsed with the field ignored —
a typo in an authored file must not become silence.

#### Scenario: A misspelled field is named
- **WHEN** a manifest carries `descripton` where `description` is defined
- **THEN** the manifest is reported as malformed, naming the unknown
  field

### Requirement: The manifest anchors a consumer workspace
Workspace detection SHALL treat `agents.yaml` as the consumer anchor, in
place of `agents.lock`, so a project that has declared an environment but
never resolved it is still a workspace.

#### Scenario: A manifest with no lock is a consumer workspace
- **WHEN** a directory contains `agents.yaml` and no `agents.lock`
- **THEN** it is detected as a consumer workspace rooted at that directory

### Requirement: The manifest is created by intent, never by arrival
UZE SHALL NOT create `agents.yaml` merely because a Git project was
opened, inspected, or launched into. It SHALL be created only by an act
that declares something or sets the project up: `uze install`, adding a
plugin, or changing the isolation policy from the workspace client. A project with no manifest
SHALL behave exactly as one declaring nothing — the default isolation
policy, no plugins — rather than failing or prompting.

#### Scenario: Opening the workspace client writes nothing
- **WHEN** the TUI starts in a Git repository with no `agents.yaml`
- **THEN** no file is created, the repository's working tree is unchanged,
  and the default isolation policy is in effect

#### Scenario: Declaring something creates the manifest
- **WHEN** a plugin is added, or the isolation policy is changed from the
  workspace client, in a project with no `agents.yaml`
- **THEN** `agents.yaml` is created carrying that declaration and nothing
  else

#### Scenario: `uze install` creates a manifest explicitly
- **WHEN** `uze install` is run in a project with no `agents.yaml`
- **THEN** a manifest is created with the built-in default isolation
  policy written out and commented, and no plugins
- **AND** running it again leaves the file byte-identical

#### Scenario: A manifest somebody wrote is never touched by creation
- **WHEN** `uze install` runs in a project whose `agents.yaml` exists
- **THEN** the file is left exactly as it is, comments included

### Requirement: The primary checkout owns the policy, and every worktree inherits it
The isolation policy SHALL be read from the manifest in the primary
checkout — the repository the worktrees are born from — for every task in
that repository. An isolated checkout SHALL NOT declare or override a
policy of its own, and a manifest found inside one SHALL be ignored in
favor of the primary's. There is no per-worktree policy, and no scope
above the project: an undeclared policy resolves to the built-in default
on every machine, so a repository projects the same `AGENTS.md`
everywhere.

#### Scenario: An agent's checkout inherits the primary's policy
- **WHEN** a task is evaluated from inside an isolated checkout
- **THEN** the policy applied is the primary checkout's

#### Scenario: A manifest inside an isolated checkout does not override
- **WHEN** an isolated checkout contains an `agents.yaml` differing from
  the primary's
- **THEN** the primary's policy is the one applied

#### Scenario: An undeclared policy resolves identically on any machine
- **WHEN** the same repository with no declared policy is used on two
  machines
- **THEN** both resolve to the built-in default and project the same
  `AGENTS.md` text
