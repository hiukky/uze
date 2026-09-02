## Why

The dependency direction this project documents — `CLI/TUI → uze-application
→ uze-core` — is an aspiration, not a fact. Measured today, `src/` names
`uze_core::` in 19 places across 9 production files, more than it names
`uze_application`. A domain change therefore ripples straight into the
frontend, which is the concrete cause of "I want to add one module and I have
to touch several unrelated places".

Three further seams are drawn in the wrong place, and each one gets more
expensive to move the longer it stands:

- **Git is spoken by two crates with incompatible conventions.**
  `uze-core::worktree::git` treats any non-zero exit as failure;
  `uze-extensions::git_diff::run_git` treats `1` as success because `diff`
  uses it. A third caller has to guess, and a write lock over refs is
  worthless while two modules spawn Git independently.
- **An extension draws into the host's frame.** `git_diff::render` takes
  `&mut ratatui::Frame` and pushes its own `Rect` hits, so an extension can
  never be anything but compiled-in code in this process. The cost of
  changing that contract scales with the number of extensions; there is
  exactly one today.
- **`ExtensionHit` is a flat enum in the host crate.** The crate's own doc
  already says it must be namespaced once a second extension exists.

None of this is about opening the product to third-party authors. Every
change below pays for itself in the closed core. Together they happen to be
what makes opening it later a small step rather than a rewrite.

## What Changes

- Add a deterministic architecture suite (`tests/architecture/`) that
  enforces layering as data: each rule names a scope, a forbidden path
  prefix, a reason, a permanent sanctioned list, and a per-file debt budget
  that may only shrink. Existing violations are frozen, not fixed, so the
  guardrail lands before the refactor.
- Extract `uze-git`: the single transport for speaking to the Git binary —
  spawn conventions, per-command exit-code classification, porcelain
  parsers — consumed by both the domain and the presentation side.
- Replace the extension render contract: an extension returns a serialisable
  view model describing its content; the host owns rendering, geometry, and
  the palette. Hit-testing moves to the host, which already receives
  semantic hits on the input side.
- Namespace `ExtensionHit` per extension.
- Record that extension code is a distinct trust class from plugin bytes,
  and lock the property that an extension never touches UZE's own state.
- Restate the layering rules in `AGENTS.md` as enforced facts, each naming
  the test that proves it.

## Capabilities

### New Capabilities

None. No user-visible behaviour changes.

### Modified Capabilities

None.

## Impact

- `tests/architecture/` (new suite), `AGENTS.md`,
  `docs/architecture/invariants.md`.
- `crates/uze-git` (new), `crates/uze-core/src/worktree.rs`,
  `crates/uze-extensions/src/git_diff.rs`.
- `crates/uze-extensions/src/lib.rs` (view model, `ExtensionHit`),
  `src/ui/orchestrator/` (host-side rendering of the view model).

## Non-goals

- No extension loading mechanism (WASM, subprocess, dynamic library).
- No declarative integration manifest.
- No process boundary for `uze-application`.
- No layout DSL: the view model is Rust types that already serialise.
- No widget added to the host vocabulary that only one extension needs.
