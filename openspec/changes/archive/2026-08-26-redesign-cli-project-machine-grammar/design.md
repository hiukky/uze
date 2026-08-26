## Context

See `proposal.md` — Why. This section only records the audit findings the design rests on.

### 1. Current CLI command inventory (`src/main.rs`)

```
uze                                  → TUI (interactive terminal) or --help (non-terminal)
uze add <source> [--trust] [--format]        → app.add_plugin            (MACHINE)
uze list [--format]                          → app.list_plugins          (MACHINE)
uze inspect <plugin> [--format]              → app.inspect_plugin        (MACHINE)
uze remove <plugin> [--format]               → app.remove_project_plugin, THEN
                                                 falls back to app.remove_plugin (MIXED)
uze update <plugin> [--trust] [--format]     → app.update_plugin         (MACHINE)
uze install [path] [--trust] [--format]      → app.install_project_environment (PROJECT)
uze status [path] [--format]                 → app.status                (PROJECT)
uze context inspect|plan|reconcile [path]    → app.context_*             (PROJECT)
uze marketplace add|list|remove              → app.marketplace_*         (MACHINE)
uze plugin install|list|remove|update        → app.plugin_install, app.list_plugins,
                                                 app.remove_plugin, app.update_plugin (MACHINE)
uze doctor [--format]                        → app.doctor                (diagnostics)
uze setup [harness]                          → app.setup                 (diagnostics/bootstrap)

uze <plugin>@<market> [--trust] [--format]   → app.add_project_plugin    (PROJECT)
                                                 — NOT a clap Subcommand variant; a raw
                                                 argv[1].contains('@') check in main(),
                                                 before Cli::parse(), with its own
                                                 hand-rolled --trust/--format loop
                                                 (run_project_shorthand, main.rs:288-340)
```

`uze plugin install/list/remove/update` already duplicate `uze add/list/remove/update` almost verbatim —
both call the same application methods for `list`/`remove`/`update`, and `plugin install` differs from
`add` only in resolving a marketplace spec first. Nothing under `plugin` today does anything a root command
doesn't already do; the namespace exists but doesn't have exclusive ownership of anything.

`uze remove` is the one command whose current behavior is genuinely mixed: it tries
`remove_project_plugin` first, and on `NoLock`/`NotInLock` falls back to `remove_plugin` (global). This
fallback is not an oversight — it is `openspec/changes/project-agent-environment/design.md`'s Decision #8,
implemented exactly as specified: *"`remove` disambiguated by context (lock present + plugin in lock →
project; else → global)"*. This proposal's Decision D2 below reverses it — see the ADR for the full
argument.

### 2. The application layer already has the Project/Machine boundary this proposal wants the CLI to show

`crates/uze-application/src/application/project_environment.rs` and `.../marketplace.rs` +
`.../lifecycle/{install,update,remove}.rs` already implement, with no change needed for this proposal:

| Use case | Function | Touches `agents.lock`? | Touches machine Store? |
|---|---|---|---|
| Project uses a plugin | `add_project_plugin(plugin, marketplace, root, authority)` | writes | ensures present (side effect) |
| Project stops using a plugin | `remove_project_plugin(plugin, root)` | writes (removes entry) | never |
| Reproduce a project's lock | `install_project_environment(root, authority)` | reads only | ensures every locked plugin present |
| Install on this machine | `plugin_install(spec, authority)` → `add_plugin` | never | writes |
| Update on this machine | `update_plugin(id, authority)` | never | writes |
| Remove from this machine | `remove_plugin(id)` → `detach_and_remove` | never | writes (subject to ADR-009) |
| Marketplace source admin | `marketplace_add/remove/list` | never | writes registry only |

This is the single most important audit finding: **this is a CLI grammar problem, not an architecture
problem.** ADR-016 already established and the application layer already enforces the boundary; `main.rs`
just never finished exposing it. The risk profile of this change is therefore low — no new invariant is
being invented, only a thin presentation layer is being rewritten to match one that already exists
underneath it.

Two small additions are needed, both thin reads with no new business rule:
- `market_inspect(name) -> Result<MarketplaceDetail>` — filters `marketplace_list()`'s existing computation
  down to one entry; today only the full list exists.
