## 1. Guidance

- [x] 1.1 `AGENTS.md` gains a `## Dependencies` section: provenance tiers, what to refuse, what to check and write down.
- [ ] 1.2 Cross-check it against `openspec/config.yaml`'s `context` so an agent reaching for a crate finds the same rule from either entry point.

## 2. syntect off Oniguruma

- [x] 2.1 `crates/uze-extensions` and `crates/uze-theme` now carry `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`; `onig` and `onig_sys` are gone from `Cargo.lock`.
- [x] 2.2 Guarded by tests rather than by a one-off comparison: `crates/uze-extensions/src/git.rs`'s `tests::highlighting` module pins the two properties that separate working highlighting from silently collapsed highlighting — across rs/py/js/go/md/yaml/json/sh, a line comes back in more than one colour, and the spans reconstruct the source byte for byte — plus block-comment state carrying across lines, and unknown-extension fallback. A control test asserts plain text comes back in a *single* colour, so the multi-colour assertion demonstrably discriminates.
- [ ] 2.2b Not claimed: no A/B run against the `onig` backend was performed, so historical parity is unproven. The tests above guard the failure mode going forward, which is what matters for a backend that is now pure Rust; close this out by running the comparison once if a highlighting regression is ever reported.
- [ ] 2.3 Confirm `onig`/`onig_sys` are gone from `Cargo.lock`, and drop the musl toolchain workaround in `release.yml` if nothing else needs it.
- [x] 2.4 `make attributions` run; `CREDITS.md` regenerated. Caught by CI, not locally — the `Licences` job runs `attributions-check` and the tree changed in both directions (Oniguruma chain out, `sha2` chain in).

## 3. bincode — evaluated, staying

- [x] 3.1 Evaluated migrating to `wincode`; not worth it. `wincode` is `0.6.1`, so the move trades a frozen-but-known dependency for an unstable one, against this change's own rule, and the wire format is agreed between two processes — the cost is a protocol version, not a bump.
- [ ] 3.2 Rewrite the `RUSTSEC-2025-0141` ignore to say it is carried deliberately, naming what would reopen it: an advisory against the format itself, or the protocol being versioned for another reason anyway.
- [ ] 3.3 Record in `crates/uze-terminal` why the format is pinned, so the next reader does not re-open it: the bytes are internal to UZE's own two processes and never parsed from a third party.

## 4. noyalib — evaluated, staying under a written exception

- [x] 4.1 The exception is drafted in `design.md` ("`noyalib` stays, as a written exception"): what it is, why it is kept over each alternative that was evaluated, what limits the exposure to one module, and the three conditions that remove it. This closes `project-agent-environment` 9.6's "record the exception where `AGENTS.md` asks for it" as far as this change can: the text exists and is reviewable.
- [ ] 4.2 Copy the exception's four sentences into `AGENTS.md`'s `## Dependencies` section, under the refuse list, so a reader of the policy finds the one crate that breaks it without leaving the page. Needs the repository owner: `AGENTS.md` is this project's own authored context, not a generated bridge.
- [ ] 4.3 Fail the build on the *next* one. A `0.0.x` direct dependency not named in a written-exception table should not be able to enter in passing inside a large change, which is exactly how this one did — either a `deny.toml` `[bans]` entry or a scan in `tests/architecture`.

## 5. Advisory hygiene

- [ ] 5.1 Rewrite both `deny.toml` ignores to name the condition that deletes them, not only the reason they are tolerated.
- [ ] 5.2 Install `cargo-deny` locally (absent on this machine) and confirm `cargo deny check` is clean.

## 6. Validation

- [ ] 6.1 `make check` clean; `openspec validate harden-dependency-provenance --strict` passes.
