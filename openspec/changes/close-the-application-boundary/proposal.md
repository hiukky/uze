## Why

`enforce-architecture-seams` put a number on the layering debt and froze it:
`src/` named `uze_core::` in 19 production places, and a compatibility
facade in `src/lib.rs` re-exported the whole domain so that a reach written
as `uze::trust` or `crate::UzeHome` named no forbidden path at all. The
guard could count the debt. It could not close it, and it could not see
through the facade.

Three things had to be true before it could:

- **`uze-application` had to have the thing to reach for.** Presentation
  needs `AttachmentState` because a read model carries one; it needs prompt
  history, workspace-root resolution and harness descriptors because
  screens are built from them. Every one of those was a reach into the
  domain purely because the facade did not offer it.
- **The facade had to be scopeable.** 49 public methods on `UzeApplication`
  is the shape that makes "add a feature, touch the middle" true, and it
  makes "this caller may read packages but never write the Store"
  inexpressible — there is only one handle and it can do everything.
- **`uze-core` had to have a readable shape.** 36 modules at one depth:
  `hook.rs` beside `home.rs` beside `subprocess.rs`, a capability beside a
  path beside a process utility, with nothing in the tree saying which is
  which.

## What Changes

- Group `uze-core`'s 36 flat modules into the five concerns they already
  are — `package`, `capability`, `delivery`, `project`, `machine` — each
  with a module doc stating what belongs in it. Public paths stay flat via
  re-exports, so no call site changes.
- Split `UzeApplication` into seven capability-scoped borrowed views
  (`plugins`, `marketplace`, `profiles`, `health`, `project`, `context`,
  `workspace`). Nothing but constructors and bootstrap stays on the handle.
- Move the operations presentation was performing itself: the seat rule
  (where a new agent's checkout goes) and hook dispatch (resolving a vendor
  adapter, normalising a native payload, rendering the decision).
- Delete the `src/lib.rs` compatibility facade, moving 184 references onto
  the crates that own them, and re-export from `uze-application` the
  vocabulary its read models are made of.
- Record that Git is spawned in exactly two places for two different threat
  models, and fail a third.
- Give preference translation one procedure across the four harness
  verticals, deriving `changed_keys` instead of maintaining it by hand.

## Capabilities

### New Capabilities

None. No user-visible behaviour changes.

### Modified Capabilities

None.

## Impact

- `crates/uze-core/**` (module grouping), `crates/uze-application/**`
  (services, vocabulary re-exports), `src/**` (CLI and TUI consume the
  application), `tests/**` (imports follow the crates that own them).
- `crates/uze-integrations/src/shared/preference.rs` (new) and the four
  verticals' `preferences.rs`.
- `tests/architecture/layering.rs`, `AGENTS.md`,
  `docs/architecture/invariants.md`.

## Non-goals

- Making `rustc` the enforcer by dropping `uze-core` from the binary's
  dependencies. `src/shim.rs` and `src/bin/uze-harness-matrix.rs` share
  that crate and legitimately name the domain; giving them crates of their
  own is a decision about what the `uze` binary is, not a tidy-up.
- A registry-driven CLI grammar. Recorded separately as deliberately not
  taken.
- Extracting the remaining harness concerns (`mcp`, `skills`, `plugin`,
  `generate`). Preference translation is the first, measured, and the
  measurement is what should decide the rest.