- `harness_list()` / `harness_inspect(name)` — slices `doctor()`'s existing `harnesses: Vec<HarnessHealth>`
  by name; today only the full doctor report exists.

Neither reads new state or introduces a new invariant; both are extractions of data `doctor()` and
`marketplace_list()` already compute.

### 3. `<plugin>@<market>` shorthand is provably unambiguous against built-ins, but not formally proven today

`uze_core::project_lock::parse_plugin_marketplace_spec` already requires `spec.split_once('@')` and
charset-validates both halves (`is_ascii_alphanumeric() || '-' || '_'`) — this is already the exact,
already-tested formal grammar for the right-hand side of the shorthand. What's missing is only the
*left*-hand dispatch: today `main()` special-cases `argv[1].contains('@')` by hand, before `Cli::parse()`
ever runs, with a duplicate, less strict flag parser
(`run_project_shorthand`, main.rs:288-340) that silently ignores any flag it doesn't recognize (the `_ =>
{}` arm at line 308) — e.g. `uze flow@ai --trsut` (typo) proceeds without granting trust rather than
erroring. This is exactly the kind of "hack espalhado pelo main.rs" the request called out, and it has a
real correctness gap.

The soundness argument for *why* built-ins can never collide with shorthand is simple and provable: no
built-in command or subcommand name (root, `market`, `plugin`, `harness`, or any of their verbs) contains
`@`. Given that, and given the shorthand grammar *requires* `@`, a first argument's shorthand-or-not
classification reduces to one lexical fact, checkable independent of command registration order. See
Decision D3.

### 4. `marketplace.json` names two unrelated things

- **UZE's own marketplace registry manifest** (ADR-015): the contract a marketplace root's
  `marketplace.json` answers — `{name, plugins: [{name, source, description, keywords}]}`. Read by
  `uze-core/src/acquisition/marketplace.rs`, `uze-application/src/bootstrap.rs` (embedded official
  marketplace), and `uze-application/src/application.rs`'s `parse_marketplace_source`/
  `load_marketplace_manifest`. This is what "rename to `agents.json`" (request §12) is actually about.
- **Vendor-owned, derived native-plugin catalogues** that happen to share the filename because the *vendor*
  (Claude Code, Codex) defines it, not UZE: `.claude-plugin/marketplace.json`
  (`crates/uze-integrations/src/claude.rs:109`, `claude/plugin.rs`) and
  `.agents/plugins/marketplace.json` (`crates/uze-integrations/src/codex.rs:75`, `codex/plugin.rs`). These
  are `Derived Artifact`s (rebuildable, not authoritative — same status as every other UZE-owned catalogue)
  that republish UZE's installed packages *into the vendor's own native plugin marketplace format*. Claude
  Code's own plugin system requires that exact filename at that exact relative path; UZE does not control
  it and renaming UZE's registry manifest must never touch it.

These are unrelated files that happen to share a name by coincidence of both describing "a marketplace" in
their respective ecosystems. Conflating them in a rename would be a real correctness bug, not a
naming nit.

### 5. `parse_marketplace_source` does not support a GitHub `owner/repo` shorthand today

`crates/uze-application/src/application.rs:262-292`: a marketplace source is either a full URL
(`https://`, `http://`, `git://`, `ssh://`, `file://`) or a local path that must already contain
`marketplace.json`. `uze market add hiukky/ai` (the request §4 example) would today fail: `hiukky/ai` is not
a URL, and canonicalized as a relative path it will not exist (or will not contain `marketplace.json`) in
the general case. This proposal's CLI grammar does not depend on this shorthand existing — see Decision D6.

### 6. `PluginSource` has no `Local` variant

`uze_core::project_lock::PluginSource` is `Marketplace { marketplace, plugin } | Git { url, reference,
subdirectory }`. There is no way to lock a bare local-filesystem plugin into `agents.lock` today, by design
consistent with ADR-017's reproducibility goal (a local path on the author's machine cannot reproduce on a
clone). `add_project_plugin`'s signature (`plugin, marketplace, root, authority`) only ever constructs a
`Marketplace` source — even the `Git` variant the schema already models has no CLI path to populate it
directly (it exists only for round-tripping a lock hand-written or produced by tooling this proposal does
not add). This is a pre-existing, orthogonal gap; not something this proposal changes or needs to change —
see request §1's "local plugin/path" question, answered under Decision D1 below.

