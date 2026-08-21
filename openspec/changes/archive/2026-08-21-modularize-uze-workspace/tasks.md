## Workspace boundaries

- [x] Create the workspace and make `e2e` an explicit member.
- [x] Extract harness-agnostic domain, Store, router, lifecycle, and generic
      integration contract into `uze-core`.
- [x] Extract named peer integrations into `uze-integrations`.
- [x] Extract `UzeApplication` into `uze-application`.
- [x] Preserve root `uze` reexports and `cargo install --path .` compatibility.

## Documentation

- [x] Record ADR-011 for the workspace dependency boundaries.
- [x] Update LikeC4 with crate ownership and the conformance member boundary.

## Verification

- [x] Run `cargo test --workspace --no-fail-fast`.
- [x] Run `cargo clippy --workspace -- -D warnings`.
- [x] Run `cargo fmt --check` and normalize the accumulated formatting drift.
- [x] Run OpenSpec, LikeC4, and diff validation.
