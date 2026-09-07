## Why

uze had never been built or run on macOS. `ci.yml` was Ubuntu end to end and
`release.yml` shipped four Linux targets, so the answer to "does it work
there" was not *no* — it was that nobody had asked the machine.

ADR-034 named macOS installers an explicit non-goal. That was the right call
at the time: it was written to get *a* distribution out, and adding a second
platform to an unproven pipeline would have delayed the first one. What has
changed is that the question is now answerable — a macOS runner costs nothing
on a public repository, and the code turned out to be far closer than the
docs claimed. `cargo check --target aarch64-apple-darwin` was already clean;
what was missing was behaviour, and behaviour is testable.

The gap was never the compiler. It was five silent divergences: `/proc`
readings with `None`-returning stubs that no platform could ever satisfy, a
journey check that observed nothing and reported "not running", `sed -i` in
GNU spelling, a scratch root that production code would never match through
`/private/var`, and a `PATH` line written to a file macOS bash does not read.
Each of them fails quietly, which is why none had ever been noticed.

## What Changes

- **Prove it.** A macOS workflow builds, runs, lints, tests and performs the
  product journeys on Apple Silicon. Not a job inside `ci.yml`: until it
  settles green the answer is unknown, and an unknown must not block a pull
  request that has nothing to do with macOS.
- **Ask the kernel once.** `process_probe` holds the four facts only the
  kernel has — the peer on a socket, the image a pid runs, where it stands,
  what it inherited — answered by `/proc` on Linux and `libproc`/`sysctl` on
  macOS. The decisions built on them are written once, and `None` means
  *unknown*, never *no*.
- **Ship it.** `release.yml` packages `aarch64-macos` and `x86_64-macos`,
  both cross-compiled on one Apple Silicon runner, and `install.sh` reads
  `uname` and picks them. This is the part that reverses ADR-034's non-goal.
- **Say so.** The installation page stops telling macOS users to build from
  source.

## What this does not change

- **Windows.** Still out of scope, and still stated as such.
- **Notarization.** The macOS binaries are ad-hoc signed by the linker, which
  is what `curl | sh` needs. A tarball fetched through a *browser* is
  quarantined by Gatekeeper and needs one `xattr -d`. Notarizing means an
  Apple Developer account and signing material in CI secrets — a recurring
  cost and a new class of secret, for a path most installs do not take. The
  release notes and the installation page both say this plainly rather than
  letting a user discover it.
- **The Conformance Lab.** It stays Linux and Docker. Its isolation is
  `--network internal`, a read-only root and dropped capabilities — that is
  the experiment, not the packaging, and macOS runners have no Docker.
