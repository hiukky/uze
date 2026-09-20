# What UZE persists is tiered by what deleting it costs, and only records climb

Status: Accepted

## Context

Every UZE on a machine meets state a previous UZE left there, and it met it
badly. A task document written under an older schema failed `serde` before
the version guard could read it, so no agent could be created in that
repository at all. A live server held a workspace at a path the new build
no longer names, and restarting the machine was the only way out. On
2026-09-19 one release moved three versions at once; the running client
could not reach the running server, was hung up on in silence, and the
operator started from nothing.

Nothing had been written carelessly. Each file made the best decision it
could make *alone* — name the old version with a `serde` default, refuse a
document from a schema you are ahead of, set aside rather than delete, even
re-adopt agents from the checkouts on disk. The step that cost the operator
their spaces was a dropped field, every other field mapping one to one, and
the answer was still "start fresh", because **there was nowhere in UZE to
write what a version change means.**

The same absence showed up as ordinary incoherence: one project's records
under four roots all keyed by the same one-way hash with only one of them
recording what the id meant; thirteen documents, four hand-written schema
policies, nine documents carrying no version at all. The project's
most-stated principle — derived artifacts are rebuildable — was nowhere
visible in the filesystem.

## Decision

**A persisted thing's tier is decided by what deleting it costs, never by
which module wrote it.**

| tier | where | deleting it costs |
|---|---|---|
| bytes | `store/` | the packages, until they are acquired again |
| record | `state/` | the operator: nothing else knows it |
| generated | `runtime/`, `shims/` | nothing — it is produced again |
| remembered | `cache/` | nothing — it is observed again |

A thing must not sit in a tier that claims a different cost than it has.
The rule reclassifies: a harness's recorded setup is *remembered*, not a
record. Generated harness content sat one letter from the ledger that says
who owns what inside it — authoritative-looking, entirely reproducible, and
the answer to "can I delete this" opposite for each.

**Only records declare a shape, and there is exactly one mechanism for
carrying them across.** `uze-document` — a leaf crate naming no domain, no
path and no harness — holds the shape a record declares, the ladder that
carries it across, and the floor beneath that. Generated and remembered
things are produced or observed again when they cannot be read; there is
nothing in them to carry.

Carrying a record across is the product working, and nothing is said about
it. Setting one aside is the *floor*, reached only by a shape with no rung;
what the world still knows is then reconstructed and only the residue
reported. Recovery has a direction: a record from a *newer* build is never
taken, because two builds on one machine is this repository's daily state.

**Scattered compatibility is refused.** The alternative — a version branch
where each document is read — is what produced four policies and nine
unversioned documents, and it makes the current struct progressively harder
to read. One concentrated ladder inverts that: a ladder step is deletable
once no machine can be below it, so the current struct stays clean
*because* the old shapes live in the ladder.

Every path UZE owns is named in `UzeHome`, and
`every_path_uze_owns_is_named_in_the_map` fails the build over a path
composed anywhere else.

## Consequences

An upgrade stops being an event. A new document declares its tier by where
`UzeHome` puts it and, if it is a record, its shape by implementing
`uze_document::Shaped` — nothing else is needed and nothing else is
allowed, so the next document cannot invent a fifth schema policy.

The cost is a crate and a discipline: every record shape change owes a
ladder rung, and a rung is only removable once no machine can still be
below it, which is a judgement nobody can make from the code alone. The
filesystem layout also becomes public surface — moving a tier later is
expensive, which is the point, but it means the tiering has to be right
before it ships rather than after.

Source change: openspec/changes/archive/2026-09-20-redesign-persisted-state/
