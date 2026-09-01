# Explicit Project/Machine Boundary in the CLI Command Grammar

Status: Accepted

## Context

ADR-016 established, at the application layer, a firm separation between Project state (`agents.lock`,
`AGENTS.md`, `.agents/`) and Machine state (`~/.uze/store`, marketplace registry, harness provisioning),
and specified `uze <plugin>@<marketplace>` as the project-scoped shorthand for "this project uses X." That
application layer (`add_project_plugin`, `remove_project_plugin`, `install_project_environment`,
`plugin_install`, `marketplace_add/remove/list`, `remove_plugin`) already implements the boundary
correctly and needs no change.

The CLI surface (`src/main.rs`) never finished catching up to it: the shorthand is a raw `argv[1]`
string check performed *before* `clap` parses anything, with its own hand-rolled, non-validating flag
parser; machine-level commands (`add`, `list`, `inspect`, `update`) sit at the CLI root next to
project-level commands (`install`, `status`, `context`) with no positional signal distinguishing them from
the already-namespaced, functionally-duplicate `plugin` subcommand; and `uze remove` — per
`project-agent-environment/design.md`'s Decision #8 — deliberately falls back from project-scoped removal
to machine-scoped removal when no lock is present or the plugin isn't declared in it.

That fallback is the one place this decision reverses something already accepted: a root command whose
target scope depends on ambient, invisible state (does a lock exist? does it mention this plugin?) is
incompatible with a grammar whose entire premise is that scope is inferable from a command's position. Kept
as-is, `uze remove flow` could delete a machine-wide package other projects depend on, from inside a
project whose lock simply doesn't happen to mention `flow`, with no indication at the call site.

## Decision

We will make the `uze` CLI's command grammar carry Project vs Machine scope in its structure, not in
runtime state:

1. **Root-level commands are unconditionally project-scoped.** `install`, `remove`, `status`, `context`,
   and the `<plugin>@<market>` shorthand only ever read or write the current project's `agents.lock` /
   `AGENTS.md` / `.agents/`. No root command mutates machine state as its primary effect (the shorthand's
   Store-acquisition-as-side-effect is documented, not hidden).
2. **`market`, `plugin`, and `harness` are the exhaustive machine-level namespaces.** Every command that
   mutates or inspects `~/.uze/*` global state lives under exactly one of these three, never at the root.
   `market` renames the existing `marketplace` verb (domain name, state filename, and internal types are
   unchanged — this is a CLI-vocabulary decision only). `harness` is new, and deliberately thin: `list`,
   `inspect`, `setup`, re-presenting data `doctor`/`setup` already compute, no new provisioning semantics.
3. **`uze remove <plugin>` becomes strictly project-scoped**, superseding ADR-016 / `project-agent-environment`
   Decision #8's fallback clause. Outside a project, or when the plugin isn't in the project's lock, `uze
   remove` now fails with an error naming `uze plugin remove` as the machine-level equivalent, rather than
   silently reaching into machine state. All of ADR-016's other decisions (global admin never writes the
   lock; the shorthand requires `@`; `install` never silently re-resolves) are reaffirmed unchanged.
4. **`<plugin>@<market>` dispatch is resolved through `clap`'s own `external_subcommand` mechanism**, not a
   pre-`clap` string check — one parser, one documented precedence rule (named subcommands match first, by
   `clap`'s own generated matcher; anything else falls to the external variant), provable by a test that no
   built-in command name ever contains `@` (the shorthand's required, and only, dispatch signal).

**Alternatives considered:**
- *Keep `remove`'s fallback, add a `--global`/`--project` flag to disambiguate explicitly.* Rejected: adds
  a flag to memorize for a distinction the grammar should carry structurally; doesn't fix the "silent
  scope depends on invisible state" problem for the flag-less default case.
- *Leave the four machine-level root commands (`add`/`list`/`inspect`/`update`) in place alongside their
  `plugin` equivalents, permanently.* Rejected: keeps two spellings for the same operation indefinitely,
  which is exactly the "gramática... apenas uma coleção de comandos" the redesign exists to fix; this
  project is pre-1.0 alpha and has chosen a clean break over permanent aliases.
- *Keep the pre-`clap` string check, just harden it.* Rejected: still two independent argument-parsing
  code paths reachable from `main()`; `external_subcommand` gives one.

## Consequences

**Easier:** a command's scope is answerable by reading its name, not by knowing what state happens to
exist; `uze --help` can state the boundary once instead of it being implicit per-command; adding a future
machine-level concern has an obvious home (a fourth namespace) instead of another root command needing a
case-by-case scope judgment; the shorthand gets `clap`'s validation, error messages, and `--help` for free,
closing a real bug (unrecognized flags after `<plugin>@<market>` were silently ignored).

**Harder / accepted trade-offs:** `uze remove flow` run out of old habit, outside a project or against a
plugin not in the lock, now fails instead of falling through to what used to "just work" — this is the
deliberate, breaking part of this decision, mitigated only by a clear error message pointing at `uze plugin
remove`, not by a compatibility shim. Four root commands (`add`, `list`, `inspect`, `update`) disappear
with no built-in alias by default; scripts and muscle memory referencing them break at once (acceptable
pre-1.0 per project convention; an optional short-lived hidden-alias-with-warning window is documented in
design.md if a softer landing is wanted later, without changing this decision).

This decision does not touch `uze-core`, `uze-integrations`, the Store, ADR-009's receipt/drift lifecycle,
or the `agents.lock` schema (ADR-016) — it is scoped entirely to the CLI presentation layer and the two
small read-model additions (`market_inspect`, `harness_list`/`harness_inspect`) needed to serve it.

Status note: this ADR partially supersedes ADR-016 (`docs/adr/016-project-agent-environment.md`), narrowly
— only its `project-agent-environment/design.md` Decision #8 "`remove` disambiguated by context" clause.
Every other decision in ADR-016 stands, reaffirmed by this ADR rather than replaced.

Source change: openspec/changes/redesign-cli-project-machine-grammar/
