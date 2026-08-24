# Projection Conflicts Are Detected at Naming Time, Not Attach Time

Status: Accepted

Refines: [ADR-026 (Stable Namespaced Invocation Labels)](026-stable-namespaced-invocation-labels.md).
Narrowed by [ADR-030 (Skill + Invocation Policy replace the canonical Command)](030-skill-plus-invocation-policy.md):
with one canonical Skill kind, the same-name Skill+Command collision is
gone; the conflict machinery below remains for the residual cases
(legacy receipts, or a reused shared-root artifact that cannot carry the
reusing integration's invocation encoding).

## Context

ADR-026 defines the stable presentation label `<plugin>:<capability>` and
declares that decomposed user-scope delivery is "deterministically blocked,
never silently renamed" when a shared root cannot hold two claims. The
implementation did not keep that promise: `UzeApplication`'s naming
resolver (`resolve_exposure_name`, `attachment/lifecycle/attach.rs`)
computed a `claimed` set of physical entry names, then resolved every fully
claimed candidate to `candidates.last()` — handing the *conflicting* name
back instead of blocking. The failure surfaced later, inside
`ExposureMechanism::attach`, as `ManagedEntryDrift`:

```
a managed entry has drifted and was preserved at ~/.agents/skills/git:commit
```

Nothing had drifted. The real situation was a projection ownership
conflict: one physical entry, two incompatible representations, two
claimers — e.g. a legacy Command-era receipt and a Skill, or two
distinct resources converging on one label. The second claimer saw an
existing symlink pointing elsewhere and reported "drift", after the first
attachment had already happened, with a message that named neither the
conflict nor the two claimers.

## Decision

**A projection ownership conflict is a distinct, deterministic error
detected during naming resolution — before any attachment is attempted —
when a candidate entry is already claimed by a *different* canonical
resource with an incompatible representation.**

- New error category: `UzeError::ProjectionConflict`, carrying the physical
  entry, both canonical resource identities, both integrations, and both
  physical targets. CLI/TUI wording: "projection conflict at `<entry>`:
  `<requested>` (<integration>) cannot be exposed from `<target>` because
  `<existing>` (<integration>) already owns this entry (target `<target>`);
  remove or rename one of the capabilities."
- Detection point: `resolve_exposure_name` — the earliest layer that can
  see both the candidate and the claiming receipt. The reuse path that
  handles the *same* resource sharing one entry across shared-root harnesses
  (Codex/OpenCode both pointing at one Skill entry) runs first and is
  unchanged: same canonical identity → share; different canonical identity →
  conflict.
- No arbitrary suffix, no rename, no new ledger, no new ownership model:
  the receipt ledger already records who owns each entry; the resolver was
  simply not consulting it for compatibility.
- `ManagedEntryDrift` stays reserved for actual drift (an owned entry whose
  physical state changed after attachment). It is no longer the catch-all
  for predictable planning collisions.

## Consequences

- A cross-harness shared-root conflict is reported honestly at install
  time, with both claimers named.
- The user-facing workaround is explicit (remove or rename one of the
  capabilities) rather than an opaque "preserved at" failure.
- No integration code changed for the detection itself: the fix is in the
  application's naming resolver, which already had every fact needed.
  `IntegrationPort` is untouched.
- ADR-030 adds one more deterministic conflict source: when a harness
  reuses another harness's physical entry, the artifact must still carry
  the reusing harness's own invocation encoding — checked at attach
  (`codex`/`opencode` `materialize_or_verify_skill`), because the reused
  artifact's content is only readable there.

## Conformance

`tests/skill_invocation_conformance.rs`:

- `model_only_skill_shared_root_detects_cross_integration_policy_loss`:
  OpenCode reusing Codex's shared-root artifact without its `slash: false`
  encoding → precise `ProjectionConflict`;
- `codex_and_opencode_reuse_one_physical_entry_for_a_default_skill`: the
  same canonical Skill still shares exactly one physical entry;
- `user_only_skill_installs_cleanly_with_codex_and_opencode`: each harness
  gets its own native encoding without a false conflict.
