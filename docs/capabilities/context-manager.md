# Context Manager

Status: **first usable vertical slice, implemented and tested, 2026-08-21.**
Companion to [instructions.md](instructions.md) and
[instructions-design.md](instructions-design.md), which cover the earlier
Instructions tracer bullet this builds directly on. This document describes
the Context Manager as its own product boundary, distinct from the Package
Manager, and the `inspect → plan → reconcile` model that boundary now
exposes.

## Principles

These hold for this vertical slice and for anything built on top of it
later (including the future `/uze` agentic layer sketched below). They are
invariants, not implementation details, and are restated here explicitly so
they survive independently of any one code comment:

- **Context Manager is deterministic.** Every operation — `inspect`,
  `plan`, `reconcile` — produces the same output for the same filesystem
  state, every time. Nothing here is probabilistic.
- **It does not semantically merge instructions.** Two files (e.g. a
  hand-written `CLAUDE.md` and a hand-written `GEMINI.md`) are never
  compared, judged equivalent, or combined. The Context Manager observes
  and reports; it never decides that two pieces of text "mean the same
  thing."
- **It does not depend on an LLM.** No function in `text_region.rs`,
  `context.rs`, or `UzeApplication::context_*` calls a model, and none ever
  will as part of this boundary. Determinism and LLM-independence are the
  same property stated twice, on purpose.
- **`AGENTS.md` is the portable project context baseline.** It is the one
  file every recognized delivery path either reads natively or bridges
  into. It is not a UZE-invented format — it is the existing, external
  `agents.md` convention, preserved as plain content.
- **Vendor files remain valid sources of vendor-specific context.**
  `CLAUDE.md`/`GEMINI.md` are never treated as deficient or as something to
  be replaced. A harness's own file legitimately holding harness-specific
  instructions, alongside or instead of a bridge, is expected and
  supported — see `derive_warnings`' "expected and supported, not a gap"
  case.
- **Claude's bridge is an implementation mechanism, not canonical
  context.** The bridge region (`@AGENTS.md`) exists only so that one
  harness reaches the same canonical content Codex/OpenCode/Antigravity
  already read directly. The bridge itself carries no content of its own
  and is never the source of truth — `AGENTS.md` is.

## Why a separate boundary

Early in the Instructions work, `project_root` threading briefly looked like
it might require `uze add` to become project-scoped, or a persistent
`Project` entity to track which projects reference which packages. Neither
happened. The resolution: **`uze add`/`uze remove`/`uze update` stay 100%
global**, exactly as before Instructions existed, and a second, independent
concern reads the globally-installed package set and reconciles it into
*one project's* shared context — taking `project_root` as ordinary function
input, never as a persisted concept.

```text
                         UZE
                          |
       +------------------+------------------+
       |                  |                  |
 Harness Manager     Plugin Manager     Context Manager
       |                  |                  |
 install/update      packages/store      instructions
 harnesses           skills/MCP/...      AGENTS.md reconciliation
                          |                  |
                    ~/.uze/store       <project>/AGENTS.md
```

Package Manager and Context Manager share one primitive
(`ManagedTextRegion`) and one Core convention (`AttachmentState`), but are
otherwise independent: the Context Manager never writes to the Store, and
the Package Manager never reads or writes a project's files.

## Dependency graph

```text
uze-core:
  text_region.rs   — attach / inspect / detach / reconcile /
                      remove_unconditionally / region_shape /
                      has_content_outside_managed_regions /
                      region_identities_present
        |
  context.rs       — InstructionContribution
                      inspect_agents_md   (read-only)
                      plan_agents_md      (read-only, built on inspect)
                      reconcile_agents_md (writes; built on inspect too)
        |
  engine.rs::package_resources_at — package-side AGENTS.md discovery

uze-application (composition root — the one place vendor names appear):
  instruction_contributions()  — reads Store, pure
  context_inspect(project_root)   — read-only; builds ProjectContextStatus
  context_plan(project_root)      — read-only; builds ContextPlan
  context_reconcile(project_root) — writes; builds ContextReconciliationReport

src/main.rs:
  uze context inspect|plan|reconcile [path] [--format json]
```

**The load-bearing rule, enforced by construction, not just by convention:**
`reconcile_agents_md` calls `inspect_agents_md` to compute its diff (attach
first, then re-observe); nothing read-only ever calls a function that
writes. `context_plan` is built on `plan_agents_md`, which is itself built
on `inspect_agents_md` — a plan and a subsequent reconcile can disagree
about whether something got fixed, never about what "wrong" looked like,
because both trace back to the same one observation function.

