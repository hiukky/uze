# uze-application

The product-facing facade. `UzeApplication` orchestrates Core plus the
integrations into the lifecycle operations a user asks for: add, install,
remove, update, attach, context.

- `src/application.rs` — the orchestration surface
- `src/application/lifecycle/` — one module per operation

This is also the only surface presentation may consume: `src/` must not name
`uze_core::` or `uze_integrations`. Something missing? Add a read model or a
method here.

```bash
cargo test -p uze-application
```
