## 1. Unblock delivery

- [x] 1.1 `attach_symlink` adopts a reference that resolves to nothing and
      keeps preserving one that resolves; two tests covering both sides
      (`crates/uze-core/src/delivery/exposure.rs`)
- [x] 1.2 An install whose delivery hits one preserved reference reports that
      capability and delivers the rest, instead of failing the command
      (`lifecycle/attach.rs`, test in `tests/lifecycle/`)
- [x] 1.2b Adoption keys on `ErrorKind::NotFound`, never on `exists()`,
      which swallows every error — an unreadable target (permission, an
      unmounted volume, a dead network mount) is preserved, with its own test
- [x] 1.3 Sweep: find every reference in a harness discovery root that
      resolves to nothing, points inside `$UZE_HOME`, and no receipt claims.
      `IntegrationPort` exposes only `shared_agent_skill_root`
      (`delivery/integration.rs:376`), so this either adds a
      `discovery_roots()` to the port or is scoped to that shared root —
      decide and say which in the spec
- [x] 1.4 `uze doctor` reports the sweep's findings and removes them on the
      explicit maintain path; a resolving reference and one outside
      `$UZE_HOME` are never reported (`application/doctor.rs`,
      `application/maintenance.rs`)

## 2. The marketplace cache becomes a repository

- [x] 2.1 Clone a Git marketplace into `cache/` with `--filter=blob:none
      --no-checkout`, keeping the repository; `Meta` records the resolved
      commit (`marketplace_catalogue.rs`)
- [x] 2.2 Acquisition from a cached marketplace is `fetch` + sparse checkout
      of the plugin's `source` subdirectory, never a second clone
      (`marketplace.rs::materialize_plugin`, `acquisition/git.rs`)
- [x] 2.3 The cache stops materializing a working tree: `copy_tree` into
      `<name>/checkout/` goes away, and `marketplace.json` is read from the
      repository at the resolved commit rather than from a copied file
      (`marketplace_catalogue.rs:168-226`). A local `path:` marketplace is
      still read in place and still caches nothing
