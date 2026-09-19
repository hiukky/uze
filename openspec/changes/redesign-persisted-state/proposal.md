## Why

Every UZE on a machine meets state a previous UZE left there, and it does
not meet it well. Four failures, all the same shape, each reaching the
operator as an error about a file or a path they never wrote:

- a task document written under an older schema failed `serde` before the
  schema guard could read the version, so the sidebar said `tasks
  unreadable` and **no agent could be created at all** in that repository;
- the endpoint's own rules changed, so a live server held the workspace at
  a path the new build does not name — `uze terminal stop` reported nothing
  to stop, and **restarting the machine was the only way out**;
- `make install` over a running client left `current_exe()` resolving to
  `<path> (deleted)`, and starting a server from it answered `No such file
  or directory`;
- and, on 2026-09-19, `add-space-kinds` moved three versions in one release
  — `PROTOCOL_VERSION` 15→16, the task schema 2→3, `WORKSPACE_SCHEMA_VERSION`
  introduced at 2. The running client could not reach the running server and
  was hung up on in silence, the spaces were set aside, and **the operator
  started from nothing.**

The fourth is the one that decides this change's shape. Nothing was written
carelessly: `runtime.rs` names the old document's version with a `serde`
default rather than letting the field parse into silence, refuses to touch
a document from a schema it is ahead of, and sets aside rather than
deletes. `task.rs` goes further and re-adopts agents from the checkouts on
disk. Both are the best decision each file could make **alone**. The
workspace step that cost the operator their spaces was a *field drop* —
`kind` per space, every other field mapping one to one — and the answer was
still "start fresh", because **there is nowhere in UZE to write what a
version change means.**

So setting a document aside is the floor, not the policy. And the same
absence — knowledge with no place to live — shows up as ordinary
incoherence in what UZE persists:

- One project's records sit under four roots — `state/tasks/<id>.json`,
  `state/conversations/<id>/`, `state/prompt-history/<id>.json`,
  `runtime/projects/<id>/` — all keyed by the same `project_id_for(root)`,
  a one-way hash, and only the last records what the id means.
- Thirteen documents, four hand-written schema policies (refuse, set aside,
  discard in silence, warn and read on), and nine documents with no version
  at all.
- The project's most-stated principle — derived artifacts are
  non-authoritative and rebuildable — is not drawn in the filesystem:
  generated content lives in `state/attachments/`, inside the authoritative
  tier, sharing its name with `attachments.json`, the ledger *about* it.
- The preserved-work list answers for the repositories this session has
  looked at, never the machine, so a space closed by accident takes its
  agents' work out of the one surface that exists to find it again.

## What Changes

**Carrying state across versions**

- **A document written in a shape this build knows is carried across,
  silently.** Carrying a document across is the product working, not an
  event. What a version change means is written once, as a step from one
  shape to the next, in a place the next document can reach — not as prose
  in one file the next file cannot read. A step is removable once no
  machine can be below it.
- **Setting aside is the floor.** Only a document whose shape cannot be
  carried across is set aside, reconstructed from what the world still
  knows, and reported.
- **Recovery has a direction.** A version this build is ahead of may be set
  aside; a version ahead of this build is refused and left untouched — two
  builds on one machine is the daily state of this repository. A document
  with no version at all is version 1.
- **The first step is written**: workspace v1→v2 drops the per-space
  `kind`, so a machine that has not upgraded keeps its spaces.
- **Every document declares its version and it is read before the
  document**, so the guard can never be killed by the very shape it exists
  for.
- **Everything UZE owns stays addressable when the rules that locate it
  change** — the claim names its holder, the endpoint lives beside the
  workspace it serves, and a server is never started from a binary that is
  gone.
- **The runtime can reach the screen.** Setting a workspace aside is
  announced to the client the way an adopted task store already is; today
  it is a `tracing::warn!` to a sink that is off unless `UZE_LOG` is set.
- **A mismatched `PROTOCOL_VERSION` is answered** with an error naming both
  versions, instead of falling to `_ => None` and hanging up.
- **`uze doctor` reports upgrade leftovers** — documents set aside, state
  from an unknown schema, a server at an endpoint nobody can reach — each
  with its remedy.
