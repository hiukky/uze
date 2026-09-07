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
python3 journeys/journey.py list     journeys/suites                  # what is proven, in order
python3 journeys/journey.py run      journeys/suites --tag gate       # everything the gate runs
python3 journeys/journey.py run      journeys/suites/04-workspace     # one chapter
python3 journeys/journey.py run      journeys/suites/04-workspace/01-agents-and-slots.yml
python3 journeys/journey.py probe    <spec>                           # leave it up to poke at
python3 journeys/journey.py validate <spec|dir>
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

## How the suite is organized

```
journeys/suites/
  01-first-run/     a machine with nothing on it: it opens, it reports itself
  02-packages/      a plugin arrives, is delivered, is taken back
  03-context/       AGENTS.md, and the bridges each harness reads
  04-workspace/     spaces, agents, checkouts, slots
  05-delivery/      handing the work back
  06-recovery/      drift, a lost checkout, a rebase that stopped
      NN-<the claim, in kebab>.yml
```

**The numbers are the reading order, not a dependency.** Every journey builds
its own world from nothing, so `04` does not need `01` to have run. The
prefix exists so that someone opening the folder meets the product the way a
user does — install it, configure it, work in it, deliver, recover — instead
of alphabetically. Chapters appear when their first journey does; an empty
one is a promise, not a plan.

The index is `journey list`, read from the files themselves. Do not keep a
table of what the suite proves — that is the same trap as a hand-copied test
matrix: correct the day it is written.

Naming: the file is the claim in kebab-case, the `journey:` field is the same
claim as a sentence, and `about:` is the paragraph a reader needs before the
first scene makes sense.

## Scene, file, or chapter?

**A scene continues the story; a file starts one over.** If you would join
the two with *"and then"*, it is a scene. If you would join them with
*"also"*, it is a file.

Write a new **file** when any of these is true:

- **the world differs** — a different fixture, harness set, or manifest;
- **the claim does not depend on the story so far** — it would pass just as
  well first;
- **the two would run at different cadences** — one on the gate, one nightly;
- **one failing would hide the other.** A run stops at the first failure, so
  claims sharing a file share a fate. Two claims you would want proven
  independently do not belong in one.

Write a new **scene** when the claim needs what the last one left behind —
"the next agent takes the freed slot" means nothing without "an agent was
closed".

Write a new **chapter** when a group of files shares a stage of use, not when
it shares a subsystem.

### A scene must be load-bearing

Delete it: a later scene must break, or its checks must be the only proof of
its own claim. A scene that survives its own deletion was a step, not a
claim — fold it into its neighbour. This is what keeps a journey from
drifting into a script that clicks a lot and proves little.

### Tags say when it runs, numbers say where it sits

`gate` is the pull-request set: fast, deterministic, no real harness.
Everything runs nightly. `cli` / `tui` name the surface, and a domain tag
(`workspace`, `packages`, `context`) is how you run a slice while working on
one. Cadence never goes in the folder name — a journey that becomes slow
should change its tag, not move.

### A journey can name the page it backs

```yaml
proves: web/content/docs/workspace.mdx
```

`journey validate` fails a `proves:` that no longer resolves, and
`journey list` prints it. That is the whole mechanism, and its limits are
worth stating: it catches a page that moved or was deleted, and it tells
whoever changes a flow which page to re-read. It cannot tell you the prose
stopped being true — no check can, and pretending otherwise is worse than
not having one.

The direction is journey → page, not page → journey: the suite changes far
more often than the site, the link belongs where the change happens, and the
site's frontmatter schema stays untouched.

### Worlds repeat before they deserve a name

The `world:` block stays in the journey, where a reader can see it without
opening another file. When a third journey needs the same fixture, and not
before, lift it into `journeys/worlds/<name>.yml`.

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

**This is for your machine, not for CI.** A journey drives tmux and writes a
world; on your own laptop the container is what keeps it away from your tmux
server, your `HOME` and your terminal. CI has nothing to protect — a hosted
runner is already disposable, and `journey.py` builds its own world with a
guard that refuses one overlapping a real home — so both CI jobs run the
journeys natively.

The pinning the image also gave is not lost. What it pins is the Ubuntu
release, via its `FROM`; `runs-on: ubuntu-24.04` pins exactly the same thing
and costs nothing, and `LANG`/`TERM` are set in the workflow. Neither pins a
tmux *version* — `apt-get install tmux` on `ubuntu:24.04` takes whatever that
release currently carries, the same as the runner does.

There is a second reason CI runs natively: macOS runners have no Docker.
A containerised Linux run and a native macOS run would be proving the same
journeys two different ways, and a difference between them would be about
the two runtimes rather than about the two platforms.

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