### 7. TUI is entirely machine-scoped today

`src/ui/worker.rs` calls `remove_plugin`, `update_plugin`, `plugin_install`, `marketplace_add` — never
`add_project_plugin`/`remove_project_plugin`/`install_project_environment`. The TUI has no notion of "used
by this project" today; everything it shows and mutates is machine state. See Decision D8 / request §14.

## Goals / Non-Goals

**Goals:**
- A CLI grammar where scope (Project vs Machine) is inferable from a command's position alone.
- A formally statable, testable precedence rule for `<plugin>@<market>` vs. built-ins.
- Full command-by-command migration disposition (KEEP/MOVE/RENAME/DEPRECATE/REMOVE) with new semantics
  spelled out.
- Explicit, non-silent behavior for every edge case listed in the request (§1, §3, §10).
- A `uze --help` / per-namespace `--help` design that names the split.
- A recorded ADR for the two decisions here that are hard to reverse once shipped: the grammar's
  scope-by-position rule, and the `remove` semantics reversal.

**Non-Goals (explicitly deferred, not decided here):**
- Renaming `marketplace.json` → `agents.json` (request §12) — analyzed, not proposed.
- A GitHub `owner/repo` shorthand for `market add` — flagged as a gap, not built.
- A CLI/application path to lock a bare local or direct-Git plugin source without a marketplace.
- `plugin enable`/`disable` (request §6) — explicitly rejected, not deferred.
- TUI redesign to show Project vs Machine state (request §14) — consequence documented, not implemented.
- Any change to `uze-core`, `uze-integrations`, the Store, ADR-009 receipt/drift lifecycle, or the
  `agents.lock` schema (ADR-017).

## Decisions

### D1 — Project shorthand grammar and edge cases (request §1)

`uze <plugin>@<market>` requires `@`; both segments are charset-validated exactly as
`parse_plugin_marketplace_spec` already validates them (reused, not reimplemented). Full edge-case table:

| Input / state | Behavior |
|---|---|
| No `agents.lock` yet | Creates one at the resolved project root (walk-up: lock > `AGENTS.md` > `.git` > cwd) |
| `agents.lock` exists | Adds/updates the entry; other entries untouched |
| Plugin already present, same marketplace | Idempotent no-op (same bytes rewritten, no duplicate install) |
| Plugin already present, different marketplace | `MarketplaceMismatch` error; lock untouched |
| Marketplace known but source conflicts with lock's record | `MarketplaceSourceConflict` error; lock untouched |
| Marketplace unknown | Error naming the marketplace, suggests `uze market add`; lock untouched |
| Run outside any recognized project | Falls back to cwd as root (existing ADR-016 rule) and **creates** a project there — this is the deliberate bootstrap path, not an error |
| Token collides with a built-in name | Impossible by construction (§3 above) — `@` presence alone decides |
| No `@` in input | Not shorthand; falls through to ordinary dispatch, which fails as an unrecognized command, with a hint naming the missing `@market` |
| Local path (`./x@ai`, `../x@ai`) | Rejected by the existing plugin-name charset validation; no local-plugin form is supported at project-shorthand level (see Context §6) |

