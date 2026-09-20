## Context

See proposal.md — Why. What shapes the approach:

- `task.rs` already holds the best version of this: it names the old shape
  with a `serde` default rather than letting the field parse into silence,
  refuses to touch a document from a schema it is ahead of (two builds on
  one machine is the ordinary state of this repository), sets aside rather
  than deletes, and re-adopts agents from the checkouts on disk. What it
  lacks is a way to say "v1→v2 = drop this field".
- `runtime.rs` independently arrived at the same policy for the workspace,
  minus the adoption — and minus any way to reach the operator, because the
  runtime is a different process from the screen.
- `state_for`/`space_for` already match a space by canonical root, and
  `Seating::Open` already means "attach if there, open if not". The resume
  path does not need a new mechanism, only to use this one.
- `src/` may not name `uze_core::`; presentation consumes read models from
  `uze-application`.
- Nothing the workspace client draws may wait on a repository.
- Pre-1.0: no compatibility branches are written for shapes older than the
  ladder covers.

## Goals / Non-Goals

**Goals:**

- Upgrading does not cost work that the records still describe.
- One place to write what a version change means, so the knowledge stops
  living in prose comments that the next file cannot read.
- The tier a thing sits in tells the operator what deleting it costs.
- Preserved work is found and resumed without having opened its project.

**Non-Goals:**

- Protocol negotiation. A mismatched `PROTOCOL_VERSION` is *answered*
  instead of hung up on; making two versions interoperate is a different
  change with a different shape.
- Compatibility with shapes older than the ladder's first step. The ladder
  starts where the current release is and grows forward.
- Reworking the per-session evaluation cache (`remembered.tasks`), the
  `TASK_REFRESH` cadence, or entries never leaving it within a session.
  Those govern the sidebar's readiness and delivery marks, which stay
  per-repository.
- Changing what `alt+p` reaches on the keyboard. The key stays bound at
  `Scope::Workspace`; only the list's content becomes machine-wide.

## Decisions

### The tier is what deleting costs, not what UZE can rebuild

"Reconstructable" conflated two different things: *re-derivable from the
world* (probe the harness again, clone the marketplace again) and
*re-expressible across versions* (nothing is missing; it is in an old
shape). The first is a property of the thing; the second is a property of
the reader. Using the first as an excuse for the second is how a document
that only needed a field dropped got thrown away.

Restated as cost, the tiers sort themselves and the filesystem can draw
them:

```
store/     bytes       the packages
state/     record      what UZE was told or decided
runtime/   generated   for another program to read   ← absorbs state/attachments/
shims/     generated   UZE's own bin; stays at the root
cache/     remembered  observation                    ← absorbs logs/
```

`shims/` stays top-level rather than under `runtime/` because it is the
directory that goes on `PATH`: a `which claude` reading
`~/.uze/shims/claude` says what it is, and one reading
`~/.uze/runtime/shims/claude` says less.

*Consequence worth naming:* `integrations.json` and `provisioning.json`
merge into one `harnesses.json` — same key space, both carrying `version`,
two writers and two clocks — and it lands in `cache/`, because every field
in it comes back from a probe. It needs no shape and no ladder. Classifying
honestly is what keeps the ladder short.

### A project is a directory, not a key repeated in four places

The same `project_id_for(root)` keys `state/tasks/<id>.json`,
`state/conversations/<id>/`, `state/prompt-history/<id>.json` and
`runtime/projects/<id>/`, and only the last records what the id means. The
id is a one-way hash, so the records can be enumerated and none of them
resolved.

```
state/projects/<id>/
  project.json          the canonical root — what makes the id reversible
  agents.json
  conversations/<agent>.json
  prompt-history.jsonl
```

This is the same answer `harness_runtime::PROJECTION_MARKER` already gives
for the generated tier: name the root so a sweep is a `readdir` rather than
an exercise in inverting a hash.

*Alternative: a root field inside the task document.* That was this
change's first shape, and the structure carries the fact better: it needs
no version of its own, it makes forgetting a project one removal, and it
makes `state/projects/<id>/` and `runtime/projects/<id>/` mirror each other
under the same id — which is itself the statement of which of the two is
safe to delete.

*Alternative: a machine-level index of projects.* Rejected for the reason
an index is always rejected here: it is a second document that can drift
from the first.

### One read policy, and the ladder is the only place a version change is written

Every read of a record goes through one path:

