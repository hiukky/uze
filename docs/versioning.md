# Versioning

UZE uses [Semantic Versioning 2.0.0](https://semver.org/) from its first
distributable build. Until v1, every release remains an explicit pre-release:

```text
0.y.z-alpha.N
```

`[workspace.package].version` in the root `Cargo.toml` is the sole version
source for the installable `uze` binary, Core, integrations, Application and
conformance workspace members. Do not version a member independently.

Before producing a binary intended for installation, increment that value:

- `alpha.N + 1` for a compatible development delivery;
- `y` for a v0 API/product milestone that intentionally breaks compatibility;
- `z` for a v0 compatible bug-fix milestone, retaining the appropriate
  pre-release identifier until the project deliberately promotes it.

Development builds may be rebuilt freely without a version change. A binary
that is copied, installed, attached to a release, or shared for testing must
carry a newly incremented SemVer pre-release version. Confirm it with:

```bash
make version
make release
target/release/uze --version
```
