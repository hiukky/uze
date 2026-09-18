## Context

See proposal.md — Why. What matters here is that the three failures were
not three bugs. Each subsystem had decided, on its own and at the moment
it was written, what to do with state it could not read; the decisions
disagreed, and two of them had no answer at all for "a previous version
wrote this".

The material already in the codebase is better than its reputation. The
package registry quarantines an entry it cannot read and names the
remedy. The receipts ledger refuses a destructive operation rather than
guessing. The terminal runtime keeps a claim beside the workspace
precisely because a cleaner can take the endpoint. What is missing is
that these are three *instances of one rule set* nobody has written down,
so the fourth subsystem invents a fourth answer.

## Goals / Non-Goals

**Goals**

- One classification of the state UZE owns, and the answer each class
  owes when this build cannot read it.
- Every such document and endpoint audited against it once, so the rule
  is a property of the system rather than of the files somebody
  remembered.
- A place where an upgrade failure is visible before it is hit
  (`doctor`), and a tier that proves the claim with two real binaries.

**Non-Goals**

- Migration code. This project is pre-1.0 and its rule is to clean stale
  state, not to carry readers for every past shape. A document from an
  older schema is set aside, not upgraded in place.
- Anything a harness reads, plugin bytes, `agents.yaml`/`agents.lock`.
  Those are authored or derived from authored state and have their own
  rules.
- Downgrade. Running an older UZE after a newer one is out of scope: the
  older build cannot be taught anything by this change.

## Decisions

### The classification is by what UZE can rebuild, not by file

Three classes, and a document belongs to exactly one:

| Class | Examples | Answer when unreadable |
| --- | --- | --- |
| **Rebuildable** | task document, client layout, prompt history, conversation index, caches | Set aside, rebuild from the machine, say it once |
| **Irreplaceable** | receipts, the attachment ledger | Refuse the operation, name the remedy, never touch |
| **Located** | terminal endpoint, generated attachment directories | Reachable through a fixed anchor; the computed location may change between builds |

Alternative considered: classify by subsystem. Rejected — it is the same
mistake at a larger scale, and it gives no answer for a new file.

Alternative considered: one answer for everything ("always set aside").
Rejected — applied to receipts it would destroy the only proof UZE has
that an artifact is its to remove.

### Direction is part of the answer, and an absent version is version 1

Two builds on one machine is not an edge case here — it is the daily loop
of this repository, a release beside a development build. A rule that set
aside whatever it could not read would have them take turns destroying
each other's records, each reporting that it had recovered. So recovery
moves one way: a version this build is *ahead* of may be set aside; a
version ahead of *this* build is refused and left untouched.

And every document already on every machine carries no version field at
all. Read strictly, "every document declares a version" would make all of
them unreadable on the first run of this change — the proposal
reproducing the failure it describes. An absent version is version 1, for
every class, stated once so nobody smuggles in a per-file reader later.

### The version is read before the document, out of a shared shape

Every document gets `{"schema_version": n, …}` and a probe that reads only
that field. This is what failed in the task document: the version guard
existed, and the strict parse ran first, so the guard was dead for exactly
the case it was written for. A probe is three lines and removes a whole
class of "missing field" errors about files nobody authored.

### Setting aside keeps the bytes and moves the name

A document that is set aside is renamed to `<name>.unreadable-<unix>` —
kept, so nothing is destroyed; renamed out of the document's own
extension, so nothing that lists the directory reads it as a second
document. It is never read again. `doctor` reports it; removing it is the
operator's.

Alternative considered: delete it. Rejected — the bytes are the only
record of what was lost, and a person debugging their own machine needs
them.

Alternative considered: keep reading it with a compatibility shim.
Rejected — see Non-Goals.

### Recovery is judged under the lock that guards the document

A reader without the write lock can catch a publication halfway and would
set aside a document that was never broken. So the judgement — and only
the judgement — happens where the document is held. Read-only surfaces
report and move on; the next mutation heals.

### A located resource is addressed through an anchor that cannot move

The endpoint's path is computed from the environment and the build's own
rules, so two builds can disagree about it. The anchor is the claim, which
lives beside the workspace: it is what proves a server is alive, and it
now records which process. Every located resource gets the same treatment
— if a build can create it, a later build must be able to name and end it
without computing the same path.