| declared shape | what happens |
|---|---|
| equal | read |
| **lower** | climb the ladder step by step, rewrite at current, say nothing |
| higher | leave untouched, report which build wrote it |
| unreadable | set aside, reconstruct from the world, report the residue |

Climbing is silent because carrying a record across is not an event — it is
the product working. The operator hears only about what could not be
carried.

A ladder step is a function from one shape to the next, and it is deletable
once no machine can be below it. That is what makes this *not* the
scattered compatibility the project refuses: the current struct stays clean
precisely because the old shapes live in the ladder rather than as optional
fields that never leave.

The first step the ladder gets is the one that caused this change:
**workspace v1→v2 drops the per-space `kind`.** Everything else maps one to
one.

### Reconstruction is the ladder's floor, not its rival

Where the ladder cannot help — the bytes are not a record at all — the
world is asked instead. `task.rs` already does this: branches and checkouts
on disk say which agents exist, and only what only the record knew (a label
the operator gave, what was published) is lost.

So the two mechanisms divide by what the world can still answer:

| record | world still says | ladder carries |
|---|---|---|
| `agents.json` | branches `agent/*`, worktrees on disk | label, publication, `base_commit` |
| `attachments.json` | the artifacts are there; **ownership is not inferable** | the whole receipt |
| `packages.json` | the store has the bytes; url+commit are not in them | provenance |
| `marketplaces.json` | nothing | name → url |
| `harnesses.json` | **a probe re-derives every field** | nothing — it is `cache/` |
| profiles, layout, theme | nothing | intent, trivially |

The irreducible set is small, which is what makes a ladder affordable.

### The runtime needs a way to reach the screen

Setting a workspace aside is reported with `tracing::warn!` today, and the
file sink is off unless `UZE_LOG` is set — so the one time it mattered, the
operator saw spaces disappear with nothing said anywhere. The task store,
by contrast, reaches the operator as a toast.

The runtime is a different process from the client, so the report has to
travel the protocol: an event the client turns into a toast, beside the one
the adopted task store already raises. This is also what carries the
`PROTOCOL_VERSION` answer, which today falls to `_ => None` and hangs up.

*Alternative: have the client read the workspace file itself to notice.*
Rejected — the client would be inspecting state the runtime owns, and the
two would disagree the moment either changed.

### Placement uses what exists

Resume resolves the space by canonical root via `space_for`, and opens one
with `Seating::Open` when there is none — both already in the runtime. The
work carries no space identity and never did: `Isolation` holds base,
branch, checkout, target, state and publication, and nothing about a space.
That is why a space's name and id changing is irrelevant to the match, and
why no new metadata is needed.

A space rooted *above* the project — an operator working out of a space
rooted at `$HOME` — does not match. A space's root is what the sidebar, the
Git badge and the changes overlay describe, so seating an isolated agent in
a space that describes no repository reintroduces the problem this change
removes. Matching by containment instead would make one space rooted at
`$HOME` the owner of every project beneath it, which is most of them.

### Setting aside is the floor, and the ladder is what stands on it

This change absorbs `survive-an-upgrade`, which had written the rule set,
the direction rule and the upgrade test tier — and had made "set aside and
record again from the machine" *the answer*. Its own proposal said so out
loud: what is lost is UZE's own labels and bookkeeping, "but it *is* lost".

The incident of 2026-09-19 priced that. The workspace step was a field
drop, every subsystem behaved correctly, and the operator still started
from nothing. So the order inverts: carrying a document across is the first
answer and is silent; setting it aside is what happens when no step exists.
Everything else that change decided is kept as written, because none of it
was wrong — it was the floor, described as the whole building.

### Addressability is the same problem one layer down

A document survives an upgrade by declaring its shape. A *resource* — a
socket, a generated directory — survives one by being reachable through
something whose location does not depend on the rules that moved. The claim
naming its holder, the endpoint living beside the workspace it serves, and
never starting a server from an image that is gone are the same commitment
applied to things that have no bytes to version.

This is why they belong in one change rather than two: a release moves both
at once — 2026-09-19 moved a protocol, two schemas and, earlier, an
endpoint — and a rule that covers only documents leaves the operator
rebooting.

### Recovery is judged under the lock that guards the document

A reader without the write lock can catch a publication halfway and would
set aside a document that was never broken. So the judgement — climb, set
aside, refuse — happens only where the document is held. Read-only surfaces
report what they saw and move on; the next mutation heals.

### A located resource is addressed through an anchor that cannot move

