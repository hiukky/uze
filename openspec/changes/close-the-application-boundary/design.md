## Grouping without renaming

`uze-core`'s five concerns are submodules, and the public paths stay flat
through re-exports at the crate root: `uze_core::store`, not
`uze_core::package::store`. Renaming 581 external references would have
bought a diff nobody can review for a naming benefit that is secondary —
the complaint was about *reading the crate*, and a reader who opens
`lib.rs` now sees which concern each module belongs to, in the re-export
list itself.

That leaves the option open rather than spending it: dropping one
re-export later lets the compiler drive that migration one module at a
time, if it ever earns its keep.

Module file names keep their full public spelling. `project/lock.rs` reads
better in the tree but would stop matching `uze_core::project_lock`, and a
name that changes between the inside and the outside costs more than the
prefix saves.

## Services as borrowed views

Each service is a newtype over `&UzeApplication`: no state, no cost, one
owner still. What changes is that a caller names the capability it wants
and gets only that — `Workspace` reaches nothing that writes.

The boundaries follow the module each operation already lived in. Those
files were drawn deliberately, and redrawing them in the same change would
make one diff argue two things.

Deliberately *not* `Deref` to `UzeApplication`. That would re-expose every
method through every service and defeat the scoping, which is the point.
Reaching the owner is explicit (`self.0`), and a sibling service is reached
the same way (`self.0.plugins().acquire(…)`).

The split forced a useful question each time a helper turned out to be
shared. `runtime_shim_is_active` (health *and* workspace) and
`locked_plugin_id` (project *and* workspace) belong to neither view, so
they went back to the type that owns the state. `acquire` and
`install_materialized_from_marketplace` are genuinely plugin-lifecycle
mechanics, so they stayed on `Plugins` and siblings compose through the
handle. Two different answers, and the split is what made the difference
visible.

## What the application owes presentation

A read model that carries an `AttachmentState` has to name that type.
Making the caller find it in the domain is precisely what put `uze_core::`
in the TUI's imports, so the application re-exports the vocabulary its
read models are made of. That is not a hole in the layer; it is the layer
carrying its own types.

The seat rule moved with the operations, and the split does it good: the
TUI answers *which panes are agents* — a presentation fact about the
generated label — and the application decides *what occupying a checkout
means*. That rule is the guarantee two agents never share a checkout, and
it was living next to a render loop.

## The facade was the hole

`src/lib.rs` re-exported all of `uze-core` and `uze-integrations`. A reach
written as `crate::UzeHome` named no forbidden path, so the layering rule
saw nothing — a limitation documented in the rule itself when it landed.
Deleting it moved 184 references and revealed that `src/main.rs` alone
named `uze_core::` 54 times. That debt was not new; it was newly visible,
and closing it was then ordinary work.

## Two Git spawns, not one

`uze-git` was described as the single owner of speaking to Git. It is not:
`acquisition::git` spawns Git in production too, and correctly — it clones
*untrusted remote* repositories, so it strips the environment where
`uze-git` drives the operator's own checkout and must let their
configuration apply. Merging them would be wrong in both directions, so
the rule names both and fails on a third.

## Preference translation, measured

Each vertical wrote the same procedure with different data. Extracting the
procedure gave back 29% per vertical (657 → 470 production lines) at a
cost of 194 shared lines — net +7 today, paying from the fifth harness on.

The number matters more than the saving: it recalibrates what "reduce the
cost of a new harness" can mean. What remains after extraction is the
vendor's own table — that Claude calls it `acceptEdits` and Codex calls it
`on-request` — and that is irreducible. The extraction converged on "a
table written in Rust", which is about as declarative as it should get;
moving it to a manifest would trade type safety for a parser.