- [x] 2.3b An existing `<name>/checkout/` is deleted, never migrated — the
      cache tier declares no shape and is observed again (AGENTS.md, "Only
      records declare a shape"). Measured on the operator's machine today:
      756 KB per marketplace, of which 684 KB is content no install touches
      and 36 KB is a second copy of the installed plugin
- [x] 2.3c Test: after installing a plugin, its bytes exist materialized in
      exactly one place — the Store
- [x] 2.4 What enters the Store still carries no `.git`; a stored package
      reads with the cache deleted (test)
- [x] 2.5a A commit that is not the head still resolves and materializes, and
      one the mirror already holds needs no network at all — which is what
      makes reproducing `agents.lock` work offline
      (`mirror::a_commit_the_mirror_already_holds_needs_no_network`)
- [ ] 2.5b A source refusing `--filter` still works end to end (a server
      without `uploadpack.allowFilter` ignores it); Git version floor for
      the mirror reported by `doctor` rather than assumed
- [ ] 2.6 Measure: `market add` + two installs from one marketplace opens one
      connection, not three (`performance_tests.rs`)

## 3. Freshness, computed once

- [x] 3.1 `Freshness` read model: `UpToDate`, `Behind { commits }`,
      `Linked { checkout }`, `Unpinned`, `NotChecked` — each carrying when it
      was established (`application/read_models.rs`)
- [x] 3.2a The mirror records the head its declared ref resolved to; the
      comparison is that against the installed commit — a JSON read, because
      one `rev-parse` per package put the machine snapshot over its budget
      (`performance_tests::machine_snapshot_meets_the_budget` caught it)
- [ ] 3.2b The commit distance, where the detail view can afford to ask for
      it: `mirror::distance` is written and tested, and nothing calls it yet
- [x] 3.3 The embedded package keeps `bootstrap::has_update` and maps onto
      `UpToDate`/`Behind`; a `local`-marketplace package reports `Unpinned`
- [x] 3.4 Remember the answer under `cache/` — done by the mirror's own
      `catalogue.json`, which already records the commit and when it was
      fetched, so no new path and no new record were needed
- [x] 3.5 Replace `update_available` on both summaries with the freshness
      state; update every consumer
- [ ] 3.6 Every read surface reports the state and its date, writes nothing,
      and answers offline (`tests/cli/`)
## 4. `uze update`

- [x] 4.1 `Project::update(root, plugin: Option<&str>, authority)` —
      re-resolve the declared ref through `resolve_into_lock`, rewrite
      `agents.lock`, reconcile the context, per-plugin trust
      (`application/project_environment.rs`)
- [x] 4.2 `uze update [plugin]` in the CLI, project-scoped per ADR-019, with
      text and JSON output (`src/main.rs`)
- [x] 4.3 Classify it in `src/command_performance.rs` — `JustifiedSlow`,
      with the reason
- [x] 4.4 Tests: install leaves a moved ref pinned; update moves it and
      records where it landed; updating one plugin leaves every other lock
      entry byte-identical; a revision introducing execution is refused and
      the rest proceed (`tests/lifecycle/`)

## 5. Linked marketplaces

- [x] 5.1 The link record in machine state — marketplace name to checkout
      path — with its own path in `UzeHome` (`delivery/state.rs`)
- [x] 5.2 `uze market link <name> <path>` / `uze market unlink <name>`;
      linking refuses a checkout whose repository identity is not the
      marketplace's (`src/main.rs`, `application/marketplace.rs`)
- [x] 5.3 Split `resolve_into_lock`: acquiring and installing stays shared by
      `add`/`install`/`update`; what is written to the lock becomes the
      caller's, so a linked marketplace writes nothing without a branch
      inside the shared function
- [x] 5.3b Resolution prefers the linked checkout's working tree, re-ingesting
      when its content differs from the Store's; no network, no commit
      (`application/project_environment.rs`)
- [x] 5.3c A linked checkout's content is what Git does not ignore — tracked
      plus untracked-not-ignored — for both the digest and the ingest, so an
      editor's temporary file or a build artifact never becomes package
      content. Tests for both sides
- [ ] 5.3d An mtime/size pass short-circuits before the digest, so an
      unchanged linked checkout costs a stat per file, not two full tree
      reads (`digest.rs:54-90`)
- [x] 5.4 `install` and `update` never write a lock entry resolved from a
      linked checkout, and say why the pin did not move
- [x] 5.5 `market list`, `market inspect` and the plugins screen say a
      marketplace is linked and to where
- [x] 5.6 Tests: an edit in the checkout reaches the Store with no commit;
      the lock is byte-identical across an update while linked; linking to a
      foreign repository is refused

## 6. Marketplace honesty

- [x] 6.1 `install` and `update` skip a plugin whose marketplace this
      machine cannot reach, name every skip in their own report, and succeed;
      a reachable marketplace that fails is still an error
      (`application/project_environment.rs`)
- [x] 6.1b `uze status` reports a project declaring a marketplace with no
      resolvable remote as not fully reproducible elsewhere, naming the
      marketplace and its plugins
- [x] 6.2 Classify an acquisition refused on credentials and report the
      marketplace, the URL and that it is an access question
      (`uze-core::error`, `package/acquisition/`)
- [ ] 6.3 Tests: a clone with one unreachable and one reachable marketplace
      gets the reachable half and a named skip; a local-only marketplace
      still installs, delivers and removes normally on the machine that has
      it; a reachable marketplace that fails still fails the command
- [ ] 6.4 `uze --help` distinguishes the three update verbs in one line
      each — `update` (this project's pins), `plugin update` (one package on
      this machine), `self-update` (the binary); making `self-update` visible
      is task 4.3 of `keep-the-installed-binary-current`
- [ ] 6.5 `uze update` outside a project, or naming a plugin the project does
      not declare, fails with an error naming `uze plugin update`, the way
      `uze remove` already names `uze plugin remove` (ADR-019 §3)

## 7. The client's freshness worker

- [x] 7.1 `Plugins::update` resolves *outside* `MutationLock`; the lock
      covers ingest and attach only (`lifecycle/update.rs:17,29`)
- [x] 7.2 Split `spawn_startup`: `Refreshed` is sent before any freshness
      work, which arrives as its own later message
      (`src/ui/worker.rs:560-590`)
- [ ] 7.3 `spawn_*`/`absorb_*` pair for the freshness refresh; every answer
      carries its question, a late one is dropped
- [x] 7.4 Widen `auto_update` past `Embedded`, and rewrite
      `auto_update_never_re_resolves_a_source_it_would_have_to_fetch`
      (`application/tests.rs:1104`) to hold what still stands: no CLI
      dispatch path re-resolves a network source
- [x] 7.5 Apply only a revision introducing no executable capability the
      installed one lacked; anything crossing the trust boundary is reported
      and never applied; no `agents.yaml`/`agents.lock` is written
- [ ] 7.6 A mutating operator action during a background update is not
      refused (test)
- [ ] 7.7 Outcomes arrive as toasts, never in the header
- [x] 7.8 Plugins screen reads each row's state; "not checked", "unpinned"
      and "up to date" read differently (`TestBackend` tests)
- [x] 7.9 The architecture suite still passes

## 8. Journeys

Per `journeys/README.md`: every `then` reads the filesystem, Git or the
process table — never UZE's own report — and screen text is a gate, never an
assertion.

- [ ] 8.1 `journeys/worlds/`: a verb for "this fixture is a repository with N
      commits". Three of the journeys below need it, which is the project's
      own threshold for lifting a world
- [ ] 8.2 `02-packages/05-the-author-edits-and-the-harness-follows.yml` —
      linking adopts the checkout (and UZE runs no Git on it: `git status
      --porcelain` stays empty) · an edit reaches the harness with `rev-parse
      HEAD` unchanged · `uze update` leaves `agents.lock` byte-identical
- [ ] 8.3 `02-packages/06-a-pin-the-ref-moved-past.yml` — `install`
      reproduces commit A after the marketplace moved to B · `update` moves
      it, checked against `git rev-parse` · updating one plugin leaves the
      other lock entries byte-identical
- [ ] 8.4 `02-packages/07-a-marketplace-this-machine-cannot-reach.yml` — the
      reachable half installs, the unreachable is named, exit 0 · `status`
      says the project does not fully reproduce
- [ ] 8.5 `06-recovery/08-a-reference-into-a-path-that-moved.yml` — **the
      reported bug**: a reference repointed by hand at a path that does not
      exist is adopted and the install succeeds · one repointed at content
      that exists is preserved untouched
- [ ] 8.6 `01-first-run/04-what-the-plugins-screen-says-about-age.yml`
      (`[tui]`, nightly) — the gesture proves the route opened; the claim is
      `plugin list --format json` beside `git rev-list --count`
- [ ] 8.7 Update the journeys the grammar change breaks — `02-packages/01,
      02, 03`, `03-context/01`, `06-recovery/01`, `01-first-run/01` — once
      that change lands; `06-recovery/01` proves machine-scope removal and
      keeps its claim through `uze remove <p> --machine`

## 9. Diagrams and gate

- [x] 9.1 `docs/architecture/attachment-lifecycle.mmd` — split the DRIFTED
      branch into adopted (resolves to nothing) and refused (still resolves)
- [x] 9.2 `docs/architecture/install-pipeline.mmd` — `uze update` entering at
      resolution with the declared ref, and a linked marketplace re-ingesting
      without touching `agents.lock`
- [x] 9.3 `cargo test -p uze-extensions` passes, so both diagrams still route
- [x] 9.4 `docs/architecture/invariants.md` — record the adoption rule and
      the linked-marketplace pin refusal, each naming the test that holds it
- [x] 9.5 `make check` (fmt, clippy `--all-targets -D warnings`, workspace
      tests, coverage floor, `cargo deny`, `openspec validate --all --strict`)