- **The previous release becomes part of the suite**: a journeys chapter
  that runs the *last released binary* to create state, then runs this
  build against it and checks what is left on the machine.

**Where state lives**

- **A tier criterion that holds.** `state/` stops meaning "what UZE cannot
  rebuild" and starts meaning **what UZE was told or decided** — intent and
  ownership. Everything else is observation, and observation is
  re-derivable. `state/attachments/` moves to `runtime/`; `logs/` moves to
  `cache/`; `shims/` stays at the root, because it is the directory that
  goes on `PATH`.
- **A project is one directory.** `state/projects/<id>/` holds
  `project.json` (the canonical root, which makes the id reversible),
  `agents.json`, `conversations/` and `prompt-history`. Forgetting a
  project becomes one removal; sweeping the machine becomes a `readdir`.
- **Fusions and removals:** `integrations.json` + `provisioning.json` →
  `harnesses.json`, in `cache/`, because a probe re-derives every field
  (`install.json` stays as it is: it is the installer's receipt, written by
  `install.sh`, not a record of UZE's own); the attachment receipt
  map key — `{package}:{integration}:{identity}`, unparseable because the
  identity carries colons of its own, and read by nobody — is removed and
  the ledger becomes a list.
- **One map and one writer.** `UzeHome` names every path UZE owns and
  `persistence::write_atomic` performs every write; `self_update.rs` stops
  carrying its own.

**What it buys the operator**

- **Preserved work answers for the machine**, and resuming places the agent
  in a space rooted at its own project — attaching to one already open, by
  canonical root, and opening one when there is none.

**BREAKING** (for state, not for API): paths move and documents change
shape. A document whose shape this build knows loses nothing. A document
older than the ladder's first step is cleaned rather than carried —
pre-1.0, and stated out loud rather than left for each subsystem to decide
quietly.

## Capabilities

### New Capabilities

- `persisted-state`: what UZE keeps on disk, which tier each thing belongs
  to, what happens when a build meets state another build left there, and
  what the operator is told in each case.

### Modified Capabilities

- `terminal-runtime`: the workspace carried across, the claim naming its
  holder, the endpoint beside the workspace, a server never started from a
  binary that is gone, the runtime announcing what it could not carry, and
  a protocol mismatch answered rather than hung up on.
- `agent-isolation`: the preserved-work list answering for the machine, and
  a resumed agent placed in a space rooted at its own project.

## Impact

- `crates/uze-core/src/machine/home.rs` — the single map; every path built
  inline today moves here.
- `crates/uze-core/src/delivery/persistence.rs` — the versioned envelope,
  the ladder, the direction rule, set-aside and reconstruction.
- `crates/uze-core` — every persisted document (`task`, `client_layout`,
  `prompt_history`, `conversation`, `delivery::state`, `package::store`,
  `theme_state`, `profile_state`, `detection_cache`) audited against the
  rule set; one project directory replaces four roots.
- `crates/uze-terminal` — the workspace ladder, the claim, the endpoint,
  the announcement event and the handshake answer. It has a writer of its
  own, since this crate depends on nothing else here.
- `crates/uze-application` — the machine-wide preserved-work read model,
  and `doctor`'s leftovers report, which needs a way to see the terminal it
  does not depend on today.
- `src/ui/orchestrator*` — the list drawn from the read model, resume
  placing by root; `src/self_update.rs` — its own I/O removed.
- `journeys/` — a new `07-upgrade` chapter running two binaries against one
  machine, and `06-recovery` scenes for work whose space was closed.
- `docs/architecture/invariants.md` and `AGENTS.md` — the rule set's
  properties and the tiers.
- `agents.lock` is out of scope for the rule set — derived from an authored
  manifest and committed, so it is read across versions by *other people's*
  machines — but the audit names what it does today so the decision is
  taken rather than inherited.

## Supersedes

Absorbs the change `survive-an-upgrade`, which was never implemented
(0/28). Its requirements are carried here in full; its stance — that
setting a document aside *is* the answer — is corrected by the incident of
2026-09-19, and carrying a document across becomes the first answer.