Local plugin/path is deliberately **not** supported at this level: `agents.lock` cannot express a
non-reproducible local source (Context §6), and inventing a new lock representation for it is out of scope
for a CLI grammar change (would need its own ADR/spec touching `agents.lock`'s schema).

### D2 — `uze remove` becomes strictly project-scoped (request §3) — reverses ADR-016's design.md Decision #8

**This is the one place this proposal has a real, documented conflict with a currently-accepted design
decision**, per the instruction to surface conflicts before proposing past them.

`project-agent-environment/design.md` Decision #8 specifies: *"`remove` disambiguated by context (lock
present + plugin in lock → project; else → global)."* That was a deliberate choice at the time, to let
`uze remove` keep working as a drop-in replacement for the pre-project-environment global `remove` in the
common case (no project, or plugin not in this project's lock).

The new grammar's premise — root commands are unconditionally project-scoped, machine-only operations are
unconditionally namespaced — is incompatible with a root command whose target scope depends on invisible
state (does a lock exist? does it mention this plugin?). Concretely: `uze remove flow` run inside a project
whose lock doesn't happen to mention `flow` currently deletes `flow` from the *machine*, potentially
breaking every other project on that machine that depends on it, with no indication at the call site that
this is what's about to happen.

**Resolution:** `uze remove <plugin>` becomes strictly project-scoped — outside a project, or when the
plugin isn't in the lock, it now **fails** (see the spec's new scenarios) rather than falling back. The
error names `uze plugin remove <plugin>` as the machine-level equivalent. This is `BREAKING` per the
request's explicit acceptance of breaking changes pre-1.0. See ADR-019, which records this as a supersession
of ADR-016 §Decision-8's `remove` clause (ADR-016's other invariants — shorthand requires `@`, global admin
never touches the lock, `install` never re-resolves — are unaffected and reaffirmed).

### D3 — Parsing strategy: `clap` `external_subcommand`, not a pre-`clap` string check (request §10)

Replace the current `argv[1].contains('@')` check in `main()` (before `Cli::parse()` even runs) with a
formal `#[command(external_subcommand)]` variant on the `Command` enum:

```rust
#[derive(Debug, Subcommand)]
enum Command {
    // ...existing variants unchanged...
    #[command(external_subcommand)]
    External(Vec<String>),
}
```

`clap`'s own derive-generated matcher already tries every named variant first, falling through to
`External` only when nothing matches — this is `clap`'s documented behavior, not a hand-written priority
list, so the "no hacks scattered through main.rs" requirement is satisfied by construction: the precedence
rule lives in one attribute clap itself enforces.

When `Command::External(args)` is reached, `args[0]` is checked for `@` (reusing
`parse_plugin_marketplace_spec` for validation, not reimplementing it). If absent, this is an unrecognized
command — return `clap`'s own "unrecognized subcommand" error (augmented with the `@market` hint from D1).
If present, parse the *rest* of `args` — including `args[0]` itself as the positional — through a small
dedicated `#[derive(Parser)] struct ShorthandArgs { spec: String, #[arg(long)] trust: bool, #[arg(long,
value_enum, default_value_t = OutputFormat::Text)] format: OutputFormat }` via
`ShorthandArgs::try_parse_from`. This closes the exact correctness gap in Context §3 (silently-ignored
unknown flags) for free, because it's `clap` validating the flags, not a hand-rolled loop — and gets
`--help` for the shorthand form (`uze flow@ai --help`) for free too.

`--help`/`-V` at the top level continue to be handled before this dispatch exactly as they are today
(`main.rs`'s current check-args-len-2 branch for the custom-colored help stays; only the shorthand branch
moves into `clap`).

**Alternative considered:** keep the pre-`clap` string check but harden it (exhaustive tests, shared flag
parser). Rejected: it still means two independent argument-parsing code paths reachable from `main()`,
which is exactly what the request asked to avoid; `external_subcommand` gives one parser with one
documented precedence rule instead.

### D4 — Root-level command disposition (request §13 migration table)

