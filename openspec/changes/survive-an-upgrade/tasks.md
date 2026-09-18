## 1. The inventory, before anything is classified

- [ ] 1.1 List every file UZE writes for itself, machine and project: the state
  directory's documents, `state/terminal/workspace.json`, `state/install.json`,
  `state/update.json`, `runtime/projects/<id>/`, the caches
  (`harness_detection`, inspection, marketplace catalogue), the keymap, the
  conversation index, prompt history, client layout — with, for each, whether it
  declares a version, what reads it, what happens today when it cannot be read
- [ ] 1.2 List every resource whose *location* is computed: the terminal
  endpoint, the generated attachment directories, the shims, the cache paths —
  and the anchor each can be named through when a build computes a different one
- [ ] 1.3 Classify each (rebuildable / irreplaceable / located) from 1.1 and 1.2,
  and record the answers that disagree with their class today — `profile_state`
  and `theme_state` refuse where the table would rebuild; `conversation`,
  `client_layout`, `workspace.json` and `detection_cache` default silently and
  let the next save overwrite the bytes

## 2. The rule set, written where a new file will meet it

- [ ] 2.1 Add the classes, the direction rule and "an absent version is version
  1" to `docs/architecture/invariants.md`, each tied to the test that holds it
- [ ] 2.2 Name the rule in the module doc of each writing surface the inventory
  found — `uze-core`'s `persistence`, `uze-terminal`'s own atomic write,
  `self_update`'s — since there is no single chokepoint every document passes
  through
- [ ] 2.3 State in `AGENTS.md` that a new persisted document declares its class
  and carries an upgrade scenario

## 3. The audit: every document UZE writes for itself

- [ ] 3.1 Give every document a `schema_version` and a probe that reads it before
  the document, reading an absent field as 1 so no existing machine is set aside
  on first run
- [ ] 3.2 Apply the direction rule everywhere a version is compared: older is set
  aside by its class's rule, newer is refused and left untouched
- [ ] 3.3 Give every rebuildable document the set-aside recovery, and decide the
  rule for the ones with no write lock of their own (`client_layout`,
  `conversation`, `workspace.json`) — the judgement needs a held document, and a
  retry loop is not an answer this project accepts
- [ ] 3.4 Make the irreplaceable ones refuse rather than default, where they do
  not already, and make the ones that refuse today but are rebuildable recover
- [ ] 3.5 An architecture test that finds the writers rather than a list somebody
  maintains — the way `tests/architecture/layering.rs` greps sources — so a new
  persisted document fails the build until it is classified

## 4. Located resources stay addressable

- [ ] 4.1 Close the gaps 1.2 found, following the claim-names-its-holder shape
  the terminal runtime uses
- [ ] 4.2 Decide what a generated projection does when the generator changed: a
  receipt carries content identity, not generator version, so an old wrapper
  stays `Matched` and is never rebuilt
- [ ] 4.3 Decide whether the self-updater may replace the binary while this
  process holds a server or a mutation lock, or state the window as accepted
- [ ] 4.4 Test per resource: a previous build's artifact at a location this build
  does not compute is still named, reported and removable

## 5. `uze doctor` reports what the previous version left

- [ ] 5.1 Decide how `doctor` learns about the terminal: `uze-application` does
  not depend on `uze-terminal` today, so this is a new crate edge (with the
  LikeC4 note the project's rules require) or the claim's location known to core
- [ ] 5.2 Each area carries its own upgrade leftovers beside its existing error
  fields — documents set aside, state of an unknown schema, a workspace held at
  an unreachable endpoint
- [ ] 5.3 One summary line at the top counting them, so an operator finds them
  right after an update without reading the whole report
- [ ] 5.4 Every leftover carries its remedy, in the shape
  `QuarantinedRegistration::REMEDY` already uses
- [ ] 5.5 Bound what accumulates: a set-aside document per event, kept forever,
  is its own mess

## 6. The upgrade tier

- [ ] 6.1 Teach the journey runner to download and cache the released `uze` for
  the runner's platform (the release publishes one asset per target), and to
  expose it to a journey beside `{uze}`
- [ ] 6.2 Give the chapter a world root short enough that `$UZE_HOME` can hold
  the endpoint, or the endpoint does not move inside a journey and the scenario
  passes for a reason that does not exist on a real machine
- [ ] 6.3 Chapter `07-upgrade`: the released binary creates a workspace, agents
  and checkouts; this build opens the same machine; the branches, commits and
  checkouts are still there
- [ ] 6.4 Journey: the released binary leaves a server running; against
  `alpha.6` this build reports what it cannot reach and the socket stays where
  the old release put it — checked on the process table, `workspace.json` and the
  socket's directory, never on the screen
- [ ] 6.5 Journey: the older binary reads a document this build wrote — the case
  two binaries on one machine actually produce — and leaves it untouched
- [ ] 6.6 Tag the chapter nightly (it needs the network and a cached download)
  and say in `journeys/README.md` what it costs and why it is not on the gate

## 7. Close the loop

- [ ] 7.1 Run the chapter against the current release; separate what must pass
  from what can only be reported until a release carries the mechanism
- [ ] 7.2 Record each property the chapter holds in
  `docs/architecture/invariants.md` — which needs a Rust test reading the
  journey's evidence, or a second citation form taught to
  `invariants_citations.rs`, since a `.yml` cannot be cited today
