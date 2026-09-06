## Context

Two tiers exist and neither can see the frontend. `tests/` is deterministic
Rust against fake harnesses and its own reports; `conformance/` is a Docker
lab whose subject is the vendor binary. The regressions we keep paying for
live between them: a gesture in the TUI that must end in a real worktree, a
CLI command whose report is right and whose bytes are missing, a removal
that leaves an artifact behind.

## Goals / Non-Goals

- **Goals**: one readable scenario file per user flow; the same file drives
  CLI and TUI; every assertion reads the machine; adding a scenario needs no
  code; a failure prints the screen *and* the filesystem delta.
- **Non-goals**: replacing any existing tier; golden screens; model calls;
  proving vendor behavior (that is the Lab).

## Decisions

### A journey is a YAML file with three parts

`world` (given) → `scenes[].when` (gestures) → `scenes[].then` (checks).

```yaml
journey: an agent gets a checkout of its own
tags: [tui, workspace]
world:
  harnesses: [claude]          # synthetic stubs, or real binaries in --world real
  project: demo-app            # a Git fixture with a believable past
  background:
    - uze market add {fixtures}/market
    - uze plugin install hello@demo

scenes:
  - scene: placing an agent creates a worktree, a branch and a pane
    when:
      - open: uze                        # the TUI, in the sandbox, in {project}
        expect: demo-app
      - click: "✦"
        in: strip
        expect: new agent
      - click: Claude Code
        in: pane
        offset: 4
        expect: "Claude Code v"
    then:
      - git: { worktrees: 2 }
      - git: { branch: "agent/*", exists: true }
      - dir: "{project}/.worktrees/*"     # one, and it is a checkout
        count: 1
      - file: "{project}/.worktrees/*/AGENTS.md"
        exists: true
      - json: "{home}/.uze/state/terminal/panes.json"
        at: "$.panes[*].project"
        equals: "{project}"
      - process: "uze terminal serve"
        alive: true
```

`expect` belongs to `when` and `then` never reads the screen: a gesture that
landed is a precondition of the assertion, not the assertion. A `when` step
that acts without an `expect` fails `journey validate` — untimed gestures are
the single largest source of TUI-test flake, and the linter refuses them
rather than the reviewer.

### Checks read the machine, never UZE's opinion of it

The check verbs are `file`, `dir`, `link`, `json`, `toml`, `yaml`, `git`,
`process`, `unchanged`, `no_orphans`, `delivered`, and `cmd`. All but the
last two read the filesystem, Git, or the process table directly. `cmd` runs
a command and asserts its exit status and output — that is the *subject*, so
`uze status --format json` is asserted against filesystem checks in the same
`then`, which is what catches "reported success, wrote nothing".

`delivered: { plugin: hello@demo, to: claude, as: native }` is the one
composite. It resolves through `journeys/world/atlas/<vendor>.yaml` — a
declaration of where that harness reads, owned beside the integration — and
then reads those bytes. Journeys stay vendor-neutral (the same property
`contract/` holds in the Lab); a vendor path changes in exactly one file.

`no_orphans` snapshots the sandbox tree before a scene and requires the tree
after teardown to match, which is the receipt discipline stated as an
outcome instead of as a receipt-file assertion.

### The runner is Python, driving tmux

Python because a journey is a hand-maintained document: editing one must not
recompile a 28k-line binary crate, and the `conformance/` lab already
establishes Python-in-this-repo with its own `ruff.toml` and CI shape. The
alternative — Rust with `portable-pty` inside `cargo test` — keeps one
toolchain and reuses `uze-testkit`, but turns every scenario into a builder
expression, which is the readability this tier exists to buy.

tmux (not `pexpect`, which the Lab uses for harness TUIs) because UZE's own
TUI is mouse-driven: `capture-pane` gives a settled frame to find a target
in, and SGR 1006 press/release written into the pty is indistinguishable from
a hand. The beat vocabulary — `click`/`dclick`/`type`/`key`, `in:` bands,
`offset`, `occurrence`, `glyph`, `wait`+`until` — is taken verbatim from the
demo recorder's spec language, which already drives this exact TUI reliably
enough to record video takes.