| Current | Proposed | Scope | Disposition | New semantics if changed |
|---|---|---|---|---|
| `uze add <source>` | `uze plugin install <source>` | Machine | **MOVE** | None — same `add_plugin` call, moved under `plugin` |
| `uze list` | `uze plugin list` | Machine | **MOVE** | None |
| `uze inspect <plugin>` | `uze plugin inspect <plugin>` | Machine | **MOVE** | None |
| `uze remove <plugin>` | `uze remove <plugin>` (project) / `uze plugin remove <plugin>` (machine) | split | **RENAME semantics** | See D2 — root `remove` loses its global fallback |
| `uze update <plugin>` | `uze plugin update <plugin>` | Machine | **MOVE** | None |
| `uze install [path]` | `uze install [path]` | Project | **KEEP** | None — already correct |
| `uze status [path]` | `uze status [path]` | Project | **KEEP** | None |
| `uze context ...` | `uze context ...` | Project | **KEEP** | None |
| `uze marketplace add\|list\|remove` | `uze market add\|list\|remove` | Machine | **RENAME** (verb only) | None |
| — | `uze market inspect <name>` | Machine | **NEW** | New (D-context §2) |
| `uze plugin install\|list\|remove\|update` | `uze plugin install\|list\|remove\|update` | Machine | **KEEP** | None |
| `uze doctor` | `uze doctor` | Diagnostics | **KEEP** | None |
| `uze setup [harness]` | `uze setup [harness]` | Diagnostics | **KEEP** | None |
| — | `uze harness list\|inspect\|setup` | Machine | **NEW** | New namespace, no new behavior (setup delegates) |
| *(argv[1]-hack)* `uze <plugin>@<market>` | `uze <plugin>@<market>` (formal `external_subcommand`) | Project | **KEEP semantics, MOVE parser** | Stricter flag validation (D3); everything else unchanged |
| — | `uze plugin enable\|disable` | — | **REJECTED** | Not introduced (request §6) |

