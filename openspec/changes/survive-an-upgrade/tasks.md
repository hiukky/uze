## 1. The rule set, written where a new file will meet it

- [ ] 1.1 Add the three classes (rebuildable, irreplaceable, located) and the
  answer each owes to `docs/architecture/invariants.md`, each tied to the test
  that already holds it
- [ ] 1.2 Name the classification in `crates/uze-core/src/delivery/persistence.rs`'s
  module doc — the module every persisted document already goes through — so the
  rule is met where a document is written, not only in a document about documents
- [ ] 1.3 State in `AGENTS.md` that a new persisted document declares its class
  and carries an upgrade scenario

## 2. The audit: every document UZE writes for itself

- [ ] 2.1 Inventory every file UZE persists under `$UZE_HOME` and per project,
  with its class, whether it declares a version, and whether the version is read
  before the document (task, client_layout, prompt_history, conversation,
  delivery::state, package::store, theme_state, profile_state, attachments,
  marketplaces, provisioning, update, integrations)
- [ ] 2.2 Give every rebuildable document a `schema_version` and a probe that
  reads it before the document, the way `task::read_document` does
- [ ] 2.3 Give every rebuildable document the set-aside recovery under its own
  write lock, and a test per document that an unreadable one is set aside with
  its bytes kept and the work carries on
- [ ] 2.4 Confirm every irreplaceable document refuses rather than recovers, and
  add the test where one is missing
- [ ] 2.5 Fail the build on a persisted document that declares no class: an
  architecture test listing the known documents, so a new one has to be
  classified to compile the suite

## 3. Located resources stay addressable

- [ ] 3.1 Inventory the resources whose location is computed (terminal endpoint,
  generated attachment directories, caches, shims) and name the anchor each is
  reachable through when a build computes a different location
- [ ] 3.2 Close the gaps found, following the claim-names-its-holder shape the
  terminal runtime now uses
- [ ] 3.3 Test per resource: a previous build's artifact at a location this build
  does not compute is still named, reported and removable

## 4. `uze doctor` reports what the previous version left

- [ ] 4.1 Read models: each area carries its own upgrade leftovers (documents set
  aside, state of an unknown schema, a workspace held at an unreachable endpoint)
  beside its existing error fields
- [ ] 4.2 One summary line at the top of the report counting them, so an operator
  finds them right after an update without reading the whole report
- [ ] 4.3 Every leftover carries its remedy, in the shape
  `QuarantinedRegistration::REMEDY` already uses
- [ ] 4.4 Tests: a machine with a set-aside document, and one with a server at an
  unreachable endpoint, are both named by `doctor` with their remedy

## 5. The upgrade tier

- [ ] 5.1 Teach the journey runner to fetch and cache the last released `uze`
  binary for the world's platform, and to expose it to a journey as a second
  binary beside `{uze}`
- [ ] 5.2 Chapter `07-upgrade`: the released binary creates a workspace, agents
  and checkouts; this build opens the same machine; the branches, commits and
  checkouts are still there and an agent can be created with no intervening step
- [ ] 5.3 Journey: the released binary leaves a server running; this build attaches
  and the workspace comes back, without the operator finding a process
- [ ] 5.4 Journey: the released binary's project state meets a schema this build
  does not know; the document is set aside, the checkouts are re-adopted, and
  `doctor` names what was set aside
- [ ] 5.5 Tag the chapter for the nightly cadence (it needs the network and a
  cached download), and state in `journeys/README.md` what it costs and why it is
  not on the gate

## 6. Close the loop

- [ ] 6.1 Run the chapter against the current release and fix what it finds
- [ ] 6.2 Record in `docs/architecture/invariants.md` each property the chapter
  now holds, tied to the journey that proves it