## Context Inspection

```rust
pub struct ProjectContextStatus {
    pub root: PathBuf,
    pub canonical: PathBuf,
    pub sources: Vec<InstructionSourceObservation>,   // AGENTS.md, CLAUDE.md, GEMINI.md
    pub contributions: Vec<PackageInstructionStatus>, // installed packages vs. AGENTS.md
    pub orphaned_regions: Vec<String>,
    pub malformed_regions: Vec<String>,
    pub harnesses: Vec<HarnessContextStatus>,
    pub portability: Portability,
    pub warnings: Vec<String>,
}
```

`InstructionSourceObservation` distinguishes **discovered** (`exists`),
**user-owned** (`has_user_content`, computed by
`text_region::has_content_outside_managed_regions` — content outside every
well-formed managed region) and **managed** (`managed_region_identities`)
content for each of the three recognized files — the smallest split that
answers Fase 3's question without inventing a larger taxonomy.

`HarnessContextDelivery` is three cases, not five: `Native` (Codex,
OpenCode, Antigravity — reads `AGENTS.md` directly), `Bridge { needed,
state }` (Claude Code — `state` is always checked, even when `needed` is
false, so a stale-but-present bridge is visible rather than silently folded
into "not needed"), and `NotDetected` (the harness isn't on this machine at
all — never counted as a gap).

**Proven zero-write** by filesystem-snapshot equality across every state a
project can be in: absent, populated-and-matched, drifted, orphaned, and
malformed (`tests/context_inspection.rs`,
`context_inspect_never_writes_anything_in_a_populated_project`; the same
property at the pure-`uze-core` level in
`inspect_agents_md_never_writes_in_any_state`).

## Portability assessment

```rust
pub enum Portability {
    NoContext,
    Portable,
    PartiallyPortable { gaps: Vec<String> },
    VendorLocked { files: Vec<PathBuf> },
}
```

