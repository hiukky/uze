## Why

Choosing `noyalib` exposed a gap: it entered the workspace inside a large
implementation commit, was never compared against alternatives, sits at
`0.0.x`, comes from a single author, and had already renamed itself once
(`serde_yml` → `noyalib`). Nothing in the repository would have caught
that, and nothing told an agent adding a crate what "good" looks like.

An audit of the whole surface — 22 direct external crates, 311 transitive
— found the rest is overwhelmingly first-tier, and three problems worth
fixing while the project is still pre-1.0 and removals are cheap. Two of
them `deny.toml` already knows about and is deferring with no exit
condition met.

## What Changes

- **Dependency guidance in `AGENTS.md`** — a provenance tier list, what to
  refuse outright, and what to check and write down before adding a crate.
  Applies to agents and people equally.
- **`noyalib` → a maintained YAML serde crate** — tracked as task 9.5 of
  `project-agent-environment`, not duplicated here.
- **`syntect` off Oniguruma** — the default `default-onig` feature
  compiles Oniguruma from C, which is the dependency `release.yml:206`
  already carries a musl-toolchain workaround for. `default-fancy`
  (`fancy-regex`, pure Rust) is the supported alternative.
- **`bincode` 1.3 stays, on purpose** — `wincode` was evaluated and is
  itself `0.6.1`; trading a frozen dependency for an unstable one fails
  this change's own rule. What changes is the `deny.toml` entry, which
  stops reading as pending work and states the condition that reopens it.
- **Advisory ignores get exit conditions** — each entry names what would
  remove it, and is revisited rather than inherited.

## Capabilities

### New Capabilities
- `dependency-provenance`: what the workspace accepts as a dependency, and
  what an advisory ignore must carry to stay.

## Impact

- **Docs** — `AGENTS.md` gains a `## Dependencies` section
- **`deny.toml`** — ignores gain exit conditions
- **`crates/uze-extensions`, `crates/uze-theme`** — syntect features
- **Build** — one fewer C dependency in the four-target release matrix
