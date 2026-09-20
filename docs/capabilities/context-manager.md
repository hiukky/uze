# Context Manager

The boundary that owns a *project's* instructions context, distinct from the
Package Manager that owns a *machine's* installed bytes. It exposes three
operations — `inspect`, `plan`, `reconcile` — and nothing else.

## Principles

Invariants, not implementation details, and they hold for anything built on top
of this boundary later:

- **Deterministic.** Every operation produces the same output for the same
  filesystem state, every time. Nothing here is probabilistic.
- **No semantic merging.** Two files — a hand-written `CLAUDE.md` and a
  hand-written `GEMINI.md` — are never compared, judged equivalent, or combined.
  The Context Manager observes and reports; it never decides that two pieces of
  text mean the same thing.
- **No LLM.** No function in `text_region.rs`, `context.rs`, or
  `UzeApplication::context_*` calls a model, and none ever will as part of this
  boundary. Determinism and LLM-independence are the same property stated twice.
- **`AGENTS.md` is the baseline.** The one file every recognized delivery path
  either reads natively or bridges into. It is an existing external convention,
  preserved as plain content, not a uze format.
- **Vendor files stay valid.** `CLAUDE.md`/`GEMINI.md` are never treated as
  deficient. A harness's own file legitimately holding harness-specific
  instructions, alongside or instead of a bridge, is expected and supported.
- **The bridge is a mechanism, not content.** The `@AGENTS.md` region exists only
  so one harness reaches the same canonical content the others read directly. It
  carries nothing of its own and is never the source of truth.

## Why a separate boundary

`uze plugin install`/`remove`/`update` stay entirely machine-scoped. A second,
independent concern reads the installed package set and reconciles it into *one
project's* shared context, taking `project_root` as ordinary function input —
never as a persisted `Project` entity.

```text
 Harness Manager     Plugin Manager     Context Manager
       |                   |                   |
 install/update       packages/store      instructions
 harnesses            skills/MCP/…        AGENTS.md reconciliation
                           |                   |
                     ~/.uze/store       <project>/AGENTS.md
```

The two share one primitive (`ManagedTextRegion`) and one convention
(`AttachmentState`), and nothing else: the Context Manager never writes to the
Store, and the Package Manager never reads or writes a project's files.

## The call graph

```text
uze-core:
  text_region.rs   attach / inspect / detach / reconcile / region_shape /
                   has_content_outside_managed_regions / region_identities_present
  context.rs       inspect_agents_md   (read-only)
                   plan_agents_md      (read-only, built on inspect)
                   reconcile_agents_md (writes; built on inspect too)
  engine.rs        package_resources_at — package-side AGENTS.md discovery

uze-application (the one layer where vendor names appear):
  instruction_contributions()      reads the Store, pure
  context_inspect(project_root)    read-only
  context_plan(project_root)       read-only
  context_reconcile(project_root)  writes

src/main.rs:
  uze agent context inspect | plan | reconcile [path] [--format json]
```

**The load-bearing rule, enforced by construction:** `reconcile_agents_md` calls
`inspect_agents_md` to compute its diff, and `plan_agents_md` is built on the
same function. Nothing read-only ever calls a function that writes, and a plan
and a later reconcile can disagree about whether something got fixed — never
about what "wrong" looked like.

## Inspect

`ProjectContextStatus` carries the observed sources (`AGENTS.md`, `CLAUDE.md`,
`GEMINI.md`), each package's contribution status, orphaned and malformed
regions, per-harness delivery, a portability verdict, and warnings.

`InstructionSourceObservation` splits each file three ways — **discovered**
(`exists`), **user-owned** (content outside every well-formed managed region)
and **managed** (the region identities present). That is the smallest split that
answers "whose content is this?" without inventing a larger taxonomy.

`HarnessContextDelivery` is three cases, not five: `Native` (reads `AGENTS.md`
directly), `Bridge { needed, state }` — `state` is checked even when `needed` is
false, so a stale-but-present bridge is visible rather than folded into "not
needed" — and `NotDetected`, which is never counted as a gap.

**Proven zero-write** by filesystem-snapshot equality across every state a
project can be in — absent, matched, drifted, orphaned, malformed
(`tests/context_inspection.rs`, and the same property at the `uze-core` level in
`inspect_agents_md_never_writes_in_any_state`).

## Portability

```rust
pub enum Portability {
    NoContext,
    Portable,
    PartiallyPortable { gaps: Vec<String> },
    VendorLocked { files: Vec<PathBuf> },
}
```

A pure function of `sources` + `harnesses`. `VendorLocked` fires when
`AGENTS.md` is absent but another recognized file has its own content;
`PartiallyPortable` when `AGENTS.md` exists but a *detected* bridge-needing
harness's bridge isn't `Matched`. Two cases get a warning instead of a verdict:
two divergent vendor files with no `AGENTS.md`, and a bridge file legitimately
carrying vendor-specific content alongside its bridge region.

