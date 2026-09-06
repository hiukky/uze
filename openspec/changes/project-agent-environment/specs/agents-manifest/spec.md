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

### Requirement: A marketplace declares its source and what the project takes from it
A marketplace entry SHALL carry the name the project refers to it by,
exactly one source — a Git remote with an optional reference and
subdirectory, or a local path — and the plugins the project installs from
it, as bare names. A plugin's identity is the pair of its name and the
marketplace above it, which is `plugin@marketplace` (ADR-036) spelled
structurally; the marketplace's own reference is what moves them, so a
plugin entry SHALL carry no fields of its own.

There SHALL be no top-level `plugins:` key: a plugin a project declares is
a name under the marketplace that carries it.

#### Scenario: Each source form parses to its own variant
- **WHEN** a manifest declares a marketplace as a Git remote or as a local
  path
- **THEN** it parses into the corresponding source variant carrying that
  variant's own fields, and the plugins under it are attributed to it

#### Scenario: A marketplace with no source, or with two, is rejected
- **WHEN** a marketplace entry declares neither `git:` nor `path:`, or
  declares both
- **THEN** the manifest is reported as malformed, naming the marketplace

#### Scenario: A source may be declared with nothing taken from it
- **WHEN** a marketplace is declared with no plugins under it
- **THEN** the manifest loads, declaring that source and no plugin

#### Scenario: The same plugin under two marketplaces is refused
- **WHEN** two marketplaces each list a plugin of the same name
- **THEN** the manifest is reported as malformed naming both marketplaces,
  because the file cannot say which of the two identities answers to that
  name locally

### Requirement: The built-in marketplace is never declared
The marketplace UZE carries inside its own binary is part of how UZE works,
not something a project chose: it SHALL be available to every project with
no entry behind it, its plugins SHALL be installed by the machine's own
bootstrap rather than by a project declaration, and UZE SHALL write nothing
into a project manifest for it. An entry claiming that name SHALL be
refused.

#### Scenario: A plugin from the built-in marketplace is not declared
- **WHEN** a plugin from the built-in marketplace is added in a project
- **THEN** nothing is written to `agents.yaml`, and a project that had no
  manifest still has none

#### Scenario: Declaring the built-in marketplace is refused
- **WHEN** a manifest declares a marketplace under the built-in name
- **THEN** the manifest is reported as malformed, naming it and saying it
  is built into UZE

### Requirement: The manifest declares intent, never resolution
A declaration SHALL NOT carry a resolved revision, a version, or an
integrity value. Those belong to the lock, and a manifest carrying one
SHALL be reported by name.

#### Scenario: A resolved field in the manifest is rejected
- **WHEN** a manifest entry carries a `revision` or `integrity` field
- **THEN** the manifest is reported as malformed, naming the field and
  stating that resolution belongs to `agents.lock`

### Requirement: Preparation and delivery checks are ordered steps
`setup` and `gate` SHALL each accept one command or a list of commands run
in order in the checkout. Execution SHALL stop at the first failure, and a
failed gate SHALL name the command that failed, so a gate of several checks
does not report only the output of a chain the shell assembled.

#### Scenario: One command and a list mean the same kind of thing
- **WHEN** `gate` is declared as a command, and when it is declared as a
  list of commands
- **THEN** both load, the first as a single step

#### Scenario: A failing step stops the rest and is named
- **WHEN** a gate of three steps fails at the second
- **THEN** delivery is refused naming that command, the third step does not
  run, and the target is untouched

### Requirement: The manifest carries no schema version
`agents.yaml` SHALL NOT carry a schema version. A key the schema does not
define is already reported by name, which is what an older UZE reading a
newer manifest needs; a `version` key SHALL be refused with that reason
rather than reported as an unknown field.

#### Scenario: A schema version is refused with its reason
- **WHEN** a manifest declares `version:` at its root
- **THEN** the manifest is reported as malformed, saying the file carries
  no schema version and why, and not with the answer given to a plugin's
  `version:`

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

### Requirement: UZE writes declarations as typed values, in the file's own style
A declaration UZE writes SHALL be emitted from a typed value rather than
composed as YAML text, so a value carrying a comma, a brace or a colon
cannot change the structure of the file it lands in. The emitted entry
SHALL follow the surrounding document's own layout.

#### Scenario: A value that would break the syntax is quoted
- **WHEN** a marketplace whose path or reference carries a comma or a brace
  is written into the manifest
- **THEN** the entry loads back carrying exactly that value, and the
  manifest is still one entry per declaration

### Requirement: A declaration that cannot be honored is refused, not accepted and ignored
The manifest SHALL refuse a field UZE does not act on, naming what does
carry the meaning instead. A plugin version SHALL be refused for as long
as nothing in a plugin or a marketplace catalog declares one, and the
refusal SHALL NOT point at `agents.lock`, whose version field stays empty
for the same reason.

#### Scenario: A cap of zero is refused rather than honored
- **WHEN** a manifest declares `worktrees.slots: 0`
- **THEN** the manifest is reported as malformed, saying it would refuse
  every checkout, rather than loading a policy under which no isolated
  work can ever start

#### Scenario: A version is answered with what UZE can pin
- **WHEN** a plugin entry declares a `version:`
- **THEN** the manifest is reported as malformed, saying UZE does not
  resolve plugin versions yet and naming `ref:` as the pin that exists

### Requirement: A key declared twice is an error
The manifest SHALL be read with duplicate keys refused, rather than with
YAML's last-one-wins default: a declaration written twice is a mistake in
an authored file, and keeping one of them silently drops the other.

#### Scenario: The same plugin declared twice is reported
- **WHEN** a manifest declares the same plugin key twice
- **THEN** the manifest is reported as malformed, naming the key, rather
  than loading with one declaration silently discarded

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
- **THEN** `agents.yaml` is created carrying that declaration and the
  scaffold below, and no other declaration is made on the project's behalf

#### Scenario: `uze install` creates a manifest explicitly
- **WHEN** `uze install` is run in a project with no `agents.yaml`
- **THEN** a manifest is created carrying the scaffold below, and no plugins
- **AND** running it again leaves the file byte-identical

#### Scenario: A manifest somebody wrote is never touched by creation
- **WHEN** `uze install` runs in a project whose `agents.yaml` exists
- **THEN** the file is left exactly as it is, comments included

### Requirement: A created manifest shows the whole vocabulary
A manifest UZE creates SHALL name every key the schema accepts, each with
the default already in force spelled out beside it, so the options are
discoverable by opening the file rather than by reading documentation.
Only a key whose default UZE can state as a value SHALL be live; every
other SHALL be commented, so creating the file declares exactly what was
already in force and uncommenting a line is what changes behavior.

#### Scenario: Every key the schema accepts is offered
- **WHEN** UZE creates `agents.yaml`
- **THEN** the file names `worktrees` with `completion`, `target`, `link`,
  `setup`, `gate` and `slots`, and `marketplaces` and `plugins` with the
  fields each accepts, in the same block spelling UZE writes

#### Scenario: The scaffold declares nothing new
- **WHEN** a manifest UZE created is read back
- **THEN** the isolation policy it resolves to is the built-in default,
  and it declares no marketplace and no plugin

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
