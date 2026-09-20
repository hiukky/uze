## Context

See proposal.md — Why. Three facts about the code today shape everything
below.

**Neither operand exists yet, and that is the change's real work.**
`agents.lock` records a `revision`, but the marketplace catalogue cache
holds a *bare file tree*: `acquisition::git::materialize` deletes `.git`
unconditionally (*"leaving it in place would let a `.git` directory travel
into the Store"*), and `marketplace_catalogue::Meta` records only `source`
and `cached_at_unix_nanos` — no commit. There is no object database to read
a ref's head from and no history to count against.

**And every acquisition is a fresh full clone.** `materialize_plugin` calls
`acquisition::acquire` even though the catalogue already holds a checkout of
that same repository, so `market add` plus one install is two clones, and
each further plugin from that marketplace is another. Measured on the
operator's own marketplace: 79 tracked files, 564 KB, 26 commits — the cost
is not bytes, it is the round trip, recorded at 3.4 s per clone over SSH.

The two are one problem. A cache that keeps a real repository instead of a
stripped tree makes acquisition a `fetch` and makes freshness answerable,
and neither is possible without it.

**`answers()` deliberately excludes `revision`.** `LockedMarketplace::answers`
compares source, ref and subdirectory and says so in its own doc: "`revision`
is not part of the question — that is the answer." Staleness of a
*declaration* and staleness of a *resolution* are different questions, and
only the first belongs to `install`. This change adds the second question
rather than widening the first.

**Trust is already asked at the right place.** `Plugins::update` resolves the
new bytes, computes `trust::executable_capabilities` for the installed
revision, and calls `authorize(&materialized, authority, &previous, true)`.
`auto_update` already runs that under `NoTrustAuthority` and reports what it
refused. Everything the background worker needs exists; what does not exist
is the set of plugins it considers, which today is `Embedded` only.

## Goals / Non-Goals

**Goals:**

- One freshness answer, computed once, consumed identically by the CLI, the
  overview and the plugins screen.
- An author's inner loop with no commit in it.
- The project's versioned files never carry a fact about one machine.
- The delivery defects fixed such that the *class* cannot recur, not just
  the instance on this machine.

**Non-Goals:**

- **Revision history in the Store.** Rolling back to a previous local edit
  is a separate feature (the Store replaces bytes on ingest, keeps no prior
  revision) and stays out.
- **A dependency resolver.** Plugins do not depend on plugins; there is no
  graph to solve, so nothing here resembles semver resolution.
- **Per-plugin refs.** A ref belongs to a marketplace; a plugin is a
  directory inside one. This does not change.
- **Auto-update of the `uze` binary.** That is the `keep-the-installed-binary-current`
  change and stays separate.

## Decisions

### The commit is the version; no field is introduced

`acquisition::marketplace`'s module doc already states the premise — a
commit is the only thing that answers "are these the same bytes" and "is
there something newer"; a directory answers neither. Adding a `version:` to
`plugin.json` would need the field, a catalogue field beside it, and a
resolver; it would give one fact two sources (the tag and the field), and an
author who forgets to bump reports as current while being stale.

*Rejected:* semver in `plugin.json`. *Rejected:* content digest as the
freshness key — `integrity` already answers "are these the same bytes",
which is a different question, and a digest cannot say *how far* behind.

### The marketplace cache becomes a partial repository

Each Git marketplace keeps one clone under `cache/`, made with
`--filter=blob:none` and `--no-checkout`: every commit and tree, no blobs
until a checkout asks for them. Acquisition becomes `fetch` + a sparse
checkout of the plugin's own subdirectory — which `marketplace.json`'s
`source` already names, so nothing new is declared anywhere.

This is what makes the rest possible:

- **Freshness has both operands.** The ref's head is `rev-parse`, and
  `rev-list --count <locked>..<head>` gives the distance, because a blobless
  clone keeps the commits.
- **Acquisition stops re-cloning.** One connection per marketplace, then
  incremental.
- **A pinned commit still checks out.** The code's existing objection to
  `--depth 1` — *"a shallow clone cannot check out an arbitrary commit the
  caller pinned"* — does not extend to a blob filter, which keeps all
  history.

A server without `uploadpack.allowFilter` ignores the filter and sends
everything, so this degrades rather than fails. `sparse-checkout --no-cone`
needs Git 2.25.

`.git` keeps being stripped from what enters the **Store** — that is what
stops a promisor dependency travelling into a tier that must stand alone.
The repository lives in the cache tier, where losing it costs one clone.

*Rejected:* a `files:` field in `plugin.json` for the author to declare what
to fetch. The declaration already exists as the catalogue's `source`, and
pruning inside a plugin measured at 36 KB buys nothing while letting an
author under-declare and break the plugin at runtime, inside a harness,
where that failure is hardest to see.
*Rejected:* `ls-remote` — one round trip and the head SHA, but no history,
so no distance, and it puts the network on a read path.

### Freshness is remembered, never recorded

### Freshness is remembered, never recorded

The answer is derived from the lock plus a catalogue this machine
observed. Losing it costs one refresh, so by the tier rule in AGENTS.md it
belongs in `cache/`, not `state/`, and declares no shape. That is also what
makes every read path honest: a cache miss is **not checked**, which is a
real state a person can act on, rather than a reason to reach the network
from `uze status`.

*Rejected:* storing the last-known head in `agents.lock`. The lock is what
resolution produced; where a ref points today is not a property of this
project and would make the file churn on every check.

### `uze update` is a new verb, not a flag on `install`

Every comparable tool splits these: `cargo build`/`cargo update`,
`pnpm install`/`pnpm update`, `uv sync`/`uv lock --upgrade`. `install`
staying reproducible is what lets a clone of a project reach the bytes the
project was locked at. `uze update` reuses `resolve_into_lock` — the same
function `add` and `install` use, so the three cannot drift in how they
resolve — with the ref taken from the manifest's declaration rather than the
lock's revision.

The closest model is not a registry manager but **Cargo's Git dependencies
and Go modules**: there is no UZE registry, a marketplace *is* a repository,
and a plugin is a directory inside one resolved at a commit. That is why
freshness is commit distance and why nothing publishes a version — the same
reason `cargo update -p` moves a `branch = "main"` dependency and
`cargo build` never does.

*Rejected:* `uze install --update`. The two have opposite reproducibility
guarantees, and a flag that inverts a command's contract is a second command
wearing the first one's name.

### Three update verbs, and what each one moves

Three different things can be "updated", and each gets one name:

| what moves | command |
|---|---|
| this project's pins (`agents.lock`) | `uze update [plugin]` |
| one package's bytes in the Store | `uze plugin update <p>` |
| the `uze` binary | `uze upgrade` |

`uze update` is not redundant beside `uze plugin update`: the second
re-resolves the Store package and never touches `agents.lock`, so after it
the lock denies what the machine holds. They differ in *scope*, exactly as
`uze remove` and `uze plugin remove` already do — and that pair is the more
dangerous one, since the machine half deletes bytes other projects share.
The project accepted that shape; update inherits it.

`uze upgrade` sits at the root beside `setup`, `doctor`, `theme` and
`terminal` — commands about the tool rather than about a project or about
`~/.uze/*` package state, which is the category ADR-019's three machine-level
namespaces do not cover. It replaces the hidden `uze self-update`
rather than joining it: one operation, one name.

*Rejected:* `uze install --update` (the `uv sync --upgrade` shape). It would
be either a second spelling of `uze update` — a permanent alias, which
ADR-019 refused by name — or its replacement, leaving `install`, `remove`
and `status` as verbs and the fourth sibling as a flag that `uze --help`
does not list.

**This table is grammar-dependent.** If the flat grammar proposed
separately lands, `uze update` means *machine, plus this project's files
when they exist*, `uze plugin update` is gone, and only the boundary
scenario in the `plugin-freshness` spec changes — the verb, the freshness
model and everything else here stand either way.

### An unreachable marketplace is skipped, not fatal

A project may legitimately declare a marketplace that only its author can
reach. Failing the whole command over it hands a contributor nothing, when
most of the environment was resolvable; `Project::install`'s current
stop-at-first-failure is right for a marketplace that *should* work and
wrong for one that was never going to. The distinction is declared, not
guessed: a marketplace whose identity is a path this machine does not have
is unreachable by declaration, while one that is a URL and refuses is a
failure. Skips are named in the command's own report — silence would make
a half-installed environment look complete.

### A link is a machine-scope override of resolution, not a source

`state/marketplaces.json` records what the operator registered. A link is a
second fact about the same marketplace: *on this machine, read it here*.
`Project::reproduce_locked_plugin` already prefers a locally registered
checkout when its identity matches the lock's — the link generalizes that
from "same commit, read locally" to "read the working tree".

The Store stays the single source of delivered bytes (a core invariant);
what the link changes is when the Store is refilled. A linked marketplace's
plugin is re-ingested when `digest::tree_sha256` of its directory in the
checkout differs from the Store's — no network, no commit, no lock write.
The digest is the same primitive `LockedPlugin::resolved` already uses.

Refusing to write a pin from a linked checkout is the load-bearing half:
without it, an author's uncommitted edit becomes a `revision` +
`integrity` in a file their collaborators pull.

*Rejected:* reading the checkout in place instead of ingesting. Delivery
resolves through the Store and containment rules are enforced at ingest;
bypassing it would fork the delivery path for one kind of marketplace.
*Rejected:* inferring the link from a `path:` source, which is the
conflation that put `/home/hiukky/ai` in a tracked file.

### The adoption rule is "does it resolve", not "is it ours"

Deciding ownership by comparing the current target against `$UZE_HOME`
would need the home threaded through `ManagedArtifact::attach_standard` and
its eight call sites across two crates, and would still guess about a link
repointed at another UZE path. "Does the reference resolve to anything" is
a property of the reference alone, checkable where the decision is made,
and it is the property that matters: a reference resolving to nothing
delivers no capability, so replacing it destroys nothing. A reference that
still resolves is somebody's, and stays.

This makes the class impossible to recur: any future move of a UZE-owned
target leaves broken references, and broken references are adopted.

The `$UZE_HOME` test is still used by the doctor sweep, where the home *is*
available and the question is different — not "may I replace this while
attaching" but "may I delete this while nothing is attaching".

### `resolve_into_lock` splits, so the shared part stays shared

The design said `uze update` reuses `resolve_into_lock` "so the three cannot
drift in how they resolve". As written that is not possible:
`resolve_into_lock` (`project_environment.rs`) unconditionally inserts into
`lock.plugins` and `lock.marketplaces`, and hard-errors when the resolution
is not a Git commit — which is exactly what a linked marketplace read from a
working tree is. The linked rule would need a branch inside the shared
function, which is the drift the sentence promised to avoid.

It splits instead, along the line that already exists in it: **acquiring and
installing** is shared by `add`, `install` and `update` — that is the part
that must not drift — and **what gets written to the lock** is the caller's,
because that is the part the three legitimately disagree about. A linked
marketplace then writes nothing, without a special case anywhere.

### What a linked checkout counts as its content

`digest::tree_sha256` reads every file under a directory and knows nothing
about Git, so an editor's swapfile or a build artifact in a linked checkout
would trigger a re-ingest and then be delivered to a harness as package
content.

The rule follows the author's own intent, which Git already records:
**ignored files are excluded; everything else — tracked, and untracked but
not ignored — is included.** A file the author has just written and not yet
committed is what the link exists to deliver; a file they told Git to ignore
is not part of the package in any revision. Both the digest and the ingest
read the same set, so what triggers a re-ingest and what lands are never
different questions.

A modification-time and size pass runs before the digest, so an unchanged
checkout costs a stat per file rather than a full read of the tree — twice,
which is what comparing checkout to Store otherwise means.

### A held name is a warning, not the install's verdict

`deliver_package_to` propagated a refusal, so one contested name failed the
whole command — *after* the Store had ingested the package and other
integrations had already attached. The command said total failure over
partial work.

Two named tests asserted that propagation
(`naming::a_foreign_artifact_occupying_the_short_name_is_never_overwritten`,
`shared_roots::foreign_shared_entry_without_opencode_encoding_still_conflicts`),
and what they are really protecting is that the occupant is untouched and
the conflict is explicit rather than a silent skip or an automatic retry
under another name. Both still hold; only where the conflict is *said*
moves. They are rewritten to assert the stronger property — the occupant
untouched **and** the refusal named **and** the rest delivered — rather than
deleted.

The shape already exists in the product: `PublicationOutcome::error` is
"installed, and one derived view needs rebuilding", warned about and not
fatal. A held name is the same class. Only the three refusals that are about
a single name qualify — drift, conflict, projection conflict; a failed write
or a broken vendor CLI is about the machine and still fails.

### A package with no marketplace has a state of its own

Three installed packages have no ref to compare against: the embedded
`uze@uze-official`, anything from `uze add <path|git>` (installed under the
`local` marketplace, which is in no registry and has no catalogue), and a
locked plugin whose marketplace this machine never registered. Today the
embedded one is the *only* package whose `update_available` works, through
`bootstrap::has_update` against the snapshot inside the binary.

So the states are five, not four, and the extra one is not a failure:
**unpinned** — nothing to compare against — distinct from **not checked**,
which means UZE tried and could not. The embedded package keeps its own
offline comparison and reports **up to date** / **behind** from it; deleting
`update_available` without that is a regression for the one package that
answers correctly today.

### The worker resolves outside the lock, and does not gate the screen

Two things in the current code are safe only because `auto_update` is
offline, and stop being safe the moment it reaches the network:

- `Plugins::update` takes `MutationLock` and *then* clones
  (`lifecycle/update.rs:17,29`). Widened, opening the client would hold a
  global exclusive lock for seconds × N plugins, during which the operator's
  CLI in another terminal fails with `MutationInProgress` — and so does any
  mutating action inside the client, reporting the client's own pid back at
  them. Resolution moves outside the lock; the lock covers ingest and attach
  only.
- `spawn_startup` runs `auto_update()` and only then `load_refresh_data`,
  sending one `Refreshed` (`src/ui/worker.rs:560-590`). Widened, the plugins
  screen's data would arrive after the slowest clone. The two are split:
  `Refreshed` first, freshness and any update as their own later messages.

### Widening `auto_update` past `Embedded` reverses a named decision

`lifecycle/update.rs:160` states it as policy — *"Only an update uze can
already see... a Git- or path-sourced plugin is never re-resolved behind the
operator's back"* — and
`auto_update_never_re_resolves_a_source_it_would_have_to_fetch`
(`application/tests.rs:1104`) fails by name if it changes.

The policy's reason still holds where it was written: `ensure_default_plugins`
runs before *every* command, read-only ones included, and a diagnostic must
not rewrite plugin content. It does not hold for the client opening, which is
an explicit interactive act — the same argument `spawn_startup` already makes
for the work it does today. The test is rewritten to say what still stands:
no CLI dispatch path re-resolves a network source; the client's own startup
may, under `NoTrustAuthority`.

### The worker answers through a channel, like every other unbounded read

`src/ui/orchestrator.rs` already holds the rule: nothing the client draws
waits on a repository; every Git read runs on a thread and answers through
a channel, and every answer carries the question it was asked so a late one
is dropped. A freshness refresh is a Git read of exactly that kind, so it
gets a `spawn_*`/`absorb_*` pair beside the existing ones and no inline
call — which is also what the architecture suite enforces.

### Diagrams this change moves

No container, component or external dependency changes — the work lands
inside crates that already exist. Two flowcharts do change, and are part of
this change rather than a follow-up:

- `docs/architecture/attachment-lifecycle.mmd` — the inspect branch gains
  the split this change introduces: a reference that resolves to nothing is
  adopted, while one that resolves is refused. Today the diagram shows
  every DRIFTED verdict reaching `refuse`.
- `docs/architecture/install-pipeline.mmd` — `uze update` enters the same
  pipeline at `Resolve marketplaces` with the declared ref rather than the
  locked revision, and a linked marketplace re-ingests from a checkout
  without touching `agents.lock`.

Both are drawn by `uze-extensions`' own tests, so
`cargo test -p uze-extensions` is what proves they still route.

## Candidate ADRs

- **Freshness is commit distance, and no capability declares a version** —
  binds every future marketplace, catalogue and surface; reversing it means
  introducing a field into a published `plugin.json` contract.
- **A linked marketplace is machine scope and never a source of pins** —
  fixes the boundary between what a project declares and what a machine
  overrides, which is expensive to move once operators rely on it.

## Risks / Trade-offs

- **A cached catalogue makes "up to date" as old as its window.** → The four
  states are reported with when the answer was established, so "up to date"
  is never claimed without a date behind it; the client's worker refreshes
  on open, which is when a person is looking.
- **Commit distance is not user-facing versioning.** "3 commits behind" says
  less to a consumer than "1.2 → 1.3". → Accepted: it is the only honest
  answer the chain can give, and it is strictly better than today's silence.
  A `version:` field can be added later as a display label without changing
  what decides freshness.
- **Adoption replaces a reference a person could, in principle, have created
  themselves and left broken.** → Only inside a harness discovery location,
  only at a name UZE is actively attaching, and only when it resolves to
  nothing. A broken link at a name UZE wants is indistinguishable from UZE's
  own leftover by any test, and preserving it costs the operator every
  install.
- **Re-ingesting a linked marketplace on tree change adds a digest walk to
  commands that touch it.** → Scoped to marketplaces that are linked, which
  is an explicit act; `command_performance.rs` classification covers the
  commands that gain it.
- **The worker updating plugins on open surprises an operator who wanted a
  frozen machine.** → It applies only revisions that ask nothing new, reports
  everything it did, and touches no project file. If this proves wrong, the
  narrower behavior is one condition away.

## Migration Plan

Nothing on any machine needs touching. The freshness cache starts empty and
every plugin reports **not checked** until first established. The adoption
rule repairs the blocked machines on their next install, with no command to
run. `uze doctor`'s sweep is the explicit path for references nothing is
attaching.
