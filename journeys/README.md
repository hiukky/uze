# Product journeys

A journey performs a user's flow — through the CLI, through the TUI, or both
— in a disposable world, and then checks the **machine it left behind**.

The rule that makes this tier worth having: a journey never asks UZE whether
UZE is happy. Every `then` check reads the filesystem, Git, the recorded task
state or the process table. Screen text appears only as `expect` — the gate
that a gesture landed — never as an assertion. A command that exits zero and
reports an artifact it did not write fails here, and passes everywhere else.

Where this sits: `tests/` proves the domain deterministically against fake
harnesses; `conformance/` proves what a *vendor harness* does; a journey
proves what **UZE** does, including the 28k lines of `src/ui/` no Rust test
can click.

```bash
python3 journeys/journey.py validate journeys/suites/agent-slots.yml
python3 journeys/journey.py run      journeys/suites/agent-slots.yml
python3 journeys/journey.py probe    journeys/suites/agent-slots.yml  # leave it up to poke at
python3 journeys/journey.py run      journeys/suites --tag gate       # a whole directory
```

A spec carries `tags:`. `--tag gate` is what a pull request runs; a nightly
runs the directory with no tag at all.

`run` builds the world, performs every scene in order, and stops at the first
failure. Every run — passing or failing — writes its evidence under
`journeys/.evidence/<journey>-<stamp>/`.

## Evidence

What proves a journey is **what each check read off the machine**, kept after
the run is over. Not a picture: a screenshot proves what a screen showed,
and the claim here is about the filesystem, Git and the recorded state.

| file | what it is |
|---|---|
| `verdict.json` | the run: binary and version, world, timings, and per check the `asked` (the check as written) against the `read` (the value found on the machine) and whether it `held` |
| `run.log` | the transcript, as the terminal showed it |
| `screen-<n>.txt` | the settled frame each scene ended on — written for a passing run too, because that is the baseline the next failure is read against |
| `world.txt` | the project and UZE state trees as the run left them |
| `processes.txt` | what the world was still holding, with each process's cwd |
| `take.cast` | only with `--record`, and only if `asciinema` is installed |

A cast is for a person to watch — a review, a bug report, a demo. It is
never what proves a check. Render one with the `tui` plugin's `tui-record`
if you want a GIF; the journey does not carry a renderer.

Two runs of the same journey are diffable: `verdict.json` holds the same
shape with different generated identifiers, so a regression shows up as a
`read` that changed.

## A journey

```yaml
journey: <what this proves, as a sentence>
world:
  project: demo-app          # a Git fixture with a believable past
  harnesses: [claude, codex] # stand-in binaries on PATH
  manifest: |                # agents.yaml written into the fixture
    worktrees: { target: main, slots: 4 }
scenes:
  - scene: <what this scene proves>
    when: [ ...gestures... ]
    then: [ ...checks... ]
```

### Gestures (`when`)

| verb | what it does |
|---|---|
| `open` | launches the app in a tmux-held pty — `{uze}` is the binary under test (`in:` its directory, default the project) |
| `click` / `rclick` / `dclick` | an SGR mouse event written into the pty — indistinguishable from a hand |
| `type` | one character at a time; `submit: false` to leave Enter out, `clear: all` to empty a field |
| `key` | one key or a list (`Escape`, `C-g`, `BSpace`) |
| `shell` | a command in the world (what an agent would do to its own checkout) |
| `wait` | `screen` / `file` / `shell` with `until:` and a `timeout:` |

Aiming a click: `in:` bands the search (`strip`, `sidebar`, `pane`),
`occurrence:` picks among hits (`first`, `last`, or an index), `glyph:` aims
at any character in a set, and `offset:` / `row_offset:` move from the hit —
which is how you click a menu row by its popup's title instead of by text the
pane behind it happens to share.

**Every click states an `expect`.** `journey validate` fails one that does
not: an untimed gesture is the single largest source of flake in a suite like
this, and the linter refuses it rather than the reviewer. Synchronization is
`expect` or `wait`, never a sleep.

### Checks (`then`)

| verb | reads |
|---|---|
| `dir` | directories matching a glob: `count`, `exists`, `same_as: <capture>` |
| `file` | files matching a glob: `count`, `exists`, `contains` |
| `git` | `worktrees:` count, `branches:` pattern + `count:`, `dirty:`, `in:` |
| `tasks` | the task store UZE writes: `count`, `states`, `checkouts`, `newest_state`, `any_state`, `newest_checkout_in: <capture>` |
| `process` | `matching:` + `alive:`, scoped to this world's processes |
| `cmd` | runs a command: `exit:`, `stdout_contains:` — the *subject*, asserted beside the filesystem checks |
| `capture` | remembers `dirs:` or `task_checkouts:` under a `name:` for a later scene |

`about:` on a check is what the report prints — write the outcome, not the
mechanism.

## In a container

```bash
make journey-docker                 # build the image, run against this build
make journey-docker JOURNEY=journeys/suites/other.yml
```

The image is a **runtime**, not a build: `git`, `tmux`, `python3` and the
locale, with the binary under test mounted in and named by `JOURNEY_UZE`.
Building `uze` a second time inside an image would double the slowest step of
the run for nothing — CI's cargo cache has already built it.

What the image is actually for is pinning the terminal. A journey aims a
click at a glyph on a rendered screen, so the tmux version, the locale and
the unicode width tables are part of the contract, and they are exactly what
a runner image is free to change under you. It runs as uid 1000 so the
mounted evidence directory is writable without a flag at every call site.

Same journey, same numbers: 26.4s on the host, 26.7s in the container.

## The world

Built, never inherited: HOME, UZE_HOME, XDG_RUNTIME_DIR, PATH and the Git
identity are constructed for the run, and the app is launched under `env -i`.
An inherited `UZE_PANE` alone would make the app believe it is nested inside
a pane of the developer's own running workspace.

Worlds live under `/tmp/uze-journeys/<journey>` (override with
`JOURNEY_WORLDS`), **outside the repository on purpose**: UZE reads any path
containing `.worktrees/<id>` as an isolated checkout of the repository above
it, so a world nested under a checkout of this repo opens a space rooted at
*this* repo rather than at the fixture.

A run refuses to start if its sandbox could contain the developer's real
`~/.uze`, `~/.claude`, `~/.codex`, `~/.agents` or `~/.config/opencode`, and it
stops every process its world started — the terminal server is a daemon by
design, and waiting for it to actually exit is what keeps the next run from
connecting to a socket that is about to die.

## Harness stand-ins

A stand-in is a shell script named after the harness binary, because
`/proc/<pid>/comm` reports a script by its own file name — which is how UZE
recognizes the agent running in a pane. It prints a banner and echoes what it
is sent. No network, no credentials, no model.