## Plan

```rust
pub enum PlannedAction { Attach, NoChange, Blocked(String), Remove }
```

Four cases, not six. `Update`/`Create` collapse into `Attach` — a region is only
ever freshly created or refused, never partially rewritten in place — and
`Blocked` covers both drifted content and malformed markers, because both mean
the same thing to a plan: reconcile will refuse to touch it. Each action is an
`inspect` observation mapped through this vocabulary, never a second independent
decision. Also proven zero-write.

## Reconcile

| Invariant | Where proven |
|---|---|
| User-owned content never overwritten | `tests/context_inspection.rs` scenarios A–F |
| `DRIFTED` never silently corrected | `a_still_installed_packages_drifted_region_is_reported_and_never_rewritten` |
| Other owners' regions stay intact | `multiple_regions_from_different_identities_coexist_and_detach_independently` |
| Reconciling twice is idempotent | `reconciling_repeatedly_never_duplicates_regions_or_bridges` |
| The bridge is derived state | the bridge loop recomputes `needed` every call, never reads a stored receipt |
| No bridge without a matched contribution | same |
| Native harnesses receive no extra artifact | `NATIVE_INSTRUCTION_INTEGRATIONS` never appears in `BRIDGE_INTEGRATIONS` |
| Claude receives only the minimal bridge | `INSTRUCTION_BRIDGE_CONTENT = "@AGENTS.md"`, one line |
| The Store stays machine-scoped | `context_operations_never_alter_the_installed_package_set` |
| No integration gains project lifecycle semantics | `IntegrationPort` is unmodified |

## What it does to a project that already had files

Six scenarios pass end to end (`tests/context_inspection.rs::scenario_[a-f]_*`):
only a hand-written `CLAUDE.md`; only `GEMINI.md`; both, divergent; a
hand-written `AGENTS.md` with nothing installed; a hand-written `AGENTS.md`
beside a uze region; a hand-written `CLAUDE.md` beside a uze bridge. In every
case manual content survives byte-for-byte and no migration occurs.

**`CLAUDE.md → AGENTS.md` is explicitly not attempted.** Deciding that two
hand-written files mean the same thing is a semantic judgment, which belongs to
an agentic layer and never to deterministic reconciliation.

## The agentic layer sits on top, not inside

`uze:init` (see [uze-skill.md](uze-skill.md)) calls `context_inspect` to *see* a
project's real state, decides with a model and the user's approval what a
consolidated `AGENTS.md` should contain, writes that file the way a human
editing it would, and calls `context_reconcile`/`context_inspect` again to check
that the deterministic layer agrees. Nothing in that flow requires `uze-core` or
`uze-application` to grow LLM awareness: the agentic layer is purely a client of
these three functions.

## Why this shape

Each of these was decided against a concrete alternative, and each is still in
force:

- **A region, never a file of uze's own.** A managed symlink
  (`CLAUDE.md -> AGENTS.md`) only works where `CLAUDE.md` is absent or holds
  nothing else, and the design cannot assume either. A one-line `@AGENTS.md`
  import works in both cases, and is the interop path Claude's own
  documentation prescribes.
- **No harness's config array is edited.** OpenCode's `instructions` field and
  the Gemini-family `context.fileName` are real second mechanisms, and both
  were rejected: each needs a `ManagedArtifact` variant for a JSON-array edit
  with undocumented order and conflict semantics, and each risks overwriting a
  value the user set — to serve a harness that already has a zero-artifact
  native path.
- **`AGENTS.override.md` is never written.** It *replaces* rather than merges,
  so writing one where a user override may later be expected is the silent
  destruction ADR-009 exists to prevent.
- **Order comes from the region identity, not from install order.** Identities
  are held in a sorted set, so the same packages produce the same file on every
  machine. Precedence is never a property of a package's content: no harness
  lets a file declare its own rank — every vendor rule is about where the file
  sits.
- **Project scope, not global.** A global instructions file is one file per
  user rather than a directory of discrete entries, so several packages meet the
  same region-merge problem project scope already solves; and a global write
  touches machine-wide configuration on every install. `AGENTS.md`'s own
  convention is project-level and version-controlled.
- **A package ships a plain `AGENTS.md` at its root** — not `instructions/`,
  not a uze format. It is already the exact file two harnesses read natively,
  it is usable without uze, and it decomposes into a region unchanged.

## Vendor-neutrality

`text_region.rs` and `context.rs` contain zero occurrences of any harness name,
in code, doc comments and tests alike. In `uze-application`, three lines name
one: `BRIDGE_INTEGRATIONS`, `NATIVE_INSTRUCTION_INTEGRATIONS`, and the list of
recognized filenames `context_inspect` observes. One file, one layer.
