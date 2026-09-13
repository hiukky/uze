## Context

`noyalib` was adopted without comparison inside `9e55f2f`. Auditing why
that was possible turned into auditing the whole surface.

## The surface as audited (2026-09-05)

22 direct external crates, 311 in `Cargo.lock`. By provenance:

- **Foundation** — `serde`, `serde_json`, `thiserror`, `libc`, `tokio`
- **Organization-backed** — `clap`, `ratatui`, `crossterm`, `toml_edit`,
  `indicatif`, `dialoguer`, `anstyle`, `unicode-width`,
  `alacritty_terminal`, `portable-pty`, `rmcp`
- **Single maintainer, mature** — `syntect`, `comfy-table`, `schemars`
- **Problems** — `noyalib`, `bincode` 1.3

The conclusion worth recording: the surface is healthy. The gap was never
a bad crate everywhere, it was the absence of a written rule, which let
exactly one bad choice through unnoticed.

## Decisions

**syntect drops Oniguruma.** `syntect`'s default is `default-onig`, which
pulls `onig` → `onig_sys` → Oniguruma compiled from C — the dependency
`release.yml:206` documents a musl-toolchain workaround for. `default-fancy`
swaps in `fancy-regex`, pure Rust, already present in `Cargo.lock`.
`fancy-regex` is not a drop-in for every Oniguruma construct, so the
highlighting output must be compared before and after, not assumed equal.

Note what this does *not* fix: both feature sets include `yaml-load`, so
`yaml-rust` (`RUSTSEC-2024-0320`, unmaintained) stays either way. The
existing ignore's stated exit — "it leaves when syntect moves to
yaml-rust2" — remains upstream's to satisfy, and that is exactly the shape
an ignore should have.

**bincode stays on 1.3.3, deliberately.** Two majors behind, with
`RUSTSEC-2025-0141` ignored because it "carries the terminal wire format;
replacing it is a protocol change". `wincode`, the alternative evaluated,
is `0.6.1` — trading a frozen-but-known dependency for an unstable one
fails this change's own rule, and the wire format is agreed between two
processes, so the migration cost is a protocol version, not a bump. The
outcome is to keep 1.3.3 and stop calling it pending: the ignore is
rewritten to say it is carried on purpose, with the condition that would
reopen it (a real advisory against the format, or the protocol being
versioned for another reason anyway).

**`noyalib` stays, as a written exception.** It is the crate this whole
change started from, and it breaks three of the rules the change wrote:
`0.0.x` across 33 releases, one author, renamed once from `serde_yml`, and
scope sprawl (a `robotics` module in a YAML library). The rule says such a
crate is refused *unless no alternative exists and the reason is recorded*.
This is the record.

*What it is.* The parser for `agents.yaml` and `agents.lock` — the first
thing `uze` reads in a repository, which may be one the operator has just
cloned.

*Why it is kept.* The alternatives were evaluated, not assumed.
`serde_yaml` is archived by its author and takes no fixes.
`yamlpath`/`yamlpatch` pulls `tree-sitter` plus a C grammar with a build
script onto a release matrix that already carries a musl workaround for one
C dependency — disproportionate to three write paths, and rejected in
`project-agent-environment` 9.7 with that reasoning. Writing the surgical
editor by hand was costed as 9.3 and was not needed. Against the signals,
the evidence: 43.6k lines of source against 65.5k of tests, 351 vendored
`yaml-test-suite` cases, `#![forbid(unsafe_code)]` and no `unsafe` anywhere
in the tree; `project-agent-environment` 9.2 spiked its `cst` module against
a real commented `agents.yaml` and it held on all five properties.

*What limits the exposure.* `crates/uze-core/src/project/manifest/edit.rs`
is the only file in the workspace that names `noyalib::cst`, and
`manifest::edit::reject_shapes_we_will_not_edit` refuses anchors, aliases,
merge keys, tabs and document separators before an edit is attempted. The
typed schema, the validation and the writers live in `manifest.rs`, which
names the crate only through serde. Replacing it is one module, not a
migration.

*What removes the exception.* Any of: the crate reaching `0.1` with a stable
API, so it stops being a `0.0.x` entry; the crate going quiet (no release in
twelve months) or renaming itself again, either of which makes the identity
problem concrete rather than theoretical; or a maintained YAML serde crate
with an editing model appearing, at which point the one-module containment
above is what makes the swap cheap.

**Ignores carry exit conditions.** The `yaml-rust` entry is the good
example, the `bincode` entry the bad one. Both are rewritten to name the
condition that deletes them.

**`sha2` added, under this change's own rule.** `integrity` in the lock has
to be infeasible to forge, which rules out `uze-core`'s existing FNV digest
— that one identifies content and authenticates none of it, and the module
now says so in as many words. `sha2` is RustCrypto, MIT OR Apache-2.0, pure
Rust, no C in the four-target release matrix. It appeared in `Cargo.lock`
already but nothing depended on it, so this is a genuine addition, not a
free one; the justification is written here because the rule says an
addition must carry one.

## Risks / Trade-offs

- `fancy-regex` may highlight some syntaxes differently, and is slower on
  pathological patterns. Verified by comparison, not assumed.
- Staying on bincode 1.3.3 means carrying a dependency that will not get
  fixes. Accepted knowingly: the format is internal to UZE's own two
  processes, never parsed from anything a third party sends.

## Candidate ADRs

- **Dependency provenance as a written rule** — it constrains every future
  change and is the kind of decision that decays silently if it lives only
  in a PR discussion. Worth an ADR at archive time if it holds.