Alternative considered: freeze the endpoint's location forever. Rejected —
a location that can never change is a constraint on every future platform
(sandboxes, macOS temp layout, containers), and it does not help with the
resources that already moved.

### The upgrade tier runs the previous release, not a fixture

A fixture of "what the old version wrote" is a copy of what somebody
believed it wrote. The journeys tier already runs the real binary in a
disposable world; the upgrade chapter downloads the *released* one, runs
it first, then this build against the machine it left. The check is what
is on disk afterwards, as every journey's is.

Alternative considered: keep committed sample documents per version.
Rejected as the weaker half of the same idea — worth adding later for the
classes a full run cannot reach cheaply, but not as the primary evidence.

**What the current release can actually prove.** `v0.0.0-alpha.6` writes
task schema 2, the same as this build, and records no claimant. So
against it the chapter cannot show a document being set aside at all, and
the endpoint case can only show what this build *reports* — the claim
names nobody, so there is nothing to retire. Each scenario says which
release makes its recovery provable, and the recovery scenarios join the
chapter when a release carrying the mechanism exists. Writing them as
though the recovery ran would be a suite that passes on a premise the
machine does not have.

**The world has to be short enough for the endpoint to move.** A journey
world's `$UZE_HOME` socket path is ~110 bytes against a 100-byte limit,
so `socket_path` skips the home candidate and lands where the runner sets
`XDG_RUNTIME_DIR` — the same place the old release lands. Inside a
journey both builds would agree on the endpoint and the scenario would
pass for a reason that does not exist on a real machine (a home is ~58
bytes). The chapter needs a short world root, and a check on which
directory holds the socket.

## Risks / Trade-offs

- **Setting aside loses UZE's own bookkeeping.** A project whose task
  document is set aside loses labels, publication records and task
  history; the work — branches, commits, checkouts — is Git's and
  survives, and reconciliation re-adopts the checkouts. Accepted, stated
  in the spec, and reported to the operator rather than done quietly.
- **Retiring an unreachable server ends its panes.** The processes in
  them are killed; their agents' conversations resume on restore, but an
  unsaved shell is gone. Accepted because the alternative is a workspace
  that no client can ever reach, and the operator's only remedy was a
  reboot.
- **The upgrade chapter costs a release binary per run.** Cached between
  runs and tagged so it does not run on every pull request.
- **A recorded pid can be recycled.** Corroborated against the process
  table before it is signalled, which is the bar the rest of the runtime
  already holds itself to; a recycled pid that is also a live `uze` is the
  residual window.

## Migration Plan

There is no migration code. What actually happens depends on which
release the machine is coming from, and the honest version of that is:

**From `v0.0.0-alpha.6`** (the current release: task schema 2, no
recorded claimant, endpoint under the session's runtime directory):

1. The task document reads normally — same schema — so nothing is set
   aside.
2. A server left running holds the workspace at the old endpoint and
   records nobody. This build cannot name it, so it reports that, names
   the endpoint it looked at and how to find the process; the operator
   ends it once. This is the state the change makes *legible*; it cannot
   make it automatic without the record the old release never wrote.
3. From the first release that records a claimant, the same state is
   recovered without the operator: retired and replaced, panes restored.

**From a release older than alpha.6** (task schema 1): the task document
is set aside, the project's agents are re-adopted from its checkouts, and
the operator sees one notice — the failure that started this change.

**In both**: `doctor` reports what was set aside until the operator
removes it.

## Candidate ADRs

- **The three classes and their answers.** A rule every persisted
  document inherits is exactly the kind of decision that is costly to
  change later: it decides what UZE may destroy on an upgrade. Flagged
  here; written at archive, once it has held.
- **The claim as the anchor for located resources** — signalling a
  recorded pid corroborated against the process table, alongside the
  kernel-named peer, widens a bar `docs/architecture/invariants.md`
  states today.

## Resolved Questions

- **`doctor` carries one summary line and the detail where the state
  lives.** A line at the top counts what the previous version left
  behind, so it is found right after an update without reading the whole
  report; each finding stays in the area that owns it, beside the
  `ledger_error`/`integration_state_error` fields already there, so
  nothing acquires a second home.
- **The upgrade chapter downloads the released binary.** It tests what
  users actually run, including what the release process produces, rather
  than the code at the tag. It needs the network and a cache between runs,
  which puts the chapter on the nightly cadence rather than the gate.