Derivation is a pure function of `sources` + `harnesses` — no semantic
comparison of file contents, no LLM, no decision that two files are
"equivalent." `VendorLocked` fires when `AGENTS.md` is absent but another
recognized file has its own content; `PartiallyPortable` fires when
`AGENTS.md` exists but a *detected* bridge-needing harness's bridge isn't
`Matched`. Two observed-but-not-acted-on cases get a `warnings` entry
instead of a portability judgment: two vendor-specific files with different
content and no `AGENTS.md` ("independent, potentially divergent... UZE does
not compare or consolidate them"), and a bridge file that legitimately
carries extra vendor-specific content alongside its bridge region (expected
and supported, not a gap).

## Context Plan

```rust
pub enum PlannedAction { Attach, NoChange, Blocked(String), Remove }
```

Four cases, not six — `Update`/`Create` collapsed into `Attach` (a region
is only ever freshly created or refused; `text_region` never partially
rewrites one in place), and `Blocked` covers both "content drifted" and
"markers malformed" since both mean the same thing to a plan: reconcile
will refuse to touch it. `AgentsMdPlan`/`ContextPlan` are computed by
mapping each `inspect_agents_md`/bridge-`AttachmentState` observation
through this vocabulary — never a second, independent decision about what
reconcile would do.

**Proven zero-write** the same way inspection is
(`context_plan_never_writes_anything`, exercised specifically against the
state with the most `Attach` actions pending — the state most tempting to
accidentally execute).

## Reconcile

Unchanged in behavior from the earlier Instructions milestone; now
demonstrably built on the same observation primitive as inspect/plan
rather than a parallel implementation. All ten invariants requested for
this phase hold, each backed by an existing or new test:

| Invariant | Where proven |
|---|---|
| User-owned content never overwritten | `tests/context_inspection.rs` scenarios A–F |
| `DRIFTED` never silently corrected | `a_still_installed_packages_drifted_region_is_reported_and_never_rewritten` |
| Other owners' regions stay intact | `multiple_regions_from_different_identities_coexist_and_detach_independently`, `two_packages_share_one_agents_md_and_exactly_one_bridge_per_harness` |
| Reconciling twice is idempotent | `reconciling_repeatedly_never_duplicates_regions_or_bridges` |
| Bridge is derived state | `context_reconcile`'s bridge loop recomputes `needed` from `agents_md_report.has_any_matched_contribution()` every call — never reads a stored receipt |
| No bridge without a matched contribution | same |
| Codex/OpenCode/Antigravity receive no extra artifact | `NATIVE_INSTRUCTION_INTEGRATIONS` never appears in `BRIDGE_INTEGRATIONS`; confirmed empirically via `codex debug prompt-input` |
| Claude receives only the minimal bridge | `INSTRUCTION_BRIDGE_CONTENT = "@AGENTS.md"`, one line |
| Package Store stays global | `context_operations_never_alter_the_installed_package_set` |
| No Integration gains Project lifecycle semantics | `IntegrationPort` trait is unmodified; `grep` proof below |

## Behavior on existing, hand-written projects (Fase 6)

All six scenarios pass as end-to-end tests
(`tests/context_inspection.rs::scenario_[a-f]_*`): a project with only a
hand-written `CLAUDE.md`, only `GEMINI.md`, both with divergent content, a
hand-written `AGENTS.md` with no packages installed, a hand-written
`AGENTS.md` alongside a UZE region, and a hand-written `CLAUDE.md` alongside
a UZE bridge. In every case, manual content survives byte-for-byte and no
automatic migration occurs. **`CLAUDE.md → AGENTS.md` is explicitly not
attempted** — deciding that two hand-written files "mean the same thing" is
a semantic judgment this milestone deliberately leaves to the future
agentic layer (Fase 8), never to deterministic reconciliation.

## CLI (Fase 7)

```bash
uze context inspect [path]     # read-only
uze context plan [path]        # read-only
uze context reconcile [path]   # writes
```

Chosen over `uze context --check`/`--apply` because it names the three
distinct backend operations directly, rather than overloading one verb with
flags — and because `inspect`/`plan` being separate subcommands makes their
read-only nature the obvious default, not something a flag has to opt into.
`path` defaults to the current directory; `--format json` is supported on
all three. Verified against a real, built `uze` binary in an isolated
`UZE_HOME`, install → inspect (shows `NO_CONTEXT`) → plan (shows two
`ATTACH`) → reconcile → inspect again (shows `PORTABLE`) → JSON output
checked.

## Future `/uze` (documented, not implemented)

The boundary this phase establishes is exactly what a future agentic `/uze`
skill would sit on top of, without ever needing to touch `uze-core` itself:

```text
    Agentic layer                 Context Manager
    --------------                ---------------
    reasoning                          
    semantic analysis of                  
      CLAUDE.md/GEMINI.md/AGENTS.md          
    user conversation                          
    suggest consolidation      -->    context_inspect()   (read the truth)
    generate/edit AGENTS.md   -->    (writes the file directly, same as
                                       a user would — not through a new
                                       Core API)
    call reconcile             -->    context_reconcile() (deterministic)
    verify portability again   -->    context_inspect()   (read the truth)
```

The separation that matters: **the Context Manager never depends on an
LLM.** Every function in `context.rs`/`text_region.rs` and every
`UzeApplication::context_*` method is deterministic, testable without a
model, and already is tested that way. A future skill would call
`context_inspect` to *see* a project's real state (never invent one),
decide — with a model, with the user's approval — what a consolidated
`AGENTS.md` should contain, write that file the same way a human editing it
by hand would, and call `context_reconcile`/`context_inspect` again to
verify the deterministic layer agrees. Nothing about that flow requires
`uze-core` or `uze-application` to grow LLM awareness; the agentic layer is
purely a *client* of the same three functions this milestone ships.

## Vendor-neutrality proof

```
$ grep -inE "claude|codex|opencode|gemini" crates/uze-core/src/text_region.rs crates/uze-core/src/context.rs
(zero matches, anywhere — code, doc comments, and tests alike)

$ grep -n "claude-code\|CLAUDE.md\|GEMINI.md\|\"antigravity\"\|\"codex\"\|\"opencode\"" crates/uze-application/src/application.rs
41:    &[("claude-code", "CLAUDE.md")];    # BRIDGE_INTEGRATIONS
57:const NATIVE_INSTRUCTION_INTEGRATIONS: &[&str] = &["codex", "opencode", "antigravity"];
631:        let sources: Vec<InstructionSourceObservation> = ["AGENTS.md", "CLAUDE.md", "GEMINI.md"]
```

Three lines, one file, one layer — the third is `context_inspect` naming
the same three recognized filenames to observe, no new concept beyond the
two constants above. `IntegrationPort` gained zero new methods across this
entire phase.