### One fake harness, two consumers

`crates/uze-testkit` grows a small `uze-fake-harness` binary driven by a JSON
rule file, and `FakeHarness` emits that rule file instead of a bespoke stub.
The journey world builder writes the same rule files. A second fake harness
implementation drifting from the first is the failure this avoids.

### Evidence, and what happens to a flaky journey

A run writes `journeys/evidence/<run>/verdict.json`, plus, for a failed
scene: the captured screen, the check's expectation against what was found,
and a diff of the sandbox tree across the scene. A journey that flakes is
registered in `expected.json` with a reason and an owner — the Lab's
adaptive-result discipline — and never silently retried.

### CI: a pull-request gate, not a release gate

Measured, not assumed: one journey of 7 scenes and 28 checks takes **26.4s**
on a developer machine and **26.7s** in the pinned container. The Lab, for
comparison, is ~34 runner-minutes. The expensive step in a journeys job is
the `cargo build --bin uze` that CI's cargo cache already pays for
elsewhere, so the marginal cost of the tier is seconds per journey.

That settles the cadence. Holding journeys for release would pay the same
money later and lose the reason to spend it: these regressions arrive in
ordinary pull requests touching `src/ui`, and finding one at release time
means bisecting a range instead of reading a diff.

- `journeys.yml` on **every PR** touching `src/`, `crates/`, `journeys/`:
  the journeys tagged `gate`. No network, no credentials, no model calls.
- **Nightly**: the whole directory, untagged, with `--record`, evidence
  uploaded as an artifact.
- `--world real`: nightly, skip-if-absent per harness.

### The container is for the terminal, not for isolation

A journey already builds its own HOME/UZE_HOME/XDG_RUNTIME_DIR and refuses
to start where it could reach the developer's, so on a CI runner — itself
disposable — a container adds no isolation. What it adds is a **pinned
terminal**: a journey aims a click at a glyph on a rendered screen, so the
tmux version, the locale and the unicode width tables are part of the
contract, and a runner-image bump is free to move them. The image is a
runtime only (`git`, `tmux`, `python3`); the binary under test is mounted in
and named by `JOURNEY_UZE`, because building it again inside the image would
double the slowest step of the run for nothing.
- The demo spec (`.demo/demo.yml`) is a real flow that must keep working; it
  is replayable by the same engine locally, and is not a CI gate (it needs
  real harnesses and credentials).

## Prior art, and what is actually ours

None of the three halves of this is novel; only their seam is.

- **Scenarios as files, for a CLI**: `trycmd`/`snapbox` (assert-rs, Rust,
  `.toml` + Markdown cases), `testscript` (Go, txtar; `testscript-rs` ports
  it), `cram`/`prysk`, `bats`. All of them assert on what the command
  *printed*. That is the half this change exists because it is blind.
- **Machine state as declarative YAML**: `goss` (files, commands, processes,
  ports, users — `goss validate`), InSpec, Serverspec, Testinfra. `goss`'s
  resource vocabulary is close to this change's `then` block, and the check
  names here SHALL be modelled on it so that someone who knows `goss` can
  read a journey. `goss` itself is not adopted as a dependency: it is a Go
  binary in CI with no Git, worktree or symlink-target-under-a-glob
  resource. What is adopted is its `autoadd` idea — see `journey record`.
- **Driving a TUI**: historically ad hoc (`expect`, `pexpect`, tmux). Today
  `microsoft/tui-test` is the category, and close to what section "The runner
  is Python, driving tmux" would hand-roll: MIT, Rust with Python/JS/Rust
  bindings, mouse click/move/drag, `expect text` with auto-wait, snapshots
  and SVG screenshots, actively pushed. Its cost is that it is `0.1.0-beta`
  and self-described as undergoing a major rewrite, so its API will churn.
