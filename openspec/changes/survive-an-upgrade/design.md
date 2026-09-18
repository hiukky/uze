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
disposable world; the upgrade chapter runs the *released* one first (the
last tag, built or downloaded once and cached), then this build against
the machine it left. The check is what is on disk afterwards, as every
journey's is.

Alternative considered: keep committed sample documents per version.
Rejected as the weaker half of the same idea — worth adding later for the
classes a full run cannot reach cheaply, but not as the primary evidence.

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

There is no migration code. On first run of a build carrying this change:

1. A task document from an older schema is set aside and the project's
   agents are re-adopted from its checkouts. The operator sees one notice.
2. A server running at the previous endpoint is retired by the first
   attach and replaced by one at the new endpoint; spaces and panes are
   restored.
3. `doctor` reports what was set aside until the operator removes it.

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
