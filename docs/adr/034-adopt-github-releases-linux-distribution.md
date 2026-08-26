# Adopt GitHub Releases as the official Linux distribution channel

Status: Accepted

Decision-maker: Romullo Sousa (hiukky)

## Context

The README's quick start advertises `curl -fsSL https://hiukky.com/uze/install.sh | sh`,
but today nothing serves that URL: the repo has no installer, no release
pipeline, and no published binaries. `docs/versioning.md` already requires a
newly incremented SemVer pre-release before any distributable binary, and
`release.toml` already commits and tags `v{{version}}` locally via
cargo-release — yet nothing consumes those tags to produce artifacts. The
only working install path is building from source (`cargo install --path .`).

The ecosystem uze provisions already normalizes on exactly this shape —
Claude Code and Antigravity CLI install via `curl …/install.sh | bash`,
Codex via `curl …/install.sh | sh` (see `crates/uze-integrations`). uze
adopting the same shape keeps its own distribution consistent with the
harnesses it manages.

The durable choice is the download host and artifact contract: everything
downstream (the installer URL, artifact naming, checksum format, install
semantics) becomes part of a public contract that is expensive to change once
people install from it.

## Decision

GitHub Releases is uze's official Linux download host, consumed by a
repo-root `install.sh` that is the canonical installer implementation
(`curl -fsSL https://hiukky.com/uze/install.sh | sh`). The `web/`
directory and the `hiukky.com/uze/` serving path are deliberately out of
scope — how that URL reaches the content is owned outside this repo.

Per-release artifacts, attached to the `v<version>` release where
`<version>` comes from the cargo-release bump of
`[workspace.package].version` (the single version source; the release job
only asks which bump to apply, defaulting to `alpha`):

- `uze-x86_64-unknown-linux-gnu.tar.gz`
- `uze-x86_64-unknown-linux-musl.tar.gz`
- `uze-aarch64-unknown-linux-gnu.tar.gz`
- `uze-aarch64-unknown-linux-musl.tar.gz`
- `SHASUMS256.txt` — `sha256sum` of every tarball in that release

`install.sh` semantics (Linux only for now):

- Pure POSIX sh, `set -eu`; refuses non-Linux OSes and unsupported
  architectures (x86_64, aarch64 only) with a clear message.
- Selects gnu vs musl by probing `ldd --version` for `musl`; targets
  `latest` by default, pinned releases via `UZE_VERSION` (semver without
  the `v` prefix).
- Downloads archive and `SHASUMS256.txt` from the same release, verifies
  the archive hash, and fails closed on any mismatch or missing entry —
  nothing is installed unverified.
- Installs to `$UZE_BIN_DIR`, else `$XDG_BIN_HOME`, else `~/.local/bin`
  with `install -m 0755`, and verifies the installed binary with
  `--version`; prints a PATH hint when the target is not on `PATH`.
- `UZE_BASE_URL` overrides the download root (mirrors; the offline test
  seam). The script is idempotent: re-running replaces the binary.

Releasing is a single manual action — a **Release** GitHub Actions
workflow (`workflow_dispatch`, `contents: write`, concurrency guarded). A
`prepare` job gates on the `make check` pre-release-hook (fmt + clippy +
tests; nothing is bumped if it fails), bumps every workspace crate in
lockstep via cargo-release (tag deferred with `--no-tag`), creates the
`chore(release): bump workspace version to <v>` commit, regenerates
`CHANGELOG.md` with git-cliff (`-t <v>` names the section before the tag
exists; otherwise git-cliff sees an empty range and omits it) and folds it
into that same commit, creates the annotated `v<v>` tag on that final
commit, then pushes branch + tag. Four native-runner build jobs
(gnu/musl × x86_64/aarch64) then build the tarballs from the tag, and a
`publish` job uploads assets + checksums to the `v<v>` release, creating
it first when needed. Re-runs upload with `--clobber`, so a failed publish
can be repaired without renaming assets.