- **BDD orchestration**: Cucumber/`cucumber-rs`, pytest-bdd, behave (a step
  definition per sentence — the glue this change refuses), Robot Framework
  (keyword-driven, and it would hand us tags, reports and evidence for free,
  at the cost of a worse scenario syntax and Python glue per keyword anyway).
- **In-process TUI assertions**: `src/ui/tests.rs` and
  `src/ui/extension_view.rs` already render into ratatui's `TestBackend`.
  Part of the regression class this change targets is renderable there, with
  no pty at all, and widening that tier is cheaper than any journey.

What is genuinely ours, and correctly so: one file in which a TUI gesture and
a machine assertion share a world, and the vendor atlas that keeps the
scenarios free of vendor paths.

### The drive is chosen by a spike, behind our own seam

The journey file's gesture verbs are ours either way, so the drive is an
implementation detail behind one module. Both candidates are spiked against
the same three workspace journeys: `microsoft/tui-test` (bought: mouse,
auto-wait and snapshots we do not maintain; risk: beta API churn) and tmux
`capture-pane` + SGR 1006 (built: ~200 lines, already proven against this
exact TUI by the demo recorder). `tui-test` wins the spike if it drives this
TUI headlessly in CI without a real terminal emulator; tmux is the fallback,
and either way no journey file changes if the drive is swapped.

**Half the spike is done.** The tmux drive is built and green: it opens the
workspace, aims by band/occurrence/glyph/offset, writes SGR mouse events into
the pty, and drove seven scenes of `agent-slots.yml` to nine consecutive
passes. `tui-test` has not been spiked yet, and the seam it would slot into
is `Runner._open`/`Screen` in `journeys/journey.py`.

Two facts the PoC established, both of which a naive runner gets wrong:

- The world must live **outside any path containing `.worktrees/<id>`**.
  `isolated_checkout` is lexical, so a world under a checkout of this repo
  makes the app open a space rooted at this repo instead of at the fixture.
- The environment must be **built, not inherited** (`env -i`). One inherited
  `UZE_PANE` makes the app take the nested path and refuse to open a client.
- Teardown must **wait** for the world's terminal server to exit. The
  endpoint is named after the world's UZE_HOME, so a server still shutting
  down when the next run starts is a live socket the next client connects to
  and then watches die — which surfaced as an intermittent "tab created, pane
  never paints", and was the only flake the PoC produced.

### `journey record` writes the assertions for you

Borrowed from `goss autoadd`: perform a scene once against a clean world, and
the runner emits the `then` block it observed — the files that appeared, the
links, the branches, the worktrees, the config entries. The author edits it
down to what the scenario actually means. Writing the assertions by hand for
a flow that touches four harnesses is the other thing that kills this kind of
suite, and this is the answer to it.

## Risks / Trade-offs

- **A beta drive.** If the spike picks `tui-test`, a `0.1.0-beta` tool
  undergoing a rewrite sits in CI. It is test tooling, not the binary's
  supply chain, so the dependency-provenance bar in `AGENTS.md` is not the
  same bar — but the seam above is what makes reverting to tmux cheap, and
  the version is pinned exactly.
- **A second Python runner.** Mitigated by the shape being deliberately the
  Lab's: outcomes vendor-free, vendor knowledge in a named file, evidence and
  a registry for honest failures.
- **TUI flake.** Mitigated by banning `sleep` as synchronization, requiring
  `expect` on every gesture, and quarantining with an owner rather than
  retrying.
- **Journeys restating L3.** Mitigated by the non-goal and by review: a
  journey that would pass with the TUI and the filesystem removed belongs in
  `tests/`.

## Model update

This change adds a test-tooling system alongside `conformanceLab`, so
`docs/architecture/likec4/model.c4` gains a `journeyRunner` system with its
relation to `uze` (`drives the CLI and the workspace client, and reads the
machine they wrote`), validated with the project's arch-validate script.

## Candidate ADRs

- *Product journeys prove UZE, the Lab proves the harness* — why a third test
  tier exists, where its boundary sits, and why its assertions never read
  UZE's own report.
- *A closed scenario vocabulary instead of Gherkin glue* — why step
  definitions are refused as a maintenance liability.