No aliases are proposed for the moved commands (`add`/`list`/`inspect`/`update` at root). Rationale: this
project is pre-1.0 alpha and has explicitly said it prefers a clean CLI over eternal aliases (request §13);
a short-lived alias window adds a second code path and a deprecation-warning surface for four commands
whose new spelling is one word longer, for a userbase small enough (alpha) that the `CHANGELOG`/release
notes are sufficient notice. If a transition window is wanted anyway, the minimal version is: keep the four
old root variants as `#[command(hide = true)]` clap aliases for one release, each printing `uze: 'uze add'
is now 'uze plugin install' (this alias will be removed in the next release)` to stderr before delegating —
never introduce parsing-level ambiguity between the alias and the shorthand (none of these four names
contain `@`, so D3's precedence rule is unaffected either way).

### D5 — `uze --help` design (request §9)

```
UZE — Agent Environment Manager

Usage:
  uze <plugin>@<market>
  uze <command> [options]

Project:
  <plugin>@<market>   Make this project use a plugin
  install             Install this project's environment from agents.lock
  remove <plugin>      Remove a plugin from this project
  status               Show this project's environment status
  context              Manage this project's AGENTS.md context

Machine:
  market                Manage marketplace sources
  plugin                Manage plugins installed on this machine
  harness                Manage agent harness integrations

Diagnostics:
  doctor                Check environment health
  setup [harness]       Provision a harness (or all detected harnesses)

Options:
  -h, --help     Print help
  -V, --version  Print version

Examples:
  uze flow@ai
  uze install
  uze market add hiukky/ai
  uze plugin install flow@ai
```

Differences from the request's sketch: `remove` gets its `<plugin>` argument shown (it's the one command
whose scope-by-position rule is easy to misread without it); `harness` is listed even though its verbs are
thin, for taxonomy completeness (request §7's "garantir que a taxonomia futura tenha espaço"). Each
namespace's own `--help` (`uze market --help`, `uze plugin --help`, `uze harness --help`) lists only that
namespace's verbs — `clap` already does this for free once the subcommands are properly nested; no custom
rendering needed beyond what the top-level `--help` already hand-renders today (the existing
`print_colored_help` function is extended, not replaced).

### D6 — `market add` source grammar: no GitHub shorthand in this change

`uze market add hiukky/ai` (request §4's example) is **not** supported by this proposal — Context §5 shows
`parse_marketplace_source` has no `owner/repo` form today, and adding one is a parsing-behavior change
independent of the verb rename. `market add` in this change accepts exactly what `marketplace add` accepts
today (local path with `marketplace.json`, or a full URL). A GitHub shorthand, if wanted, is a small,
separable follow-up (extend `parse_marketplace_source` with an `owner/repo` pattern resolved to
`https://github.com/<owner>/<repo>`) — flagged as future work, not blocking this grammar change.

### D7 — `marketplace.json` → `agents.json`: analysis only, no rename (request §12)

Not implemented here, per explicit instruction. Findings (Context §4):
- Impacted if ever done: `uze-core/src/acquisition/marketplace.rs` (parse/validate), `uze-application/src/bootstrap.rs`
  (embedded marketplace), `uze-application/src/application.rs` (`parse_marketplace_source`,
  `load_marketplace_manifest`), `uze-application/build.rs` (compiles the embedded manifest in), the
  project's own root `marketplace.json` + `plugins/uze` (the embedded `uze-official` marketplace).
- **Not** impacted, and must stay untouched: `.claude-plugin/marketplace.json` (Claude native catalogue),
  `.agents/plugins/marketplace.json` (Codex native catalogue) — vendor-dictated filenames, unrelated
  concept (Context §4).
- Recommendation: **separate change**. It's a file-format/identity decision (does `agents.json` also want a
  different schema, or just a rename?) orthogonal to whether the CLI verb is `market` or `marketplace` —
  this proposal's `market` rename does not depend on it and creates no pressure toward it. Bundling the two
  would make this change's diff harder to review for the boundary question it actually needs a decision on.
- If pursued later, that change should explicitly decide: rename only, or also fold in the `agents.lock`/
  `AGENTS.md`/`.agents/` naming family's rationale (a marketplace repo *is* conceptually "an agent registry",
  per the request's framing) — a naming-taxonomy decision, not a CLI grammar one.

### D8 — TUI consequence, not redesign (request §14)

Not implemented here. Consequence for whoever picks up a future TUI change: `src/ui/worker.rs` today only
calls machine-scoped application methods (Context §7). Once the CLI exposes `add_project_plugin`/
`remove_project_plugin`/`install_project_environment` as first-class commands, the TUI's plugin list/detail
views can and should distinguish "Installed" (machine, `installed_packages()`) from "Used" (project,
`project_environment(root).lock`) using the *same* application methods this proposal's CLI calls — no new
business logic in `src/ui/*`, only new read calls and, if action buttons are added, calls to the same
`add_project_plugin`/`remove_project_plugin` this CLI change wires up. This keeps CLI and TUI as two thin
adapters over one set of use cases, per the architectural constraints.

## LikeC4

No update needed. This change adds no container, component, or external dependency, and changes no
relationship between existing components — `market_inspect`/`harness_list`/`harness_inspect` are new
methods on the existing `UzeApplication` component's surface, not new components; `Command::External` is
an internal detail of the existing CLI adapter. If a future change gives the TUI project-aware use cases
(D8), *that* change should reassess whether the CLI/TUI-to-Application relationship needs a new labeled
edge in the model.

## Risks / Trade-offs

- **[`uze remove`'s behavior change is silent to existing muscle memory]** → Mitigation: the failure mode
  is a loud error naming the correct command (`uze plugin remove`), never a silent no-op or a
  differently-scoped success; release notes call it out explicitly as the one behavior (not just spelling)
  change in this set.
- **[Four users' shell history/scripts calling `uze add`/`list`/`inspect`/`update` break]** → Mitigation:
  alpha-acceptable per request §13; optional short-lived hidden aliases available (D4) if the team wants a
  softer landing.
- **[`external_subcommand` swallows genuinely unrecognized commands into a confusing error]** → Mitigation:
  D3's error path explicitly special-cases "no `@` found" to produce the same "unrecognized subcommand"
  message `clap` would give natively, with an added hint — not a generic shorthand-parse failure.
- **[`market`/`plugin`/`harness` three-namespace taxonomy invites a fourth, unplanned namespace later]** →
  Mitigation: `harness`'s scope is deliberately kept narrow (D-goals, request §7); no other namespace is
  proposed or implied by this design.
- **[Two application-layer additions (`market_inspect`, `harness_list/inspect`) still need real
  implementation]** → Mitigation: both are pure extractions of already-computed data (Context §2), lowest
  possible implementation risk; covered in tasks.md.

## Open Questions

None — every ambiguity the request raised is resolved above (D1–D8) or explicitly deferred as a named
non-goal with its own follow-up shape (D6, D7, D8). The one genuine blocking-style decision (D2, `remove`
semantics vs. ADR-016) is resolved in this document per the request's own instruction to decide it here
rather than leave it open, and is recorded as a formal supersession in ADR-019 rather than a silent
overwrite of ADR-016.