The endpoint's path is computed from the environment and the build's own
rules, so two builds can disagree about it. The anchor is the claim, which
lives beside the workspace: it is what proves a server is alive, and it
records which process holds it. Every located resource gets the same
treatment — if a build can create it, a later build must be able to name
and end it without computing the same path.

*Alternative: freeze the endpoint's location forever.* Rejected — a
location that can never change is a constraint on every future platform
(sandboxes, macOS temp layout, containers), and it does nothing for the
resources that already moved.

### The upgrade tier runs the previous release, not a fixture

A synthetic previous-shape document proves a ladder step. It cannot prove
the claim, which is about two binaries meeting on one disk: what the
release process actually produced, against what this build actually does.
So the chapter downloads and caches the released `uze`, which puts it on
the nightly cadence — it needs the network — rather than on the gate.

## Candidate ADRs

- **The tier a persisted thing sits in is defined by what deleting it
  costs** — it reclassifies existing state (a harness's recorded setup
  becomes cache), decides where every future document goes, and is
  expensive to move once the filesystem layout is public.
- **Records climb a ladder; generated and remembered things are rebuilt** —
  a durable commitment about upgrades, and the thing that decides whether
  every future schema change is cheap or is another incident.
- **The claim as the anchor for located resources** — signalling a recorded
  pid corroborated against the process table, alongside the kernel-named
  peer, widens a bar `docs/architecture/invariants.md` states today.

## Risks / Trade-offs

- **The ladder is code nobody runs on the happy path, so it rots.** → The
  `07-upgrade` chapter runs the *previously released binary* to create the
  state, then this build against it, and checks the machine rather than
  what UZE says. A synthetic previous-shape document proves the step; only
  two binaries on one disk prove the claim. The gate reads no
  previous-version document today, which is exactly why three versions
  moved in one release without anyone being asked what happens.
- **Everything moves at once: paths, shapes and two merges.** → Pre-1.0,
  and the ladder's first step is written for the shapes that exist now, so
  a machine on the current release upgrades cleanly. Older than that is
  cleaned, not carried.
- **The machine-wide list reads every project's record on open.** → Records
  only, no Git, and the documents are small. If it ever shows, the answer
  is a sweep invalidated by the records' own mtimes, not a narrower list.
- **A preserved-work row says less than a sidebar row.** → Accepted:
  readiness and delivery are questions about the project you are in. The
  row names its project, which is what gets you there.
- **Liveness is still by launch stamp**, so an agent whose pane died while
  a shell sits in its checkout lists as preserved. Today's behavior,
  unchanged here.

## Migration Plan

No compatibility code is written. What a machine meets depends on the
release it comes from, and the honest version is:

**From the current release** (`v0.0.0-alpha.6` line: task schema 3,
workspace schema 2, protocol 16):

1. Every document's shape is one this build knows, so each is carried
   across silently. Spaces, agents and checkouts survive with nothing
   reported.
2. Paths move, and each move is a ladder step rather than a fresh start —
   `state/tasks/<id>.json` becomes `state/projects/<id>/agents.json`, and
   the project's own root is recorded as it moves.
3. A server left running is met by the protocol answer rather than
   silence, and by the claim rather than a reboot.

**From the release that lost the spaces** (workspace schema 1, per-space
`kind`): the v1→v2 step drops `kind` and keeps every space, root and tab.
This is the case the change exists for, and writing the step still
recovers any machine that has not upgraded yet.

**From older than the ladder's first step**: the records are cleaned, not
carried. Pre-1.0, stated out loud rather than left for each subsystem to
decide quietly. What is lost is UZE's own bookkeeping — never a branch, a
commit, or a plugin's bytes.

**In every case**: `doctor` reports what was set aside until the operator
removes it, and what it reports is bounded, so set-aside documents do not
accumulate as their own leak.

## Resolved Questions

- **`doctor` carries one summary line and the detail where the state
  lives.** A line at the top counts what the previous version left behind,
  so it is found right after an update without reading the whole report;
  each finding stays in the area that owns it, beside the
  `ledger_error`/`integration_state_error` fields already there, so nothing
  acquires a second home.
- **The upgrade chapter downloads the released binary** rather than
  building the code at the tag: it tests what users actually run, including
  what the release process produces. It needs the network and a cache
  between runs, which is what puts it on the nightly cadence.
- **`alt+p` stays contextual.** Only the list's content becomes
  machine-wide; the key keeps its `Scope::Workspace` binding, by the
  operator's own call.
