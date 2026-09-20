## 1. The inventory, before anything is classified

- [x] 1.1 List every file UZE writes for itself, machine and project, with
      its current writer, its current version policy and whether it has one.
- [x] 1.2 List every resource whose *location* is computed — the terminal
      endpoint, generated directories, cache paths — and what finds it.
- [x] 1.3 Classify each as bytes, record, generated or remembered, by what
      deleting it costs, and record for each record what the world still
      says and what only the record knows.

## 2. The rule set, written where a new file will meet it

- [x] 2.1 Write the tiers, the direction rule, "an absent version is
      version 1", and "carrying across is silent; setting aside is the
      floor" in `persistence`'s module doc.
- [x] 2.2 Name the rule in the module doc of each writing surface the
      inventory found.
- [x] 2.3 State in `AGENTS.md` that a new persisted document declares its
      tier and its shape, and what each tier owes.

## 3. One map, one writer

- [x] 3.1 Name in `UzeHome` every path built inline today: `attachments.json`,
      `mutation.lock`, the superseded directory, `prompt-history/`,
      `logs/`, and the binary's own ledger.
- [x] 3.2 Remove `self_update.rs`'s private `read_json`/`write_json`;
      route it through `persistence::write_atomic` and `UzeHome`.
- [x] 3.3 An architecture test that *finds* the writers rather than
      checking a list somebody maintains, failing on a write to a path
      under UZE's home that `UzeHome` does not name; `uze-terminal` is
      sanctioned by name, since it depends on no UZE crate by design.

## 4. The envelope, the ladder and the floor

- [x] 4.1 Add the versioned envelope and the one read path: equal reads,
      lower climbs and is rewritten silently, higher is left alone and
      reported, unreadable falls to the floor.
- [x] 4.2 Give every record a shape read separately, before the document,
      the way `task.rs` reads it — so a required field can never kill the
      guard that exists for it.
- [x] 4.3 Register ladder steps as functions from one shape to the next,
      with the rule that a step is deletable once no machine can be below
      it.
- [x] 4.4 Write the step this change exists for: workspace v1→v2 drops the
      per-space `kind`.
- [x] 4.5 Apply the direction rule everywhere a version is compared.
- [x] 4.6 Keep the floor: a record that cannot be carried across is set
      aside, reconstructed from what the world knows, and reported —
      `task.rs`'s adoption from the checkouts on disk is the model.
- [x] 4.7 Make the irreplaceable ones refuse rather than default, where
      they default today.
- [x] 4.8 Make `cache/` and `runtime/` carry no shape: unreadable is
      discarded and produced or observed again, silently.
- [x] 4.9 Remove the four hand-written policies from `task.rs`,
      `runtime.rs`, `conversation.rs` and the theme loader.
- [x] 4.10 Test each arm, including two builds run alternately proving the
      newer one's records survive every run of the older one.

## 5. The tiers, drawn on disk

- [x] 5.1 Move generated harness content from `state/attachments/` to
      `runtime/attachments/<harness>/`, and `logs/` to `cache/`.
- [x] 5.2 Leave `shims/` at the root, with the reason in `home.rs`: it is
      the directory that goes on `PATH`.
- [x] 5.3 Move `integrations.json` to `cache/harnesses.json`, carrying no
      shape, and leave `provisioning.json` a record in `state/`. The merge
      this change first proposed does not hold either: a `ProvisioningRecord`
      is `action`, `status`, `method` and `recorded_at_unix_secs` — the
      history of an attempt UZE made, which no probe re-derives. The two
      `version` fields are not one fact duplicated: the integration's is
      *what is there now*, the provisioning record's is *what this attempt
      put there*. Different tiers may not share a document, and the tier is
      the stronger rule.
- [x] 5.4 Keep `install.json` and the updater's ledger apart, and route
      both through the map and the one writer. The merge this change first
      proposed does not hold: `install.json` is written by `install.sh`, so
      it is an *inbound* receipt from the installer — a different writer and
      a different lifetime from UZE's own memory of its update checks. Two
      documents with two writers are two documents, for the same reason
      `marketplaces.json` and `packages.json` stay apart.
- [x] 5.5 Correct the tier reasoning in `home.rs`'s doc comments.
- [x] 5.6 Test that deleting `runtime/` and `cache/` costs nothing.

## 6. A project is one directory

- [x] 6.1 Add `state/projects/<id>/project.json` naming the canonical root,
      and make it the one thing a sweep reads to resolve a project.
- [x] 6.2 Move the task store to `agents.json`, conversations to
      `conversations/<agent>.json`, prompt history to
      `prompt-history.jsonl`, each under that directory — each move a
      ladder step, so nothing is lost by moving.
- [x] 6.3 Make forgetting a project the removal of that one directory.
- [x] 6.4 Test that a project whose recorded root no longer exists is still
      readable and still names that root.

## 7. The receipt ledger

- [ ] 7.1 Remove the composite receipt key — unparseable because the
      resource identity carries colons of its own, and read by nobody — and
      make the ledger a list.
