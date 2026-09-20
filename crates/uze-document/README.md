# uze-document

The one rule for reading a record another build of UZE wrote: the shape the
record declares, the `Ladder` that carries it to the shape written now, and
the floor beneath that.

- Carrying a record across is silent — it is the product working, not an event.
- Only a shape with no rung reaches the floor, and there it is set aside, never deleted.
- A record from a *newer* build is left alone: two builds on one machine is the daily state here.

A record declares its shape and ladder by implementing `Shaped`. A leaf crate
naming no domain, no path and no harness.

```bash
cargo test -p uze-document
```
