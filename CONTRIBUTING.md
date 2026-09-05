# Contributing to UZE

UZE is small enough that one person can hold it in their head, and it is
kept that way on purpose. These rules exist so that a change from anyone
reads, tests and ships exactly like a change from the maintainer. They are
not suggestions: a pull request that does not follow them is sent back
before anyone reads the code.

By contributing you agree that your work is licensed under the
[Apache License 2.0](LICENSE), the same as the rest of the project.

## Before you write code

- **Read [`AGENTS.md`](AGENTS.md).** It is the architecture guide, the
  build reference and the list of boundaries the test suite enforces.
  Every rule in it applies to humans and to coding agents alike.
- **Open an issue first for anything non-trivial.** A bug with a
  reproduction or a proposal with a stated problem. Do not open a large
  pull request cold; agree on the shape of the change before spending a
  week on it.
- **Structural changes go through OpenSpec.** A change that adds a
  crate, moves a boundary, adds a capability kind or a harness, or
  reverses a decision recorded in `docs/adr/` starts as a change under
  `openspec/changes/` (proposal, design, specs, tasks). Read the
  relevant ADRs first; do not reopen a decision without saying which one
  you are reopening and why.
- **No drive-by refactors.** A pull request does one thing. Renames,
  formatting sweeps and "while I was here" cleanups are separate pull
  requests or not at all.

## Toolchain

- Rust stable, edition 2024, MSRV **1.97** (`rust-version` in
  `Cargo.toml`). Code that needs a newer compiler is not accepted until
  the MSRV is raised in its own pull request.
