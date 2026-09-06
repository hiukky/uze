## Purpose

States what the workspace accepts as a dependency, so the question is
answered by a written rule rather than by whoever is adding the crate.

## ADDED Requirements

### Requirement: A dependency is chosen by provenance
A new external crate SHALL be justified by who publishes it, in a written
note in the change that adds it. The standard library or a crate already
present SHALL be preferred over any addition; a foundation crate over a
community one; a crate with an organization behind it over one with a
single maintainer.

#### Scenario: An addition states its provenance
- **WHEN** a change adds an external crate
- **THEN** it records who publishes it, why a crate already present cannot
  do the job, and the trade-off accepted

### Requirement: Crates with unstable identity are refused
A `0.0.x` crate, a crate that has renamed itself, and a recently published
crate with no adoption SHALL be refused unless no alternative exists and
the reason is recorded.

#### Scenario: A `0.0.x` crate is refused by default
- **WHEN** a change proposes a dependency at `0.0.x`
- **THEN** it is refused unless the change records why no alternative
  exists

### Requirement: Compiling C is a stated cost, not a default
A dependency whose build compiles C SHALL be justified explicitly,
because the release cross-compiles four targets including two musl ones.
Where a feature flag reaches the same capability in pure Rust, that flag
SHALL be preferred.

#### Scenario: A pure-Rust feature is taken when it exists
- **WHEN** a dependency offers a pure-Rust alternative to its C backend
- **THEN** the workspace selects it, and the change records any capability
  difference accepted

### Requirement: An advisory ignore carries its exit condition
An entry in `deny.toml`'s advisory ignore list SHALL name both why it is
tolerated and what would remove it. An ignore with no exit condition
SHALL NOT be added.

#### Scenario: An ignore names what removes it
- **WHEN** an advisory is added to the ignore list
- **THEN** the entry states the condition under which it is deleted

#### Scenario: An ignore whose condition is met is removed
- **WHEN** the condition an ignore names has been satisfied
- **THEN** the entry is deleted rather than left inherited