An offline fixture suite (`tests/scripts/installer-test.sh`, `make
test-installer`, gating CI) exercises glibc/musl detection, pinned and
latest URL shapes, checksum-mismatch refusal, and unsupported
platform fail-closed paths against synthetic artifacts served over
localhost HTTP — deterministic, zero network, zero tokens.

## Consequences

Easier:

- Install path matches the README and harness-ecosystem convention; any
  Linux machine with `curl` + `sh` can install without Rust tooling.
- Artifacts are rebuildable and non-authoritative (consistent with the
  derived-artifact principle): a release can be regenerated from the tagged
  source, and the checksum ledger makes tampering or corruption visible.
- The manual, idempotent workflow fits the pre-release cadence: an alpha
  release is a deliberate act, and the version stays single-sourced.

Harder:

- GitHub release assets only serve anonymous downloads from a public
  repository; while `hiukky/uze` is private, `curl | sh` cannot work for
  third parties (installer still installs today for the owner, and the
  fixture suite keeps the contract verified meanwhile). Making the repo
  public is the unlock, not a code change.
- Four native build jobs make a release slower than a single source build;
  acceptable at the current cadence. musl artifacts depend on the workspace
  staying free of C dependencies (pure-Rust only) — a new C link dependency
  would break the musl matrix until revisited.
- Checksums authenticate the archive but not its provenance end-to-end;
  transport is still plain HTTPS to GitHub. Signing is deferred.
- `install.sh` itself is served from outside this repo (hiukky.com wiring),
  so the script and its serving point can drift while they are wired up.

Non-goals (explicitly out of scope): macOS/Windows installers; a
self-update command (`uze update` remains distribution-independent); binary
signing; publishing to crates.io (already refused by the workspace);
serving `https://hiukky.com/uze/install.sh` (owned outside this repo).

## Implementation Plan

- **Affected paths**: `install.sh` (repo root, canonical installer);
  `.github/workflows/release.yml` (manual Release workflow);
  `.github/workflows/ci.yml` (new `installer` job: `shellcheck install.sh` +
  `sh tests/scripts/installer-test.sh`); `tests/scripts/installer-test.sh`
  (offline fixture suite); `Makefile` (`test-installer` target);
  `docs/versioning.md` ("Releasing a binary" section).
- **Pattern**: ecosystem convention from uze's own integrations
  (`curl …/install.sh | sh`); version single-source from Cargo.toml;
  env-var seams (`UZE_BASE_URL`) so the installer is testable offline;
  fail-closed verification in the same spirit as receipt/inspect-before-
  detach lifecycle safety.
- **Avoid**: embedding a version or asset list in `install.sh` (latest
  resolves it); gating the installer on repo visibility; version inputs in
  the release workflow beyond the bump kind (the version always comes from
  the workspace single source); touching `web/` or hiukky.com serving
  (owned outside this repo); new Rust dependencies.
- **No configuration changes**: no new runtime deps, no Store/Engine/
  Router/IntegrationPort changes, no CLI surface.

### Verification

- [x] `make test-installer` passes (17 offline assertions: syntax, glibc
      latest, musl detection, pinned version, checksum mismatch refusal,
      unsupported OS/arch refusal).
- [x] `shellcheck install.sh` is clean; `sh -n` parses under `/bin/sh`.
- [x] `actionlint` is clean on `ci.yml` and `release.yml`.
- [ ] `cargo test --no-fail-fast` and the rest of the CI gate still pass
      (no Rust changes; the installer job is additive).
- [ ] Manual: run the Release workflow on `main` — it bumps to the next
      alpha, commits `chore(release)` with the regenerated CHANGELOG.md,
      tags `v<version>`, pushes, builds the four tarballs, and the
      workflow posts the release, verified by `gh release view`.
- [ ] Manual: on a fresh Linux machine, `curl …/install.sh | sh` installs
      the matching target-triple binary and `uze --version` prints the
      released version. (Blocked while the repo is private and the
      hiukky.com/uze serving path is unwired; the fixture suite stands in.)