- Python 3 with `ruff` for `conformance/`.
- [`lefthook`](https://lefthook.dev) for the git hooks. Run
  `lefthook install` once after cloning; the hooks mirror the fast half
  of CI so that a push that would fail never leaves your machine.

## The gate

Nothing merges unless all of this is green, locally and in CI:

```bash
make check                      # fmt + clippy (warnings denied) + cargo-deny + tests + ruff
openspec validate --all --strict
```

`make check` needs two tools CI installs for itself:
`cargo install cargo-deny cargo-about --locked`. They are dev tooling and
never appear in `Cargo.toml`.

`ci.yml` is the source of truth for what gates a merge; `make check` is
the local proxy. Specifically:

- `cargo fmt --check` clean. No exceptions, no `rustfmt::skip`.
- `cargo clippy --all-targets -- -D warnings` clean. Do not add
  `#[allow]` to silence a lint; fix the code or make the case in the
  pull request for a crate-wide configuration.
- `cargo test --workspace --no-fail-fast` passes. A test that is flaky is
  a bug in the test; fix it or delete it, never `#[ignore]` it to get
  green.
- `cargo deny check` clean: licence policy, advisories, bans and sources
  (`deny.toml`). A licence outside the allowlist is not allowlisted to get
  green — say so in the pull request and let the dependency decision be
  made. An `unmaintained` advisory may be accepted in `deny.toml` with a
  written reason that names what would remove it; a vulnerability never is.
- `CREDITS.md` is generated. A dependency change regenerates it with
  `make attributions`; CI fails when it drifts from `Cargo.lock`. Edit
  `about.hbs`, never the file.
- Coverage does not drop below the thresholds in `ci.yml`.
- The conformance verticals (`make lab-run`) pass for every harness a
  change touches. A harness that cannot deliver part of a contract
  declares it through `bindings.unsupported` with a reason; it never
  omits the check.

## Code

- Clean and self-documenting: expressive names, small functions, and a
  comment only for a non-obvious *why*. Never restate what the code says.
- Respect the dependency direction in `AGENTS.md`. The architecture suite
  fails on a violation; **never raise a budget** and never add to
  `sanctioned` to make it pass.
- `unsafe` needs a `// SAFETY:` comment stating the invariant, and a
  reviewer will check it.
- No new external dependency without a stated reason in the pull
  request. A dependency that becomes part of a public contract or a
  long-term boundary gets an ADR at archive time.
- The project is pre-1.0 and ships **no compatibility layers**. Stale
  state is fixed by cleaning data, not by permanent migration code.
- Vendor-specific knowledge lives in `uze-integrations` only. Naming a
  harness anywhere in `uze-core`, `uze-application` or `src/` fails a
  test.

## Tests

- A behaviour change comes with the test that would have caught the
  regression. A bug fix comes with a test that fails before the fix.
- Put the test where `tests/README.md` says it belongs (L0 unit through
  L4 conformance). Do not add a new top-level test binary when a domain
  suite already exists.
- Tests run in an isolated `TestEnvironment` from `uze-testkit`. A test
  that reads the developer's real `~/.uze`, `$HOME` or `PATH` is
  rejected.
- Never shell out to `kill` for a negative pid anywhere in the workspace.
  Use `libc::kill`. The history is in `AGENTS.md`; it once killed the
  whole login session.

## Commits

Every commit follows [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <imperative, lowercase description>

<why this change was needed, not a restatement of the diff>
```

- **Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
  `build`, `ci`, `chore`, `revert`. The type describes the change, not
  the request that prompted it.
- **Scope** is the crate or area (`tui`, `core`, `conformance`, `hooks`,
  `setup`, `terminal`). Omit it only for repo-wide changes.
- **Breaking changes** carry `!` after the type/scope and a
  `BREAKING CHANGE:` footer. Pre-1.0 this still matters: it is what
  drives the version bump.
- The body says *why*. `CHANGELOG.md` is generated from these messages
  by `git-cliff` (`make changelog`); a message you would not want in the
  changelog is a message that needs rewriting.
- **No AI attribution trailers.** `Co-Authored-By` lines for a coding
  agent, session links and similar are stripped before a commit is
  pushed. The author is the human who takes responsibility for the
  change.
- Commit on your own branch, never on `main`. Rewriting history that
  another person has pulled is not done without agreement.

## Pull requests

- **One concern per pull request**, small enough to review in one
  sitting. A pull request over roughly 400 changed lines of production
  code needs a reason in its description, and will usually be asked to
  split.
- **Branch names:** `<type>/<short-kebab-summary>` (`feat/portable-agent`,
  `fix/drawer-loading`). Agents launched by UZE work on `agent/<id>`.
- The title is the Conventional Commit line of the eventual merge. The
  description states the problem, the approach, what was considered and
  rejected, and how it was verified. Link the issue and, where one
  exists, the OpenSpec change.
- **Pull requests are squash-merged.** `main` is linear; every commit on
  it is one reviewed change with a conventional title, taken from the
  pull request title. The body is assembled from the branch's own commit
  messages, so write each of them as something worth reading on `main` —
  and trim the fixups out in the merge box, which stays editable. The
  pull request description is not the commit message: it is written for a
  reviewer, and a rich one full of tables reads badly in `git log`.
- Rebase on `main` before asking for review, and again if `main` moved
  under you. Merge commits into a feature branch are not accepted.
- A pull request is merged by a maintainer, only after CI is green and
  every review thread is resolved by the reviewer who opened it.
- Update documentation in the same pull request: `AGENTS.md` for a new
  boundary or command, `docs/architecture/invariants.md` for a newly
  guarded property, the OpenSpec specs for a changed contract. Do not
  create Markdown files for implementation notes or plans; ephemeral
  notes stay out of the tree.

## Versioning and releases

UZE follows [Semantic Versioning 2.0.0](https://semver.org). Until 1.0
every release is an explicit pre-release, `0.y.z-alpha.N`, and the public
contract can change between them; `docs/versioning.md` says what each
component means at this stage.

- The one version source is `[workspace.package].version` in the root
  `Cargo.toml`. Every crate inherits it; nothing else carries a version.
- A pull request never touches the version. Releases are cut by a
  maintainer through the **Release** workflow, which bumps the version,
  regenerates the changelog, tags, and publishes the Linux binaries to
  GitHub Releases (ADR-034). No binary is distributed from a build whose
  version was not bumped.
- The bump follows the Conventional Commits since the last release: a
  `BREAKING CHANGE` bumps `y`, a `fix` bumps `z`, and an ordinary
  development delivery bumps `alpha.N`. Choosing the bump is part of the
  release, not of the pull request.

## Security

Never open a public issue for a vulnerability. The policy — where to report,
what a usable report contains, and what happens after — is in
[`SECURITY.md`](SECURITY.md).

## Conduct

Be direct and be kind. Review the code, not the person. Disagreements are
settled by evidence: a failing test, a measurement, a recorded decision.
Anyone who cannot do that is asked to leave.
