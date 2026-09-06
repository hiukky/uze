---
name: journey-author
description: Writing, changing or debugging a product journey (journeys/) — use whenever you are adding a scenario for a user-facing flow, a journey fails or flakes, a selector stopped matching after a TUI change, you need to decide whether something is a new scene or a new file, or you are about to assert on UZE's own output instead of on the machine. Covers the chapter spine, the scene/file split rule, the gesture and check vocabularies, the probe loop, reading evidence/verdict.json, and the three world constraints a naive runner gets wrong.
---

# Writing a product journey

A journey performs a user's flow — through the real CLI, the real TUI, or
both — in a disposable world, then checks **the machine it left behind**.
`tests/` proves the domain deterministically against fake harnesses;
`conformance/` proves what a vendor harness does; a journey proves what UZE
does, including the parts of `src/ui/` no Rust test can click.

Read `journeys/README.md` for the full vocabulary. This is what to do, in
what order, and what goes wrong.

## The two rules everything else serves

**Never validate UZE with UZE.** Every `then` check reads the filesystem,
Git, the recorded task state, or the process table. UZE's own report is the
*subject* — a `cmd` step whose output you assert *against* those checks —
never the source of truth for another check. This is the whole reason the
tier exists: it is the only place that catches "exited zero, reported the
artifact, wrote nothing".

**Screen text is a gate, never an assertion.** `expect` proves a gesture
landed, so the next one acts on the state you think it does. If you find
yourself asserting on what the screen says, the claim belongs in
`src/ui/`'s own `TestBackend` tests, where it is cheaper and more precise.

## Step 1 — decide where it goes before writing it

Chapters are the order a person meets the product, not subsystems:

```
01-first-run  02-packages  03-context  04-workspace  05-delivery  06-recovery
```

Numbers are reading order only — every journey builds its own world, so
nothing depends on a lower number having run. A chapter appears when its
first journey does.

**Scene or file?** A scene continues the story; a file starts one over.
Joined by "and then" → scene. Joined by "also" → file. Four things force a
file: a different world; a claim that does not depend on the story so far; a
different cadence (`gate` vs nightly); and — a property of this runner, not
of taste — **a run stops at the first failure, so claims sharing a file share
a fate**. Two claims you want proven independently do not belong together.

**Does the scene earn its place?** Delete it: a later scene must break, or
its checks must be the only proof of its own claim. A scene that survives its
own deletion was a step, not a claim.

## Step 2 — find the selectors by hand, never by guessing

```bash
make build
python3 journeys/journey.py probe journeys/suites/<chapter>/<file>.yml
tmux capture-pane -t journey-<pid> -p | cat -n
```

Aim at what is stable. In order of preference:

- a **landmark plus an offset** — `click: " new agent "`, `in: pane`,
  `row_offset: 2` clicks the second row of a popup by its title, which
  survives whatever the pane behind it happens to say;
- a **glyph set** — `glyph: "●○"`, `in: sidebar`, `occurrence: last`;
- a **band** — `in: strip` / `sidebar` / `pane` keeps a sidebar entry apart
  from a pane that says the same words.

Avoid aiming at a count of a repeated character (`occurrence: 2` on `✦`)
when the count changes with the number of agents. The tab strip's new-agent
button is `click: "│"`, `in: strip`, `occurrence: 2`, `offset: 2` — anchored
on the separator, stable at any agent count.

**Every gesture states its `expect`.** `journey validate` fails a click that
does not, because an untimed gesture is where a suite like this dies. Never
use `pause` as synchronization: it is settling time after a gate, nothing
more. Wait on a condition with `wait: screen|file|shell` + `until:`.

## Step 3 — assert the machine, in product terms

`dir`, `file`, `git`, `tasks`, `process`, `cmd`, `capture`. Write the
`about:` as the outcome ("the slot survives its agent — a checkout is reused,
not destroyed"), never the mechanism.

Carry state between scenes with `capture` — that is how "the next agent took
a slot that already existed" is expressible at all:

```yaml
- capture: { name: slots, dirs: "{project}/.worktrees/*" }
# ... later scene ...
- dir: "{project}/.worktrees/*"
  same_as: slots
- tasks: { newest_checkout_in: slots }
```

Where UZE writes what a check reads: tasks in
`$UZE_HOME/state/tasks/<project id>.json` (`id`, `state`, `checkout`,
`branch`); checkouts in `<project>/.worktrees/<slot id>` on branch
`agent/<task id>`; terminal state under `$UZE_HOME/state/terminal`.

## Step 4 — when it fails, read the evidence before touching anything

`journeys/.evidence/<journey>-<stamp>/` holds `verdict.json` (per check: the
`asked` against the `read` and whether it `held`), `run.log`, the frame each
scene ended on, the world's tree, and the processes it was holding.

- **`screen-N.txt` is empty** → the app exited. The pane outlives it on
  purpose, so the frame carries `[journey] the app exited with <status>` and
  the error above it.
- **"target is not on screen"** → the frame is in the evidence; look at it
  before changing the selector. A TUI change may have moved a landmark, which
  is the journey doing its job.
- **A check failed** → `verdict.json` has both sides. If `read` is right and
  `asked` is wrong, the journey encoded an assumption the product never made
  (an empty task store *is* written on open, for instance).
- **It passed and then failed with no code change** → do not re-run it green.
  Find the cause: a flake here is usually the runner racing itself, and the
  two found so far were a teardown that did not wait, and a test identifying
  a child by a command line anyone could share.

## The world constraints, each of which was learned the hard way

- **Outside `.worktrees/`.** `isolated_checkout` is lexical, so a world under
  a checkout of this repo makes the app open a space rooted at *this repo*
  instead of at the fixture. Worlds live in `/tmp/uze-journeys`.
- **Built, never inherited.** The app launches under `env -i`. One inherited
  `UZE_PANE` makes it believe it is nested inside a pane of the developer's
  own workspace and refuse to open a client.
- **Teardown waits.** The endpoint is named after the world's `UZE_HOME`, so
  a terminal server still shutting down when the next run starts is a live
  socket the next client connects to and then watches die — which looks like
  "a tab was created and its pane never painted".

## Before you finish

```bash
python3 journeys/journey.py validate journeys/suites
python3 journeys/journey.py run journeys/suites --tag gate
make journey-docker          # the pinned terminal, which CI uses
ruff format journeys/ && ruff check journeys/
```

Run it at least twice. One green proves it can pass; two proves it does not
depend on the machine being idle.
