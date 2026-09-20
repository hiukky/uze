# uze-core

The harness-agnostic domain: what a package is, what a plugin declares, and
how either reaches a harness. Depends on nothing vendor-specific and must
stay that way.

Five concerns, each a module whose own doc says what belongs in it:

- `package/` — where a package's bytes come from, and where they live
- `capability/` — what a plugin declares, portably
- `delivery/` — how a capability reaches a harness
- `project/` — what a project declares, and what UZE writes into it
- `machine/` — the local environment outside UZE's own state

Public paths stay flat (`uze_core::store`, not `uze_core::package::store`)
via the re-exports at the crate root.

```bash
cargo test -p uze-core
cargo doc -p uze-core --open   # the reasoning lives in the module docs
```
