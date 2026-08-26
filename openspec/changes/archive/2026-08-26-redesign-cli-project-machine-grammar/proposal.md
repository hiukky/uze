## Why

UZE already separates **Project** state (`agents.lock`, `AGENTS.md`, `.agents/`) from **Machine** state
(`~/.uze/store`, marketplace registry, harness provisioning) at the application layer — ADR-016
("Project Agent Environment") formalized this split and even specified `uze <plugin>@<marketplace>` as
the project shorthand. But the CLI never fully adopted it: `uze <plugin>@<marketplace>` is a raw
pre-`clap` string check on `argv[1]` in `main.rs` rather than part of the command grammar; `uze remove`
silently falls back from project-scope to machine-scope depending on whether a lock exists; and
machine-level verbs (`add`, `list`, `inspect`, `update`) sit at the root next to project verbs
(`install`, `status`, `context`), duplicating the already-namespaced `plugin` subcommand. The result is a
CLI whose grammar does not communicate its own architecture, and a shorthand implementation that is
exactly the kind of main.rs parsing hack the project's own conventions try to avoid.

This change closes the semantics, grammar, migration, and DX questions needed before any of that is
implemented — no code changes here.

## What Changes

- Formalize the CLI's root-vs-namespace grammar around the existing Project/Machine boundary: root-level
  commands (`install`, `remove`, `status`, `context`, `<plugin>@<market>`) operate on the current project;
  `market`/`plugin`/`harness` namespaces operate on machine-level resources.
- Move the `<plugin>@<market>` shorthand from an `argv[1]` string check into a documented,
  deterministically testable resolution rule that composes with `clap`'s built-in dispatch (built-ins take
  precedence; `@` is required; no bare-name shorthand). **BREAKING**: a plugin/marketplace name containing
  no `@` no longer has an implicit shorthand form.
- **BREAKING**: `uze remove <plugin>` becomes strictly project-scoped (updates `agents.lock` only, never
  deletes the machine-level package). This reverses the "project-then-fallback-to-global" behavior ADR-016's
  own design doc specified for `remove` — documented below as a deliberate, cited supersession, not an
  oversight.
- **BREAKING**: `uze add`, `uze list`, `uze inspect`, `uze update` move under `uze plugin ...`
  (`plugin install`, `plugin list`, `plugin inspect`, `plugin update`); they no longer exist at the root.
- Rename the `marketplace` subcommand to `market` (CLI grammar only — "Marketplace" remains the product/doc
  term, `marketplace.json` the manifest filename, `MarketplaceSource`/`marketplace_add` etc. the internal
  names). Add `uze market inspect <name>` (marketplace-level detail; does not exist today only at the
  plugin-within-marketplace granularity).
- Introduce a machine-level `uze harness` namespace (`list`, `inspect`, `setup`) as a thin re-presentation of
  existing `doctor`/`setup` data — no new provisioning behavior. `uze setup`/`uze doctor` remain as
  root-level convenience commands.
- Redesign `uze --help` (and per-namespace `--help`) to show the Project/Machine split explicitly.
- Explicitly decline to add `plugin enable`/`plugin disable` (no defined semantics yet).
- Do **not** rename `marketplace.json` → `agents.json` in this change; document the impact and file-identity
  distinction (UZE's own registry manifest vs. the unrelated, vendor-dictated `.claude-plugin/marketplace.json`
  / `.agents/plugins/marketplace.json` catalogues Claude/Codex integrations already own) as a separate,
  future decision.
- No implementation in this change: this proposal, its spec deltas, an ADR, and a task breakdown are the
  deliverables. Application-layer use cases needed for the new grammar (`add_project_plugin`,
  `remove_project_plugin`, `install_project_environment`, `plugin_install`, `marketplace_add/list/remove`,
  `remove_plugin`) already exist and require no semantic change — only a small new `market inspect` and
  `harness list/inspect` read model, and CLI/main.rs rewiring.

## Capabilities

### New Capabilities
- `cli-command-grammar`: the root-vs-namespace command grammar, built-in-precedence and `<plugin>@<market>`
  shorthand resolution rule, error/edge-case semantics, and `--help` information architecture — the
  cross-cutting contract every other CLI capability composes with.
- `harness-namespace`: `uze harness list|inspect|setup` as a thin, non-behavior-changing re-presentation of
  existing harness detection/provisioning/setup data under a machine-level namespace.

### Modified Capabilities
- `project-agent-environment` (`openspec/changes/project-agent-environment/specs/project-agent-environment/spec.md`):
  tightens `uze remove <plugin>` to strictly project-scoped, removing the global-fallback behavior that
  change's own design.md specified. Everything else in that capability (`<plugin>@<market>` mutates
  `agents.lock`; `uze install` reproduces it; global admin commands never touch the lock) is reaffirmed,
  not changed.
- `marketplace` (`openspec/changes/marketplace-registry-and-plugin-install/specs/marketplace/spec.md`):
  CLI verb renamed `marketplace` → `market` (source/state/domain names unchanged); adds a marketplace-level
  `inspect` operation.

Not listed: `plugin` (`openspec/changes/marketplace-registry-and-plugin-install/specs/plugin/spec.md`).
Its requirements are reaffirmed, unchanged — removing the root-level `add`/`list`/`inspect`/`update`/`remove`
aliases that used to blur its boundary is a grammar change, not a behavior change to `plugin` itself, so no
delta is warranted there (see design.md's migration table for the root-alias disposition).

## Impact

- **Affected code**: `src/main.rs` (full CLI grammar rewrite), `src/shim.rs` (unaffected — argv[0] dispatch
  is independent of this grammar), `crates/uze-application/src/application/{marketplace,project_environment}.rs`
  (new `market_inspect`, `harness_list`/`harness_inspect` read models), `src/ui/worker.rs` (no forced change,
  but now has real project-scoped use cases available if the TUI adopts them later — out of scope here).
- **Affected docs**: `docs/adr/016-project-agent-environment.md` gets a superseding note (not an edit) via
  the new ADR this change proposes; README/CLI help text; `openspec/changes/project-agent-environment` and
  `openspec/changes/marketplace-registry-and-plugin-install` gain modified-capability deltas here rather than
  being edited in place (OpenSpec changes are immutable once written).
- **Not affected**: `uze-core` (Store, `project_lock`, capability router), `uze-integrations` (harness
  adapters), ADR-009 receipt/drift lifecycle, `agents.lock` schema (ADR-017), Store-as-cache semantics.
- **No breaking change to file formats** — only to the CLI's command surface, which is acceptable pre-1.0
  (alpha) per project convention; see design.md's migration table for the full command-by-command
  disposition and the short-lived-alias question.
