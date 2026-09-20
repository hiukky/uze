## Why

Two commands do nearly the same thing and differ in one column:

| | bytes in the Store | attached to harnesses | writes `agents.yaml`/`lock` |
|---|---|---|---|
| `uze plugin install git@ai` | yes | yes | **no** |
| `uze git@ai` | yes | yes | **yes** |

The distinction "is this recorded as this project's dependency" already
exists in the product — it is just spread across two spellings instead of
named. ADR-019 put the machine half under a `plugin` namespace to make scope
structural, which fixed a real ambiguity (`uze remove` used to fall back
from project to machine depending on whether a lock happened to mention the
plugin). But it left the duplication, and a namespace an operator must learn
to say something the working directory almost always already says.

Two further defects make the current shape unsafe to build on:

1. **There is no "not a project".** `project_root::resolve_project_root`
   ends in `.unwrap_or(start)` — any directory becomes a project, so
   `uze git@ai` run from `$HOME` would create `~/agents.yaml`.
2. **A nested `AGENTS.md` shadows the repository root.** The closure prefers
   the remembered `nearest_agents_md` over `is_repository_root`, so
   `repo/docs/AGENTS.md` makes `repo/docs` the project root. Nothing tests
   this case.

## What Changes

- **The `plugin` namespace is removed.** Its operations move to the root
  verbs. **BREAKING**: `uze plugin install|list|inspect|remove|update`
  disappear with no alias, matching ADR-019's own precedent of a clean break
  pre-1.0.
- **Scope is decided by where the command runs, and always reported.** In a
  project, the root verbs act on the machine *and* maintain the project's
  files. Outside one, they act on the machine and say that nothing was
  declared. The command names which scopes it touched — silence about scope
  is what ADR-019 was written against, and this replaces the structural
  signal with an explicit one rather than dropping it.
- **`-m` / `--machine` selects machine scope explicitly**, on the verbs that
  have two. It is not a "don't save" flag for install: it is the same word
  on every verb, and it is the only way a *script* can state intent, since
  a scripted world cannot express it by choosing a directory. The
  conformance Lab is exactly that consumer — all four verticals set up with
  `uze plugin install`, and `conformance/DECISIONS.md:529` records why
  machine scope had to be explicit there.
- **`uze remove <plugin>` keeps today's semantics**: it undeclares, and the
  bytes stay, because other projects share them. `uze remove <plugin> -m`
  is the machine removal, with the inspect-before-detach and drift safety
  ADR-009 already requires. **Nothing infers who else wants a package** —
  the machine registry under `state/projects/` records only what the
  workspace client has touched (`record::ensure` is called from `task.rs`,
  `conversation.rs` and `prompt_history.rs`, never from a CLI path), so a
  rule built on it would systematically under-report and delete bytes
  another project declares.
- **`uze inspect <plugin>` and `uze update [plugin]` return to the root.**
  This reverses part of ADR-019, which removed four root commands; the new
  ADR must say so and state what carries scope in their place.
- **Project root resolution gains a "not a project" answer**, and its
  precedence is reordered: nearest ancestor with `agents.yaml`; else the
  repository root; else nearest ancestor with `AGENTS.md`; else none.
  `AGENTS.md` can no longer shadow a repository root, and no directory
  becomes a project by being the one you stood in.
- **`--alias` / `--replace`** move to the root install form, keeping
  ADR-036's name-collision resolution reachable.

## Capabilities

### New Capabilities

- `command-grammar`: what the verbs are, how each decides its scope, how it
  reports the scopes it touched, and what counts as a project.

### Modified Capabilities

- `plugin`: its scenarios are written in the `uze plugin …` surface this
  change removes; the requirements stand, the spellings change.

## Impact

- `src/main.rs` — `PluginAction` removed, root verbs gain `-m`, dispatch.
- `crates/uze-core/src/project/project_root.rs` — precedence and the
  `Option` answer; `fallback_is_cwd_when_no_markers` and
  `prefers_agents_md_over_git` are rewritten, and the untested nested case
  gains one.
- `crates/uze-application/src/application/project_environment.rs` — the five
  `resolve_project_root(…)?` sites each decide "machine only" or "refuse".
- `src/command_performance.rs` — every renamed leaf reclassified; `remove`
  restated.
- `crates/uze-core/src/error.rs` — `PluginNotUsedByProject` and
  `NoProjectEnvironment` name a command that will not exist.
- `conformance/harnesses/{claude,codex,opencode,antigravity}/scenarios.py`
  and `conformance/DECISIONS.md`.
- `journeys/` — `02-packages/01,02,03`, `03-context/01`, `06-recovery/01`,
  `01-first-run/01`.
- `docs/adr/019-…` gains a successor; `plugins/uze/skills/**` and `docs/`
  carry the old spellings.
- ~18 citations of **ADR-038** for name-collision are wrong — that ADR is
  the terminal runtime server; the right one is **ADR-036**. Fixed here
  because this change touches the same files.
- No new dependency.
