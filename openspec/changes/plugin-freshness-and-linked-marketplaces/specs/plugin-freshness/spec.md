## Purpose

How UZE answers whether an installed plugin is still the one that exists:
what is compared, when the question is asked, what every surface reports,
what may be applied without asking, and the command that moves a pin
forward.

## ADDED Requirements

### Requirement: Freshness is the commit, and nothing declares a version

A plugin's freshness SHALL be the relationship between the marketplace
revision its bytes were resolved at and the revision the marketplace's
declared ref points at now. No version field SHALL be introduced into
`plugin.json`, `marketplace.json`, `agents.yaml` or `agents.lock`, and no
surface SHALL report a version number the chain does not carry.

A marketplace read from a checkout this machine develops has no meaningful
"newer": the working tree is what exists. Its plugins SHALL report as
linked rather than as fresh or stale.

A package with no marketplace ref to compare against SHALL report
**unpinned**, which is distinct from **not checked**: the first says there
is nothing to compare, the second says the comparison was attempted and did
not answer. The package built into the binary is not one of these — it
SHALL keep its own offline comparison against the embedded snapshot and
report **up to date** or **behind** from it.

#### Scenario: A plugin resolved at the ref's current head is up to date
- **WHEN** a plugin's locked marketplace revision equals the revision the
  marketplace's declared ref resolves to
- **THEN** it reports **up to date**, and no update is offered for it

#### Scenario: A plugin behind its ref reports how far behind
- **WHEN** the marketplace's declared ref has moved past the locked
  revision
- **THEN** it reports **behind**, with the number of commits between the
  two

#### Scenario: A plugin never compared reports so, and does not guess
- **WHEN** freshness has never been established for a plugin, or the last
  attempt to establish it failed
- **THEN** it reports **not checked**, never "up to date"

#### Scenario: A package installed from a path or URL reports unpinned
- **WHEN** a package was installed directly from a path or a Git URL rather
  than through a registered marketplace
- **THEN** it reports **unpinned**, never **not checked** and never
  **up to date**

#### Scenario: The built-in package still answers
- **WHEN** the package built into the binary differs from the snapshot this
  binary carries
- **THEN** it reports **behind**, offline, as it does today

#### Scenario: A plugin from a linked marketplace reports as linked
- **WHEN** a plugin's marketplace is linked to a checkout on this machine
- **THEN** it reports **linked**, naming the checkout, and no commit
  comparison is made or displayed for it

### Requirement: Establishing freshness is a write-free read on every read path

Reporting freshness SHALL NOT clone, fetch, install, or modify anything.
Every read surface — `uze status`, `uze plugin list`, `uze plugin inspect`,
the plugins screen, the overview — SHALL answer from what UZE has already
observed, and SHALL report **not checked** rather than reaching the network
to avoid saying so.

#### Scenario: A read path offline still answers
- **WHEN** the network is unavailable and a freshness-reporting command is
  run
- **THEN** the command succeeds, reports each plugin's last established
  freshness, and reports **not checked** for any plugin that has none

#### Scenario: A read path never writes
- **WHEN** any freshness-reporting command is run
- **THEN** no package bytes, no receipt, no lock and no manifest is
  modified by it

### Requirement: `uze update` moves a project's pins; `uze install` never does

The system SHALL provide `uze update [plugin]` at project scope. It SHALL
re-resolve the ref each affected marketplace declares, install what that
produced, and rewrite `agents.lock` with the revision it landed on. With no
argument it SHALL consider every plugin the manifest declares; with a
plugin name it SHALL consider only that one.

`uze install` SHALL continue to reproduce what `agents.lock` records and
SHALL NOT move a locked revision, so a clone of a project reaches the bytes
the project was locked at whatever has been pushed since.

`uze update` SHALL ask the same trust question an explicit
`uze plugin update` asks, per plugin, against the revision it replaces.

`uze update` SHALL NOT reach into machine state silently. Under the grammar
in force today (ADR-019) it is project-scoped: run outside a project, or
against a plugin the project does not declare, it SHALL fail with an error
naming `uze plugin update` as the machine-level equivalent — the rule
`uze remove` already holds against `uze plugin remove`. A grammar change
that makes the root verbs cover both scopes SHALL replace that failure with
a report of what it did to each scope, never with silence.

#### Scenario: Install reproduces a pin the ref has moved past
- **WHEN** a marketplace's declared ref has moved and `uze install` is run
- **THEN** the locked revision is installed unchanged, `agents.lock` is not
  rewritten, and the moved ref is reported as available rather than applied

#### Scenario: Update moves the pin and records where it landed
- **WHEN** `uze update` is run and a declared ref has moved
- **THEN** the plugin is re-resolved at the ref's current head, installed,
  and `agents.lock` records that revision

#### Scenario: Update of one plugin leaves the others pinned
- **WHEN** `uze update <plugin>` is run
- **THEN** only that plugin's marketplace entry and integrity are rewritten,
  and every other lock entry is byte-identical

#### Scenario: Update never changes a scope it did not name
- **WHEN** `uze update` is run where there is no project, or names a plugin
  the project does not declare
- **THEN** under ADR-019's grammar it fails without touching machine state
  and the error names `uze plugin update`; under a grammar where the root
  verbs cover both scopes it reports which scopes it changed

#### Scenario: Update refuses a revision that asks to execute something new
- **WHEN** a revision `uze update` would install introduces an executable
  capability the installed revision did not have
