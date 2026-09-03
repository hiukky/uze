# Enforce the architecture seams in the test suite, and close the layering debt

Status: Accepted

## Context

The dependency direction this project documents — `CLI/TUI →
uze-application → uze-core`, with `uze-integrations` implementing the
core's `IntegrationPort` — was an aspiration, not a fact. Measured,
`src/` named `uze_core::` in 19 production places across 9 files, more
than it named `uze_application`, and a compatibility facade in
`src/lib.rs` re-exported the whole domain, so a reach written as
`crate::UzeHome` named no forbidden path at all. Every domain change
rippled straight into the frontend.

Three further seams were drawn in the wrong place, each more expensive to
move the longer it stood: two modules spawned `git` with two exit-code
conventions, which makes any future write lock over refs incomplete by
construction; the one TUI extension drew straight into the frame and
copied the host's palette by hand; and `uze-application` was one large
type, so presentation had nothing narrower than "everything" to reach
for.

The compiler cannot enforce the layering rule in place of a test:
`src/shim.rs` and `src/bin/uze-harness-matrix.rs` share the binary crate
and legitimately name the domain, because a composition root consumes
`IntegrationRegistry`. Making `rustc` the enforcer would mean giving them
crates of their own — a decision about what the `uze` binary is, not a
tidy-up.

## Decision

The seams are enforced by tests in `tests/architecture/`, and each rule is
data: a scanned scope, a forbidden path prefix, a reason printed on
failure, a `sanctioned` list of files permanently allowed with the reason
written down, and a `budget` of remaining debt as an **exact** count that
may only shrink. Sanctioned and budget are two lists because they mean
opposite things: one is architecture, the other is debt with a number on
it. Test-only code is out of scope.

- **Presentation never names the domain.** `src/` must not name
  `uze_core::` or `uze_integrations`; it consumes read models from
  `uze-application`. The facade in `src/lib.rs` is deleted. The
  application re-exports the vocabulary its read models are made of — a
  read model that carries an `AttachmentState` names that type, and
  making the caller find it in the domain is exactly what put
  `uze_core::` in the TUI. The debt is zero and a budget is never raised.
- **The application is reached through capability-scoped services.**
  Each service is a newtype over `&UzeApplication` — no state, no cost,
  one owner — and a caller names the capability it wants and gets only
  that. Deliberately not `Deref` to the owner, which would re-expose
  every method through every service. Helpers shared by two views go back
  to the type that owns the state. `uze-core`'s modules are grouped into
  five concerns as submodules, with public paths kept flat through
  re-exports so the grouping is visible in `lib.rs` without renaming
  hundreds of references.
- **One transport for Git.** `uze-git` owns the spawn convention and
  reports Git's exit code rather than classifying it — a non-zero exit is
  an answer for `diff`, `rebase` and `rev-parse --verify` and a failure
  elsewhere, and only the caller knows which. Reads and writes are
  separate entry points from the start, so a read never takes a future
  lock. `acquisition::git` is the one sanctioned second spawn: it clones
  *untrusted remote* repositories and therefore strips the environment
  rather than inheriting it — a different threat model, not a second
  convention. A third spawn fails the suite.
- **An extension answers with a view and never draws.** It returns a
  `view::View` — what it has, not how it looks — and the host owns
  rendering, geometry, hit-testing and the palette. Syntax highlighting
  survives as a role per span that the host maps to its own colours. The
  view vocabulary grows only when a second extension needs the same
  primitive. Every other capability arrives through `Host` (ADR-041).

## Consequences

A domain change no longer ripples into the frontend, and there is one
surface anything else could consume. Adding something presentation needs
means adding it to `uze-application` — a read model, a method on a
service, or a re-export — never reaching past it.

A repository write lock can now be complete, because no module spawns Git
around `uze-git`. The cost is that every new Git call goes through one
crate that carries no domain, and a caller has to know which exit codes
are answers for the command it runs.

The suite is scaffolding that reports how far the layering is from being
compiler-enforced. The end state it points at is the deletion of
`uze-core` from the binary's `[dependencies]`, which is blocked on the
decision about the shim and the matrix tool named above, not on debt.

Extracting the shared preference-translation procedure out of the four
integrations gave back 29% per vertical at a cost of 194 shared lines —
net +7 today, paying from the fifth harness on. What remains is each
vendor's own table, which is irreducible; moving it to a manifest would
trade type safety for a parser.

Source change: openspec/changes/enforce-architecture-seams/
Source change: openspec/changes/close-the-application-boundary/
