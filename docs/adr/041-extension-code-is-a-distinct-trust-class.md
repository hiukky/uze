# Extension code is a distinct trust class from plugin bytes

Status: Accepted

## Context

UZE already has a trust boundary. `trust.rs` gates one question — may
these bytes be installed — and the recorded decision is that it is "one
consent boundary, not a permission system". An invariant holds that a
default plugin crossing that boundary is never installed silently.

That boundary was designed for a plugin: bytes UZE stores and delivers to
a harness, which the harness then reads. UZE never executes them. The risk
is transitive and the consent is a single yes.

An extension is not that. An extension is code UZE itself runs, in UZE's
own process, on the operator's machine, while the operator is looking at
something else. Today every extension is compiled into the binary, so the
question has not arisen: the code is the product's own, reviewed with it
and shipped with it.

The question arises the moment an extension is authored by anyone else,
and by then the mechanism for loading it will already have been chosen. A
sandbox is a property of the loading mechanism, not something added
afterwards.

## Decision

Extension code is a distinct trust class from plugin bytes, and the
existing consent boundary does not cover it.

Two consequences bind now, while there is nothing to load:

- An extension never touches UZE's own state — no `UzeHome`, no Store, no
  receipts, no `uze-application`. `uze-extensions` depends on no UZE
  crate and names no process, filesystem or environment API; everything
  it needs arrives through the `Host` trait the TUI implements. This is
  enforced by the architecture suite rather than left as a doc comment.
  It is what keeps an extension a pure function of what it is handed.
- Any future mechanism for loading an extension authored elsewhere must
  carry a capability model, not a single install-time yes. This rules out
  dynamic libraries, which offer no boundary at all, and it is the reason
  a sandboxed runtime's weight is a cost worth paying rather than overhead
  to be avoided.

No loading mechanism is chosen here, and none should be until a second
concrete use case exists.

## Consequences

The registry's claim that extensions "never touch the machine" is
corrected to the narrower property that actually holds and is actually
useful: the current extension does run Git and read files, and always
did — through the host, never on its own.

The trust model stays one consent boundary for plugins. Extending it into
a permission system is deferred, but the shape of what would need
permission — UZE's own state — is fixed now, so the answer is not designed
under pressure later.

A capability-scoped application surface stops being only an internal
tidiness argument: it is the thing that would let an extension be handed
read access to packages without being handed the Store (see ADR-042).

Source change: openspec/changes/enforce-architecture-seams/
