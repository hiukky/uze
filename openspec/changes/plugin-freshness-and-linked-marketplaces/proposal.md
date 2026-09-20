## Why

UZE can install a plugin and cannot tell you whether the one you have is the
one that exists. `plugin.json` carries `name` and `description`; the
marketplace catalogue entry carries `name`, `source`, `description`,
`keywords`. Nothing in the chain carries a version, and `update_available`
is computed only for the embedded official snapshot — for everything from a
Git or path marketplace it is `None`, which the plugins screen draws as
"Installed", indistinguishable from "up to date". Plugin versioning was
deferred on 2026-09-05 to be pulled as its own work once the `agents.yaml`
contract landed. It has landed.

Three consequences are live today:

1. **There is no command for the author's loop.** `Project::install`
   resolves what the lock does not answer and reproduces what the Store
   lacks; with the plugin locked and present it is a permanent no-op, which
   is correct — the lock is a pin. But `uze plugin update` is machine-scope
   and never touches `agents.lock`, so after it the lock denies what the
   machine holds. Editing a marketplace you author and seeing the result in
   a harness has no command at all.
2. **A marketplace read from a local checkout leaks a machine path into a
   versioned file.** This repository's own tracked `agents.yaml` declares
   `path: /home/hiukky/ai`. A contributor cloning it gets a manifest naming
   a directory they do not have, and a lock naming a private repository
   with nothing saying so — the failure they see is a raw Git
   authentication error.
3. **A managed reference UZE itself wrote can block every install.** Moving
   the generated tier from `state/attachments/` to `runtime/attachments/`
   carried the records across and left the references into it pointing at
   the old path. `attach_symlink` treats any differing target as somebody
   else's repoint and fails the whole install with `ManagedEntryDrift`,
   and because no receipt covers those references, `doctor` can neither
   see nor clear them. The same blind spot leaves a renamed capability's
   reference dangling forever, which the attachment spec already forbids.

## What Changes

**The marketplace cache becomes a repository, and that pays for both
halves.** Today `acquisition::git` strips `.git` from every materialization
and the catalogue cache keeps a bare file tree recording only its source and
a timestamp — so there is no head to read and no history to count. And
`materialize_plugin` clones the source again even when the catalogue already
holds that repository, so `market add` plus one install is two clones and
each further plugin is another; measured on the operator's own marketplace
(79 tracked files, 564 KB, 26 commits) the cost is the round trip, recorded
at 3.4 s per clone over SSH. Keeping one partial clone
(`--filter=blob:none`) per marketplace makes acquisition a `fetch` plus a
sparse checkout of the subdirectory `marketplace.json` already names, and
gives freshness both of its operands. No new schema field: the declaration
of what to fetch already exists.

**Freshness is answered by the commit.** A marketplace is a Git repository
and its commit is the only thing that answers *are these the same bytes?*
and *is there something newer?* — which `acquisition::marketplace`'s own
module doc already states. No `version:` field is introduced in
`plugin.json` or `marketplace.json`: an author who forgets to bump one
reports as current while being stale, and a tag plus a field is one fact
with two sources. Freshness is `revision` (what is installed) compared with
the marketplace ref's head (what exists), reported as up to date, behind by
a known number of commits, or never checked.

- **`uze update [plugin]`** (project scope) — re-resolves the ref each
  marketplace declares and rewrites `agents.lock`, the way `cargo update`
  moves a `branch = "main"` Git dependency. `uze install` keeps reproducing
  the pin; the two stop being one overloaded verb. UZE has no registry of
  its own — a marketplace *is* a repository — so Cargo's Git dependencies
  and Go modules are the model, not npm.
- **Three update verbs, each where ADR-019 puts it** — `uze update` moves
  this project's pins (root, so project scope); `uze plugin update` moves one
  package's bytes on the machine; `uze self-update` moves the binary and
  stops being hidden, in `keep-the-installed-binary-current`. No `uze
  upgrade` is added: it would be a second spelling of `self-update` and a
  bare root verb about machine state, both of which ADR-019 refuses.
- **A freshness check that is a read, never a write** — `uze status` and
  the plugins screen report freshness from the marketplace catalogue cache
  the `marketplace` capability already maintains, so the answer costs no
  clone on a read path and no command lies about the network.