- **THEN** the installed revision is left in place, the plugin is reported
  as needing an explicit decision, and the rest of the update proceeds

### Requirement: A linked marketplace is never the source of a pin

While a marketplace is linked to a checkout on this machine, `uze update`
and `uze install` SHALL NOT write a revision resolved from that checkout
into `agents.lock`. The project's existing lock entry SHALL be left exactly
as it is, and the operator SHALL be told that the marketplace is linked and
therefore not a source of pins.

A linked marketplace's plugins SHALL NOT have their `integrity` checked:
the pin was taken from a commit, a working tree being edited cannot match
it by construction, and checking it would refuse exactly the plugin the link
exists to serve. The report SHALL say the check was not made and why.

A linked checkout's package content SHALL be what Git does not ignore:
files the author tracks, and files they have written but not yet committed.
A file the checkout's own ignore rules exclude SHALL NOT be ingested and
SHALL NOT cause a re-ingest, so an editor's temporary file or a build
artifact never reaches a harness as package content.

A linked checkout that cannot be read — a broken manifest, a conflict left
by an interrupted rebase — SHALL leave the Store's current bytes in place
and report the problem, never empty or half-fill the package.

#### Scenario: Reproducing a project whose marketplace is linked
- **WHEN** `uze install` reproduces a plugin whose marketplace is linked and
  the checkout's content differs from the lock's `integrity`
- **THEN** the plugin is ingested from the checkout, the integrity check is
  not applied, and the report says the marketplace is linked

#### Scenario: An ignored file is not package content
- **WHEN** a linked checkout's plugin directory gains a file its ignore
  rules exclude
- **THEN** nothing is re-ingested and the file reaches no harness

#### Scenario: An uncommitted file is package content
- **WHEN** a linked checkout's plugin directory gains a file that is neither
  ignored nor yet committed
- **THEN** it is ingested and every harness resolves it

#### Scenario: A broken linked checkout does not break the command
- **WHEN** a linked marketplace's checkout cannot be read
- **THEN** the Store keeps the bytes it has, every other marketplace
  resolves normally, and the problem is reported by name

#### Scenario: Updating a project whose marketplace is linked
- **WHEN** `uze update` is run and one of the project's marketplaces is
  linked
- **THEN** that marketplace's lock entry is unchanged, the plugins from it
  are re-ingested from the checkout, and the report names the link as the
  reason its pin did not move

### Requirement: Background work never refuses the operator's own action

Establishing freshness and applying an update SHALL NOT hold a mutation lock
while reaching the network. Acquisition happens outside it; the lock covers
only the write it protects.

Reporting what the client already knows SHALL NOT wait on either. The data a
screen draws SHALL arrive on its own, before and independently of any
freshness answer.

#### Scenario: An operator action during a background update is not refused
- **WHEN** the client is applying a background update and the operator runs a
  mutating command, in the client or in another terminal
- **THEN** that command is not refused for a lock held across a network
  transfer

#### Scenario: A screen does not wait on a clone
- **WHEN** the client opens and background freshness work is slow
- **THEN** the plugins screen draws its data without waiting for it

### Requirement: The client establishes freshness in the background and applies only what asks nothing

When the workspace client opens, the system SHALL establish freshness off
the render thread — no surface waits on it — and SHALL report each answer
as it arrives.

It SHALL apply, without asking, only a revision that introduces no
executable capability the installed revision did not already have. A
revision that introduces one SHALL be reported as available and SHALL NOT
be applied. What it applies is machine scope only: no project's
`agents.yaml` or `agents.lock` SHALL be written by it.

A failure to establish freshness for one plugin SHALL NOT stop the others
and SHALL NOT be reported as an error the operator must act on; it leaves
that plugin **not checked**.

#### Scenario: Opening the client never blocks on the network
- **WHEN** the client opens and a marketplace's source is slow or
  unreachable
- **THEN** every screen draws immediately with the freshness last
  established, and the answer replaces it when it arrives

#### Scenario: A Markdown-only revision applies on its own
- **WHEN** the worker finds a newer revision of an installed plugin whose
  capabilities are Skills alone
- **THEN** it is installed, and the operator is told what was updated

#### Scenario: A revision adding a hook or an MCP server is offered, not applied
- **WHEN** the worker finds a newer revision that adds a hook handler or an
  MCP server command the installed revision did not have
- **THEN** the installed revision is untouched, and the plugin is reported
  as having an update that needs an explicit decision

#### Scenario: The worker never rewrites a project
- **WHEN** the worker applies any update
- **THEN** no `agents.yaml` and no `agents.lock` in any project is modified
  by it

### Requirement: A freshness state is reported as one of five things, everywhere

Every surface that reports a plugin's freshness SHALL report exactly one of
**up to date**, **behind**, **linked**, **unpinned**, or **not checked**,
SHALL NOT render "not checked" the same way it renders "up to date", and
SHALL carry when the answer was established wherever it claims one.

#### Scenario: The plugins screen distinguishes unknown from current
- **WHEN** the plugins screen lists an installed plugin whose freshness has
  never been established, beside one established as current
- **THEN** the two rows read differently

#### Scenario: The same state reads the same in the CLI and the client
- **WHEN** a plugin's freshness is reported by the CLI and by the plugins
  screen
- **THEN** both name the same one of the five states

#### Scenario: "up to date" never appears without its date
- **WHEN** a plugin is reported **up to date** from an answer established
  earlier
- **THEN** when that answer was established is reported beside it
