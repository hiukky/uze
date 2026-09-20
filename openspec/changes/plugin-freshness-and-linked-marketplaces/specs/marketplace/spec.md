## ADDED Requirements

### Requirement: A marketplace may be linked to a checkout this machine develops

The system SHALL let an operator declare that this machine reads a
marketplace from a checkout they develop, rather than from the source the
registry records. The declaration SHALL be machine state and SHALL NOT be
written into any project file.

While a marketplace is linked, resolving any of its plugins SHALL read the
checkout's working tree — not its committed state and not its remote — and
SHALL re-ingest a plugin whose content there differs from what the Store
holds. Re-ingestion SHALL need no network and no commit.

Unlinking SHALL restore resolution from the registered source and SHALL
leave the Store's bytes alone until something asks for them again.

#### Scenario: Linking a marketplace to a checkout
- **WHEN** an operator links a registered marketplace to a directory that
  is a Git work tree whose identity matches the marketplace's
- **THEN** the link is recorded in machine state, no project file changes,
  and subsequent resolution of that marketplace's plugins reads the
  checkout

#### Scenario: An edit in a linked checkout reaches the harness with no commit
- **WHEN** a plugin's content is edited in a linked marketplace's checkout
  and any command that resolves that plugin runs
- **THEN** the Store holds the edited content and every harness resolves it,
  with nothing committed and nothing fetched

#### Scenario: Linking to a directory that does not exist yet clones into it
- **WHEN** an operator links a registered marketplace to a path that does
  not exist
- **THEN** the marketplace's source is cloned into that path as an ordinary
  checkout the operator owns, and the link is recorded against it

#### Scenario: A linked checkout is the operator's, and UZE performs no Git on it
- **WHEN** a marketplace is linked to a checkout
- **THEN** UZE reads that working tree and never commits, fetches, checks
  out, stashes or resets it — its branch and its uncommitted work are the
  operator's alone

#### Scenario: Linking to a checkout of a different repository is refused
- **WHEN** an operator links a marketplace to a checkout whose repository
  identity is not the marketplace's
- **THEN** the link is refused, naming both identities, and nothing is
  recorded

#### Scenario: Unlinking returns to the registered source
- **WHEN** a linked marketplace is unlinked
- **THEN** the link record is removed and the next resolution reads the
  registered source

### Requirement: A Git marketplace is cached as a repository, and acquired from it

A marketplace registered from a Git source SHALL be kept under the cache
tier as a repository rather than as a copied file tree, cloned without blobs
so that every commit and tree is present and file content is fetched only
for what is checked out.

Acquiring a plugin from it SHALL update that repository and check out only
the plugin's own subdirectory — the path the marketplace manifest already
names — rather than cloning the source again. Acquiring a second plugin from
the same marketplace SHALL NOT open a second connection to it.

What enters the Store SHALL carry no repository metadata, so a stored
package never depends on the cache to be readable.

A source that does not support the filter SHALL still work, and a checkout
pinned to any commit SHALL still be reachable — the repository keeps its
whole history.

The cache SHALL hold no materialized copy of a plugin: after an install,
the plugin's bytes SHALL exist as files in exactly one place, the Store.
What the cache keeps is the repository's history, which the Store is not
and cannot be.

#### Scenario: A plugin's bytes are materialized once
- **WHEN** a plugin has been installed from a Git marketplace
- **THEN** its files exist in the Store and nowhere else UZE owns

#### Scenario: A second plugin from one marketplace costs no second clone
- **WHEN** a plugin is acquired from a marketplace whose repository is
  already cached, and then a second plugin from that same marketplace
- **THEN** neither acquisition clones the source again

#### Scenario: A pinned commit is still reachable
- **WHEN** a plugin is reproduced at a commit recorded in `agents.lock` that
  is not the ref's head
- **THEN** that commit is checked out from the cached repository

#### Scenario: The Store never carries repository metadata
- **WHEN** any package is installed from a Git marketplace
- **THEN** its stored bytes contain no repository metadata and are readable
  with the cache deleted

#### Scenario: Deleting the cache costs a clone and nothing else
- **WHEN** the cache tier is deleted entirely
- **THEN** every installed package still delivers, and the next acquisition
  clones once

### Requirement: A marketplace this machine cannot reach is skipped, never fatal

A marketplace whose identity resolves nowhere but the machine that declared
it — a local checkout with no remote — SHALL remain fully usable on that
machine, and SHALL be skipped rather than attempted on a machine that cannot
resolve it.

`uze install` and `uze update` SHALL install every plugin they can resolve,
SHALL skip each plugin whose marketplace this machine cannot reach, and
SHALL report each skip by name with the reason. The command SHALL succeed:
a project that also declares reachable marketplaces is not broken by one
that is somebody else's local checkout, and failing the whole command over
it leaves a contributor with nothing rather than with most of the
environment.

Every surface that reports a project's state SHALL say that a project
declaring such a marketplace cannot be fully reproduced elsewhere, and name
which part.

#### Scenario: A contributor gets the environment that is reachable
- **WHEN** `uze install` runs on a machine where one declared marketplace is
  a path that does not exist, and another is a reachable remote
- **THEN** the reachable marketplace's plugins are installed, the
  unreachable one's are skipped and named, and the command succeeds

#### Scenario: Status names what a clone would not reach
- **WHEN** a project declares plugins from a marketplace whose identity is a
  path on this machine
- **THEN** `uze status` reports the project as not fully reproducible
  elsewhere and names that marketplace and its plugins

#### Scenario: A skip is never silent
- **WHEN** any plugin is skipped for an unreachable marketplace
- **THEN** it is named in the command's own report, not only in `status`

#### Scenario: A local-only marketplace still installs where it exists
- **WHEN** plugins are installed from a marketplace with no remote, on the
  machine that has it
- **THEN** installation, delivery and removal behave exactly as for any
  other marketplace

#### Scenario: A reachable marketplace that fails is still an error
- **WHEN** a marketplace this machine can reach refuses or fails for any
  reason other than being unreachable by declaration
- **THEN** the command fails and reports it, rather than skipping it

### Requirement: An acquisition refused on credentials is reported as a credential question

When acquiring from a marketplace fails because the source could not be
authenticated or was not found under the operator's credentials, the system
SHALL report the marketplace's name, the URL it tried, and that the failure
is one of access — rather than surfacing the underlying tool's own output as
the whole message.

#### Scenario: A private marketplace on a machine with no access
- **WHEN** `uze install` resolves a marketplace whose repository refuses the
  operator's credentials
- **THEN** the error names the marketplace, names the URL, and states that
  it could not be accessed with this machine's credentials