- **A background freshness worker in the client** — refreshes catalogues off
  the render thread when the client opens, feeds the indicator, and applies
  on its own only a revision that introduces no new executable capability.
  A revision that crosses the trust boundary (`trust::executable_capabilities`
  — an MCP server's command, a hook handler's command) is offered and never
  applied, which is the invariant "a default plugin crossing the trust
  boundary is never installed silently" held for every plugin. The worker
  is machine-scope: `agents.lock` is never rewritten behind the operator.
- **Version indicators on the plugins screen** — every row says which of the
  five freshness states it is in (**up to date**, **behind**, **linked**,
  **unpinned**, **not checked**), so "Installed" stops meaning both "current"
  and "unknown". **unpinned** is what a package with no marketplace ref
  reports — a direct `uze add`, or anything under the `local` marketplace —
  and is distinct from "we looked and could not tell". The package built
  into the binary keeps the offline comparison that works today.
- **The background worker stops holding a lock across the network** —
  `Plugins::update` resolves outside `MutationLock` today's offline-only
  `auto_update` made safe to take early, and the client's startup sends its
  screen data before any freshness work rather than after it.
- **Linked marketplaces** — `uze market link <name> <path>` records, in
  machine state, that this machine reads a marketplace from a checkout the
  operator develops. The project keeps declaring the remote and pinning the
  commit; nothing local enters a versioned file. A linked marketplace is
  re-ingested when its working tree changes (tree digest, no network, no
  commit), and while it is linked `agents.lock` is not rewritten from it —
  a pin taken from unpublished work is the mistake `pnpm link` also refuses.
- **A marketplace this machine cannot reach is skipped, not fatal** —
  `uze install` and `uze update` install everything resolvable, skip each
  plugin whose marketplace this machine cannot reach, name every skip in
  their own report, and succeed. A contributor cloning a project whose
  author declared a local marketplace gets the rest of the environment
  instead of nothing. A marketplace that *is* reachable and fails is still
  an error. `uze status` says which part of the project does not reproduce
  elsewhere.
- **A private marketplace fails with a sentence** — an acquisition that
  fails on credentials names the marketplace, its URL, and that it is a
  credential question, instead of surfacing Git's own text.
- **A managed reference that resolves to nothing is adopted** — it holds no
  capability, so refusing it preserves no work. One that still resolves is
  preserved exactly as today.
- **`uze doctor` sweeps dangling references UZE wrote** — a broken reference
  in a harness discovery root pointing into `$UZE_HOME` that no receipt
  claims is reported and removable. It can only be UZE's: nothing else
  writes there.

## Capabilities

### New Capabilities

- `plugin-freshness`: how UZE answers whether an installed plugin is the
  one that exists — what is compared, when it is asked, what is reported,
  what is applied without asking, and the project-scope `update` that moves
  a pin.

### Modified Capabilities

- `marketplace`: a marketplace may be *linked* to a checkout this machine
  develops (machine-scope, never a project declaration); a marketplace this
  machine cannot reach is skipped and named rather than failing the command;
  an acquisition refused on credentials is reported as one.
- `plugin`: the plugin listing reports a freshness state rather than a
  `version`/`update_available` pair that is unanswerable for every
  non-embedded source.
- `transparent-harness-attachment`: a managed reference that resolves to
  nothing is adopted rather than preserved; a dangling reference UZE wrote
  and no longer claims is detectable and removable, which is what the
  existing "no dangling reference is left" requirement needs to be true.

## Impact

- `crates/uze-core/src/delivery/exposure.rs` — `attach_symlink` adoption
  rule.
- `crates/uze-core/src/project/project_lock.rs` — freshness comparison
  against a locked `revision`.
- `crates/uze-core/src/package/acquisition/marketplace.rs` — head of a ref,
  commit distance, credential-refusal classification.
- `crates/uze-core/src/delivery/state.rs`,
  `crates/uze-core/src/machine/home.rs` — the machine-scope link record.
- `crates/uze-application/src/application/project_environment.rs` —
  `Project::update`; linked-marketplace resolution; status reporting.
- `crates/uze-application/src/application/lifecycle/update.rs` —
  `auto_update` widened past `Embedded` under the trust rule it already
  states.
- `crates/uze-application/src/application/read_models.rs` — a freshness
  read model replacing `update_available: Option<bool>`.
- `crates/uze-application/src/application/maintenance.rs`,
  `.../doctor.rs` — the dangling-reference sweep.
- `src/main.rs` — `uze update`, `uze market link`; both classified in
  `src/command_performance.rs`.
- `src/ui/worker.rs`, `src/ui/view/plugins.rs`, `src/ui/model.rs` — the
  freshness worker and the indicator.
- No new external dependency.
