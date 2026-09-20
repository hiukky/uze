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

- [x] 7.1 Remove the composite receipt key — unparseable because the
      resource identity carries colons of its own, and read by nobody — and
      make the ledger a list.
- [x] 7.2 Key idempotent upsert on the fields that identify a receipt.
- [x] 7.3 Test that recording the same attachment twice leaves one receipt.

## 8. Located resources stay addressable

- [x] 8.1 Nothing to do: the inventory found no open gaps. The claim
      already records its holder (`record_claimant`), the endpoint is
      already computed from the workspace's own directory with fallbacks
      (`socket_path`), and a server is already never started from a
      replaced image (`server_executable`). These are the three incidents
      the absorbed change was written from, each fixed where it was found
      before this one began.
- [x] 8.2 Decided: regenerate, and say nothing. A projection whose root is
      gone, or whose marker cannot be read, is swept and rebuilt on the next
      launch in that project — `prune_projections` already held that, and it
      is the generated tier's rule. Moving generated harness content under
      `runtime/` made the sweep's one-tenant assumption wrong, which would
      have deleted a live delivery on the next `doctor`; `attachments` is
      now a tenant of its own, because its lifetime is the attachment's and
      the receipt ledger is what answers for it.
- [x] 8.3 Decided, and already holding: it may. `server_executable`
      falls back to `uze` as `PATH` resolves it when `current_exe` no
      longer exists — which is the binary the operator just installed, and
      the one they want serving anyway — and says so plainly when neither
      exists.
- [x] 8.4 Already covered, one test per resource:
      `a_server_answering_at_no_endpoint_this_build_names_is_still_stopped`,
      `a_claim_this_build_cannot_name_is_reported_rather_than_called_stopped`,
      `a_server_of_another_build_is_retired_and_lets_go_of_the_workspace`,
      `two_terminals_that_disagree_about_the_environment_share_one_endpoint`
      and `a_stale_socket_is_reclaimed_by_the_server_that_binds`.

## 9. The runtime can reach the screen

- [x] 9.1 Add the protocol event by which the runtime says it could not
      carry the workspace across, and raise it as a toast beside the
      adopted task store's.
- [x] 9.2 Nothing to do: a mismatched `PROTOCOL_VERSION` is already
      answered, and has been since `fail closed on hooks, delivery, updates
      and the terminal runtime` (ba0448f0, 2026-09-13) — before the incident
      this change is named for. The runtime replies "incompatible terminal
      runtime protocol", `serves_this_build` reads that as "cannot serve
      this build", and the client retires the old runtime and starts one
      that can. `_ => None` catches a first message that is not an `Attach`
      at all, which is a different case. The claim that the client was hung
      up on in silence was wrong: the protocol half of the incident worked
      as designed, and the workspace half is what cost the spaces.
- [x] 9.3 Test that a first run with nothing persisted reports nothing.

## 10. `uze doctor` reports what the previous version left

- [x] 10.1 Decided: it does not ask. It sweeps the filesystem for what
      was set aside, which answers for the terminal runtime without
      reaching it — `uze-terminal` depends on nothing here by design, and
      the point of this report is to find what nobody is asking about.
- [x] 10.2 Each area carries its own upgrade leftovers beside its existing
      errors.
- [x] 10.3 One summary line counting them, so an operator finds them
      without reading everything.
- [x] 10.4 Every leftover carries its remedy.
- [x] 10.5 Bounded in the report rather than on disk: the newest five are
      named with their remedy and the rest are counted. The bytes are never
      removed by UZE — a record it could not read is still not one it may
      throw away — so the operator is told how many there are and what to
      do, and decides.

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
- [x] 12.5 Refuse a resume whose project directory no longer exists:
      nothing opens, the reason is said, the entry stays.
- [x] 12.6 Tests: an open space receives the tab and no second space is
      created for the same root; a closed-and-reopened space matches
      however it is named; the space the operator was looking at receives
      nothing.

## 13. The upgrade tier

- [x] 13.1 Teach the journey runner to download and cache the released
      `uze` for the platform.
- [x] 13.2 Nothing to do: the worlds live under `/tmp/uze-journeys/<slug>`
      and `socket_path` already falls back when a home is too long for a
      socket path. The chapter's own runs bind in the workspace's own
      directory, which is the first candidate.
- [x] 13.3 Chapter `07-upgrade`: the released binary creates a workspace,
      agents and checkouts; this build opens on the same machine; the
      spaces, branches and commits are still there and an agent can be
      created with no intervening step.
- [ ] 13.4 Journey: the released binary leaves a server running; this build
      meets it and neither hangs up in silence nor needs a reboot. Left for
      a follow-up: the retire-and-replace path it would exercise is already
      held by five runtime tests, and driving two live servers from one
      journey is the kind of thing that makes a suite flaky before it makes
      it truthful.
- [x] 13.5 Journey: the older binary reads a document this build wrote, and
      leaves it untouched.
- [x] 13.6 Tag the chapter nightly — it needs the network and a cached
      download.

## 14. Recovery, from the operator's side

- [x] 14.1 A journey in `06-recovery`: work is left in a space, the space
      is closed, and the preserved list still names it.
- [x] 14.2 A scene continuing it: resuming opens a space rooted at its
      project and the agent runs in its own checkout.
- [x] 14.3 Tag `gate`, give every gesture its `expect`, and point `proves:`
      at the pages they back.

## 15. Close the loop

- [x] 15.1 Nothing to clean by hand, which is the point: the ladder
      carries every record the previous layout held, on the first write
      after the upgrade, and takes the dead per-project lock and the three
      emptied directories with it. What is left orphaned is `cache`- and
      `runtime`-tier — `state/attachments/`, `state/integrations.json`,
      `state/logs/` — which the next command regenerates or re-observes
      elsewhere and which cost nothing where they lie. The rule that stale
      state is cleaned rather than carried still holds; it simply has
      nothing to act on, because carrying is now possible.
- [x] 15.2 Run against `v0.0.0-alpha.6`, and it separates itself: the
      release predates `default: isolated`, so the world speaks the
      manifest the *release* understands, and this build places its own
      agent in the project's root because that manifest asks for it. What
      is provable now is that the release's record is carried into this
      layout and its work survives; the isolated-by-default path becomes
      provable from the next release on.
- [x] 15.3 Record each property in `docs/architecture/invariants.md` with
      the test that holds it, and update `AGENTS.md`'s Workspace layout.
- [x] 15.4 Run the full gate set — `cargo fmt --check`, `clippy
      --all-targets -D warnings`, the workspace suite, coverage,
      `cargo deny check`, `openspec validate --all --strict`, and
      `journey validate journeys/suites`.
