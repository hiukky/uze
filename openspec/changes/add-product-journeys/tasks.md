## 0. Proof of concept (done)

A walking skeleton exists: `journeys/journey.py` (runner: `validate`,
`seed`, `probe`, `run`), `journeys/suites/agent-slots.yml` (7 scenes,
28 machine checks) and `journeys/README.md`, writing per-run evidence. It drives the real TUI
with a mouse, places and closes agents, and proves slot reuse against
the filesystem, Git and the task store. Nine consecutive green runs.
The tasks below are what the tier still needs; the ones the PoC
already covers are ticked.

## 1. Walking skeleton (CLI only)

- [x] 1.1 `journeys/journey.py` with `list`, `validate`, `run`, `probe`,
      `evidence`; spec parsing, `{project}/{home}/{fixtures}` resolution.
- [x] 1.2 World builder: disposable sandbox HOME/UZE_HOME under
      `journeys/.world/<journey>/`, refusing any root that could overlap the
      developer's real `~/.uze`, `~/.claude`, `~/.codex`, `~/.config/opencode`.
- [x] 1.3 Git fixture projects with dated commits (`demo-app`, `demo-web`),
      seeded from `tests/_fixtures` where one already exists.
- [~] 1.4 Check vocabulary. Present: `file`, `dir`, `git`, `tasks`,
      `process`, `cmd`, `capture`. Missing: `link`, `json`, `toml`,
      `yaml`, `unchanged`.
- [x] 1.5 Three CLI journeys: install a plugin, reconcile project context,
      remove a plugin.

## 2. One fake harness

- [x] 2.1 `uze-fake-harness` bin in `crates/uze-testkit`. Writes the scripts
      `fake_harness::Standard` composes rather than reading a rule file: the
      generator already existed, and a second serialization of it would be
      the drift this task exists to prevent.
- [x] 2.2 `FakeHarness` emits that rule file; the Rust suite keeps passing
      unchanged.
- [x] 2.3 The journey world builder provisions harnesses from the same rules.

## 3. The TUI drive

- [~] 3.0 Half done: the tmux drive is built and green across ten journeys.
      `microsoft/tui-test` is still unspiked. Spike both drives against the
      same three workspace journeys:
      `microsoft/tui-test` (pinned exactly) and tmux `capture-pane` + SGR
      1006. Decide on whether `tui-test` drives this TUI headlessly in CI;
      record the verdict in design.md.
- [x] 3.1 The chosen drive behind one module, so swapping it changes no
      journey file: tmux session per journey, `capture-pane` frames, SGR
      mouse press/release, per-character typing.
- [x] 3.2 Selectors: `in:` bands (strip/sidebar/pane), `offset`,
      `occurrence`, `glyph`, `expect`/`expect_in`/`expect_timeout`, `refuse`,
      `wait`+`until`.
- [x] 3.3 `journey validate` fails a gesture with no `expect`, a check with
      no target, and an unresolved atlas term.
- [x] 3.4 `journey probe` leaves the world and the tmux session up for hand
      inspection.

## 3b. Assertions you do not write by hand

- [ ] 3b.1 `journey record`: perform a scene against a clean world and emit
      the observed `then` block (files, links, branches, worktrees, config
      entries) for the author to edit down.
- [ ] 3b.2 Widen the in-process ratatui `TestBackend` tier for the render
      half of the regression class, before writing journeys for it.

## 4. Workspace journeys (the regression class)

- [x] 4.1 Placing an agent creates a worktree, a branch and a live pane.
- [~] 4.2 A slot a pane still sits in is never handed to the next agent.
      (The PoC proves the freed slot *is* reused; the negative case —
      an occupied slot never handed on — is not covered yet.)
- [ ] 4.3 A closed space stays closed across a manage round trip.
- [ ] 4.4 A picked directory opens its project, not a slot inside it.
- [ ] 4.5 An agent's changes are read from its own checkout.

## 5. Delivery journeys

- [ ] 5.1 `journeys/world/atlas/<vendor>.yaml` per harness, owned beside the
      integration.
- [ ] 5.2 `delivered:` check resolving through the atlas and reading bytes.
- [x] 5.3 A `tree:` capture/compare by digest across install → remove,
      scoped to the trees a harness reads. Not named `no_orphans`: the name
      would claim more than the check does, since UZE's own
      `state/attachments` keeps empty staging directories (characterized in
      the journey).
- [x] 5.4 Journeys: one plugin reaches every harness; removal leaves nothing;
      drift blocks a destructive remove.

## 6. Evidence and gate

- [x] 6.1 `evidence/<run>/verdict.json` (per check: `asked` vs. `read` vs.
      `held`, plus binary version and timings), `run.log`, a screen per
      scene on every run, the world tree, the world's processes, and an
      optional `--record` asciinema cast. Still missing: a sandbox tree
      *diff* across a scene.
- [ ] 6.2 `evidence/expected.json` quarantine registry (reason + owner), no
      silent retries.
- [x] 6.3 `.github/workflows/journeys.yml` — `--tag gate` on `src/`,
      `crates/`, `journeys/`; nightly runs the whole directory with
      `--record` and uploads the evidence. `--world real` still to come.
- [x] 6.4 `make journey`, `journey-probe`, `journey-image`,
      `journey-docker` in the Makefile, plus `journeys/Dockerfile`.

## 6b. What the first CI runs taught, and what is still open

- [x] 6b.1 The container runs what CI asks it to: `proves` is a
      repository-level lint that announces when it cannot run, both binaries
      are mounted, and the run happens as whoever owns the evidence mount.
- [x] 6b.2 Every deadline is monotonic. A stepping wall clock turned an
      `expect_timeout` into a wait that never ends or one that fired early
      and blamed the product.
- [x] 6b.3 The app is quit rather than killed, so exit handlers run — which
      is what took measured coverage of `src/ui` from 0% to 26.3%, and is
      the first time any journey exercised the shutdown path.
- [x] 6b.4 Evidence carries the state documents themselves, not only their
      paths: the world is a temp directory the next run deletes.
- [x] 6b.5 The intermittent "pane never paints" was **the harness, not the
      product**. It was reported as an open defect and it is not one.
      `render_pane` shows ` starting shell…` while the focused pane has no
      snapshot yet — a documented, expected transient — and every observed
      occurrence lines up with an environment fault of this suite's own: a
      stepping wall clock before deadlines were made monotonic, a
      coverage-instrumented binary an order of magnitude slower, or a
      container that could not write its evidence. On a clean binary with
      monotonic deadlines the suite is 10/10, and the tool's author has
      never seen it in real use. Left here because "we called our own
      environment a product bug" is worth remembering.
- [ ] 6b.6 Findings characterized in scenes rather than fixed, each with its
      reason: an empty `generated/<plugin>` created during install
      (Claude/Codex), the bridge file surviving a reconcile empty
      (deliberate; the file's existence is the one managed artifact with no
      receipt), `~/.claude/skills` created empty, and a blocked
      `plugin remove` exiting 0.

## 7. Documentation and model

- [x] 7.1 `tests/README.md`: the tier (L3.5), its boundary against
      `tests/` and `conformance/`, the "never validate UZE with UZE"
      rule, and why it is not named `e2e/`.
- [x] 7.2 `journeys/README.md`: writing a journey, the full vocabulary,
      the evidence contract, the container, debugging a failure.
- [ ] 7.3 `docs/architecture/likec4/model.c4`: add `journeyRunner` and its
      relation; run the project's arch-validate script.
- [x] 7.4 `openspec validate --all --strict` passes.