- [ ] 7.2 Key idempotent upsert on the fields that identify a receipt.
- [ ] 7.3 Test that recording the same attachment twice leaves one receipt.

## 8. Located resources stay addressable

- [ ] 8.1 Close the gaps 1.2 found, following the claim-names-its-holder
      shape: the claim records its holder, the endpoint lives beside the
      workspace it serves.
- [ ] 8.2 Decide what a generated projection does when the generator
      changed: regenerate, or report.
- [ ] 8.3 Decide whether the self-updater may replace the binary while a
      client runs, and never start a server from an image that is gone.
- [ ] 8.4 Test per resource: a previous build's artifact at a location this
      build does not compute is still found, reported and ended.

## 9. The runtime can reach the screen

- [ ] 9.1 Add the protocol event by which the runtime says it could not
      carry the workspace across, and raise it as a toast beside the
      adopted task store's.
- [ ] 9.2 Answer a mismatched `PROTOCOL_VERSION` with an error naming both
      versions instead of falling to `_ => None`, and tell the operator
      what to do.
- [ ] 9.3 Test that a first run with nothing persisted reports nothing.

## 10. `uze doctor` reports what the previous version left

- [ ] 10.1 Decide how `doctor` learns about the terminal, which
      `uze-application` does not depend on today.
- [ ] 10.2 Each area carries its own upgrade leftovers beside its existing
      errors.
- [ ] 10.3 One summary line counting them, so an operator finds them
      without reading everything.
- [ ] 10.4 Every leftover carries its remedy.
- [ ] 10.5 Bound what accumulates: a set-aside document per event, kept
      forever, is its own leak.

## 11. Preserved work answers for the machine

- [x] 11.1 Add the reader that sweeps `state/projects/`, answering each
      project's agents with the root its `project.json` names, and
      reporting an unreadable project without withholding the rest.
- [x] 11.2 Filter to preserved work from the record alone — stored state
      neither integrated nor closed, isolation present — with no Git read
      per project.
- [x] 11.3 Add the read model and the `UzeApplication` method presentation
      consumes; nothing in `src/` reaches past it.
- [x] 11.4 Draw `preserved_tasks` from it instead of `remembered.tasks`,
      scheduling the read off the UI thread and keeping the last answer
      drawn while one is in flight.
- [x] 11.5 Name each row's project in `render_preserved`, through the
      widget vocabulary and `theme` tokens.
- [x] 11.6 `TestBackend` tests: a project with no space open appears; two
      projects sharing a branch name are distinguishable; delivered and
      discarded work does not appear.

## 12. Resume lands where the work belongs

- [x] 12.1 Resolve the target space by canonical root through the runtime's
      existing `space_for`, preferring the selected space when several
      match and otherwise the first.
- [x] 12.2 Select that space before opening the tab.
- [x] 12.3 Open a space with `Seating::Open` at the project's seat when
      none matches, sequencing the tab on the session update that names it.
- [x] 12.4 Leave a space rooted above the project unmatched, with the
      reason recorded beside the code.
- [ ] 12.5 Refuse a resume whose project directory no longer exists:
      nothing opens, the reason is said, the entry stays.
- [x] 12.6 Tests: an open space receives the tab and no second space is
      created for the same root; a closed-and-reopened space matches
      however it is named; the space the operator was looking at receives
      nothing.

## 13. The upgrade tier

- [ ] 13.1 Teach the journey runner to download and cache the released
      `uze` for the platform.
- [ ] 13.2 Give the chapter a world root short enough that `$UZE_HOME` can
      hold a socket.
- [ ] 13.3 Chapter `07-upgrade`: the released binary creates a workspace,
      agents and checkouts; this build opens on the same machine; the
      spaces, branches and commits are still there and an agent can be
      created with no intervening step.
- [ ] 13.4 Journey: the released binary leaves a server running; this build
      meets it and neither hangs up in silence nor needs a reboot.
- [ ] 13.5 Journey: the older binary reads a document this build wrote, and
      leaves it untouched.
- [ ] 13.6 Tag the chapter nightly — it needs the network and a cached
      download.

## 14. Recovery, from the operator's side

- [ ] 14.1 A journey in `06-recovery`: work is left in a space, the space
      is closed, and the preserved list still names it.
- [ ] 14.2 A scene continuing it: resuming opens a space rooted at its
      project and the agent runs in its own checkout.
- [ ] 14.3 Tag `gate`, give every gesture its `expect`, and point `proves:`
      at the pages they back.

## 15. Close the loop

- [ ] 15.1 Clean the records under `~/.uze` that predate the ladder's first
      step — pre-1.0, stale state is cleaned, never carried.
- [ ] 15.2 Run the upgrade chapter against the current release; separate
      what must pass now from what becomes provable only from the next
      release on.
- [ ] 15.3 Record each property in `docs/architecture/invariants.md` with
      the test that holds it, and update `AGENTS.md`'s Workspace layout.
- [ ] 15.4 Run the full gate set — `cargo fmt --check`, `clippy
      --all-targets -D warnings`, the workspace suite, coverage,
      `cargo deny check`, `openspec validate --all --strict`, and
      `journey validate journeys/suites`.
