# Invariants

Properties that hold today, each guarded by a test rather than by intent. They
are recorded here because they are what the architecture *is*: a future change
that breaks one is not a refactor, it is a different product.

Every entry names the test that fails if the property stops holding. A claim
with no such test does not belong on this page.

---

## Vendor neutrality (M1)

### Store owns packages, not harness artifacts

The Store writes package bytes and its own registry. It never writes anything
a harness reads.

> `tests/vendor_neutral_core.rs::the_store_writes_no_harness_owned_artifact_of_its_own_accord`

### Integrations own vendor semantics

Which packages belong in a harness-owned view, what shape that view takes, and
when it is rebuilt are decisions of the integration that owns the harness.

> `tests/vendor_neutral_core.rs::a_package_without_the_native_envelope_is_not_published`

### Adding a harness requires no semantic change to Store, Engine, Router or the package model

Proven by adding a fifth harness with a materially different native delivery:
Codex needs a published catalogue, Antigravity CLI needs neither but
*copies* (its `agy plugin install` stages bytes — no link verb exists), and
both go through the same `IntegrationPort`.

> `tests/vendor_neutral_core.rs::the_store_contains_no_source_mechanism_semantics`
> `tests/vendor_neutral_core.rs::no_core_module_depends_on_acquisition`

### Package publication and package-native delivery are independent

`republish_packages` maintains a derived view; `attach_package` performs a
native delivery. Codex uses both. Antigravity uses only the second — its
`republish_packages` is never overridden.

> `tests/vendor_neutral_core.rs::a_derived_view_is_rebuilt_from_the_package_set_alone`
> `tests/vendor_neutral_core.rs::republish_is_a_noop_for_an_integration_that_publishes_nothing`

### A failed derived view never invalidates an installation

One harness failing to publish leaves the package installed, the other
harnesses untouched, and the failure observable through `doctor`.

> `tests/vendor_neutral_core.rs::a_failed_publication_leaves_the_package_installed_and_says_so`

### `uze-core` production logic never names a specific harness

No line of `uze-core`'s production code (outside its own test fixtures)
names Claude, Codex, OpenCode, or Antigravity — as an identifier or
a string literal. Strengthened by ADR-005: a foreign-format importer that
once named Claude here (`ClaudePluginImporter`, acquisition-time, never
delivery-time) was confirmed dead — unreachable from `Store::ingest` or
any other production path — and removed; the invariant now holds with no
carved-out exception.

> `tests/integrations/identity.rs::core_never_names_a_vendor_harness`

### The Application and CLI/TUI never name a harness either

`uze-application` orchestrates integrations it knows only through
`IntegrationPort`; the CLI/TUI consume registry descriptors and read
models. All concrete harness knowledge — construction, display metadata,
context-delivery mode, shim names — lives in `uze-integrations`, with one
composition root (`IntegrationRegistry::builtin`/`isolated`) naming the
built-in set. A comment explaining a generic mechanism may cite a vendor;
live code may not.

> `tests/integrations/identity.rs::application_never_names_a_vendor_harness`
> `tests/integrations/identity.rs::cli_and_tui_never_name_a_vendor_harness`

### One composition root owns the built-in integration set

`crates/uze-integrations/src/registry.rs` is the single production place
that constructs the concrete integration types (env-based `builtin` and
isolated `isolated`); application, the runtime shim, and the README
matrix all consume the registry. A new harness needs one vertical, one
registry entry, conformance, and docs — nothing in core/application/
CLI/TUI.

> `crates/uze-integrations/src/registry.rs` tests
> `tests/integrations/vendor_neutral.rs::harness_selection_comes_from_the_registered_integrations`

### Project-context delivery is declared per integration

Which harness reads the shared `AGENTS.md` natively, which needs an
`@AGENTS.md` bridge region, and which additional native files are
observed for portability reporting is each integration's
`context_delivery()` declaration — never an Application-owned vendor
list. The bridge protocol itself (region identity, import line) is the
Application's, shared by every bridge-needing harness.

> `tests/memory/inspection.rs` scenarios A–F (stub harness declares its
> bridge exactly like a real integration)

### Native means preserved semantics, not identical primitives (ADR-030)

A route is **Native** when a harness offers a first-class, officially
supported mechanism that preserves the canonical semantics of the
capability. It does **not** require the same vendor name, file format, or
physical primitive across vendors: UZE models user-visible semantics, and
the same canonical capability may legitimately be Native on every harness
through differently-named primitives. The canonical capability is the
Skill, and its semantics are *who may invoke it* (invocation policy,
ADR-030). A user-only Skill is Native on Claude Code via
`disable-model-invocation: true` and on Codex via
`agents/openai.yaml` → `policy.allow_implicit_invocation: false`, even
though both deliver a Skill-shaped artifact; a vendor `Command` is only a
projection detail it may be generated from. The same definition is what
makes Antigravity's non-default policies **Adapted** rather than Native:
its only primitive is Skills that are both model-discoverable and
slash-invocable, so neither half of a non-default policy is preserved —
the loss is declared in the evidence, never silently covered. Package
exact coverage is semantic-aware: an envelope only claims a Skill when the
policy is actually preserved (`provided = discovered ∩ safely
representable`).

> `tests/skill_invocation_conformance.rs::codex_routes_every_combination_honestly`
> `tests/skill_invocation_conformance.rs::antigravity_routes_every_combination_honestly`
> `tests/skill_invocation_conformance.rs::opencode_routes_every_combination_natively`
> `tests/skill_invocation_conformance.rs::claude_routes_every_combination_at_capability_level`
> `tests/skill_invocation_conformance.rs::codex_generated_package_never_claims_a_model_only_skill`
> `tests/skill_invocation_conformance.rs::antigravity_generated_package_never_claims_a_user_only_skill`
> `tests/skill_invocation_conformance.rs::claude_generated_package_covers_a_user_only_skill_and_materializes_the_marker`
> `crates/uze-integrations/src/claude/plugin.rs::claude_native_coverage_tests::explicit_user_only_skill_with_the_vendor_marker_is_covered`
> `crates/uze-integrations/src/claude/plugin.rs::claude_native_coverage_tests::explicit_user_only_skill_without_the_vendor_marker_is_not_covered`
> `crates/uze-integrations/src/claude/generate.rs::generated_native_tests::user_only_skill_is_materialized_with_the_claude_marker`

### A Skill nobody may invoke is never projected (ADR-030)

`invoke: {model: false, user: false}` is invalid — it is parsed and kept
explicit (never silently defaulted to a model-visible or user-visible
combination), every integration routes it Unsupported, and no receipt,
symlink or generated file is ever created for it.

> `tests/skill_invocation_conformance.rs::invalid_policy_never_creates_a_receipt_anywhere`
> `crates/uze-core/src/skill.rs::invalid_combination_is_kept_explicit_never_defaulted`

### Existing Skills without `invoke:` behave exactly as before (ADR-030)

The canonical default is model+user; a SKILL.md with no `invoke:` block is
discovered with no parsed policy (defaults apply), delivered byte-preserving
where it always was, and never gains a policy sidecar it did not declare.

> `tests/skill_invocation_conformance.rs::default_skill_package_installs_cleanly_on_every_harness_as_before`
> `tests/skill_invocation_conformance.rs::absent_invoke_block_defaults_to_model_and_user_and_behaves_as_before`
> `crates/uze-core/src/project.rs::skill_without_invocation_block_defaults_and_is_not_reattached`

### A shared-root entry always carries the superset of both encodings

Codex and OpenCode share `~/.agents/skills`. The one physical entry a
non-default Skill gets is the **superset** representation: SKILL.md carries
OpenCode's own invocation controls (`opencode/autoinvoke`, `slash`) AND the
entry carries Codex's `agents/openai.yaml` policy sidecar whenever
`invoke.model` is false — so whichever integration created the entry, the
other's reuse verification passes and the canonical policy can never
silently degrade into model visibility. A genuinely foreign artifact
(a wrapper that predates the superset or carries no encoding) still fails
deterministically (`ProjectionConflict`) instead of degrading a user-only
or model-only policy.

> `tests/projection/shared_roots.rs::user_only_skill_codex_and_opencode_preserves_codex_policy`
> `tests/projection/shared_roots.rs::model_only_skill_shared_root_reuse_carries_both_encodings`
> `tests/projection/shared_roots.rs::foreign_shared_entry_without_opencode_encoding_still_conflicts`
> `tests/integrations/harness/codex.rs::real_codex_dogfood_user_only_skill_stays_hidden_in_opencode_owned_shared_entry`

### Invocation labels are stable and presentation-only (ADR-026)

Plugin capabilities exposed through UZE carry a stable, plugin-qualified
invocation label (`<plugin>:<capability>`) as their single naming
candidate: deterministic, predictable, independent of installation order
and of which other plugins are installed, with no bare aliases. The label is
a presentation concern — canonical Resource identity, Store bytes, package
layout, coverage identities (`provided_resource_identities`) and capability
bodies are untouched — and the vendor integration owns the physical
encoding (Claude's native plugin namespace, Codex/OpenCode/Antigravity
verbatim `flow:review`).

> `tests/invocation_labels.rs::installing_another_plugin_never_renames_an_existing_one`
> `tests/invocation_labels.rs::labels_never_touch_canonical_identity_store_or_receipts`
> `tests/invocation_labels.rs::claude_declares_plain_and_namespaces_natively_without_double_prefix`
> `tests/invocation_labels.rs::physical_representations_preserve_the_semantic_label`
> `tests/exposure_naming.rs::two_packages_with_the_same_skill_name_coexist_deterministically`

---

## Acquisition and provenance (M2)

### Acquisition owns source semantics

Every source mechanism lives in `src/acquisition/`. Nothing else resolves,
fetches or interprets an origin.

### Store owns installed package bytes

The Store receives a materialized directory. It does not know how the bytes
got there and does not need the origin afterwards — an installed package keeps
working, and stays safely removable, after its source directory is deleted.

### Store persists provenance but does not interpret source mechanisms

Provenance reaches the Store as an opaque value it stores and compares through
`Provenance::same_origin`. It never matches a variant or reads a field.

> `tests/vendor_neutral_core.rs::the_store_contains_no_source_mechanism_semantics`

### Remote mutable references resolve to immutable Git commits

A branch, a tag and an unspecified reference all persist as a commit SHA. The
default branch is read from the remote, never guessed from a hardcoded name.

> `tests/git_acquisition.rs::a_branch_resolves_to_an_immutable_commit`
> `tests/git_acquisition.rs::a_tag_resolves_to_an_immutable_commit`
> `tests/git_acquisition.rs::an_unspecified_reference_resolves_the_repositorys_own_default_branch`

### Reinstall uses resolved provenance; update re-resolves requested provenance

Reinstalling stays at the recorded commit even after the branch has moved.
Updating asks the original request again and may land on a new commit.

> `tests/git_acquisition.rs::reinstalling_a_resolved_commit_stays_at_that_commit`
> `tests/git_acquisition.rs::updating_re_resolves_the_request_and_moves_with_the_branch`

A local path has no immutable revision, and the model does not pretend
otherwise: reinstall and update both re-read the directory as it is now.

### Installed packages are self-contained

No symlink the Store persists may resolve outside the package root. Every
source is held to the same rule, and validation runs before any byte is
written, so a rejected package leaves nothing behind.

> `tests/package_containment.rs` — absolute escape, `..` escape, chained
> escape, escape through a symlinked directory, and a local package held to
> the identical rule.

### Package discovery never follows directory symlinks

Discovery uses `symlink_metadata` and never descends into a symlink, which
makes the traversal acyclic by construction. Containment forbids leaving the
root but does not forbid a cycle inside it.

> `tests/package_containment.rs::a_mutual_symlink_cycle_does_not_hang_discovery`
> and the self-link and ancestor-link cases beside it.

The documented cost: content reachable only through a symlinked directory is
not discovered. The symlink is still preserved as package content.

> `tests/package_containment.rs::content_reachable_only_through_a_symlinked_directory_is_not_discovered`

### Remote executable capabilities cross an explicit consent boundary

A remote package declaring an MCP `command` requires explicit consent before
anything is written or attached. A declarative package requires none.

> `tests/git_acquisition.rs::a_remote_package_with_an_mcp_command_requires_trust`
> `tests/git_acquisition.rs::a_remote_package_with_only_a_skill_requires_no_trust`
> `tests/git_acquisition.rs::denied_trust_leaves_the_store_completely_untouched`
> `tests/git_acquisition.rs::a_non_interactive_process_reports_trust_required_rather_than_assuming_consent`

**This is a consent boundary, not a security sandbox, and not a provenance
guarantee.** It is scoped to remote acquisition: `uze add ./local` is treated
as an operator-controlled source and asks nothing, even with an MCP server.
Cloning a repository by hand and installing the result as a local path
deliberately changes the classification of that origin. UZE does not
fingerprint downloads, mark them, track origins out of band, or persist trust
decisions — and should not be described as if it did.

> `tests/git_acquisition.rs::a_local_package_with_an_mcp_command_still_requires_no_trust`

Consent is not inherited across an update. A revision introducing execution
the installed one did not have asks again.

> `tests/git_acquisition.rs::an_update_introducing_executable_capability_asks_again`

### Acquisition never executes package code

Cloning does not run hooks, does not recurse submodules, and reads
configuration from nothing but explicit flags. Capability inspection parses
declarations; it invokes nothing.

> `tests/git_acquisition.rs::submodules_are_not_recursed_into`

### Cache is not required for correctness

`~/.uze/cache` is reserved and unwritten. Deleting it cannot affect an
installed package because nothing installed depends on it.

---

## Lifecycle safety (ADR-009, carried forward)

UZE never destroys external state it cannot positively identify. Drift,
conflict, and an unreadable ledger all block a destructive operation rather
than authorizing one. Every M2 addition preserved this: a failed acquisition,
a rejected package and a refused consent all mutate nothing.

---

## Prompt history (ADR-038 companion)

### A prompt is recorded only when its reconstruction is trustworthy

UZE forwards keystrokes to a PTY whose line editor it cannot observe, so the
submitted text is reconstructed client-side. Anything that could rewrite the
line invisibly — history recall, completion, an escape, a control chord —
discards the reconstruction instead of persisting a prompt the user never
typed. The history is a navigation aid, never a record of a session.

> `src/ui/orchestrator.rs::prompt_buffer_tests::history_recall_discards_the_reconstruction`
> `src/ui/orchestrator.rs::prompt_buffer_tests::a_control_or_alt_chord_discards_the_reconstruction`

### Prompt text is owner-only, workspace-scoped, and deletable

Each workspace keeps its own append-only file; one workspace can never evict
another's entries, the file and its directory are `0600`/`0700`, and
`prompt_history::clear` deletes a workspace's history outright.

> `crates/uze-core/src/prompt_history.rs::tests::history_is_owner_only`
> `crates/uze-core/src/prompt_history.rs::tests::each_workspace_keeps_its_own_history`
> `crates/uze-core/src/prompt_history.rs::tests::clear_removes_only_the_named_workspace_and_tolerates_absence`

---

## Official marketplace (M3, ADR-032)

### The repository is the official marketplace

`marketplace.json` + `plugins/**` at the repo root answer "which plugins
exist, and where" — the same contract a Git or local marketplace root would
satisfy. `uze-core::acquisition::marketplace` reads that contract; it holds
no opinion on how the directory reached local disk.

> `crates/uze-core/src/acquisition/marketplace.rs` tests, notably
> `two_distinct_plugins_resolve_independently_with_no_special_casing`

### Store, Engine, Router and every Integration stay marketplace-neutral

None of them import `acquisition::marketplace` or its types.

> `tests/exposure_naming.rs::store_engine_router_and_integrations_stay_marketplace_neutral`

### Adding a plugin to the marketplace needs no Rust change

Resolution is generic over plugin content: files + one `marketplace.json`
entry, nothing more.

> `crates/uze-core/src/acquisition/marketplace.rs::two_distinct_plugins_resolve_independently_with_no_special_casing`

### Default plugins are policy, not marketplace fact

`bootstrap::DEFAULT_PLUGIN_IDS` names which marketplace plugins install on a
fresh `UZE_HOME`; the marketplace itself may offer more.

> `crates/uze-application/src/application.rs::bootstrap_installs_exactly_the_default_policy_and_is_idempotent`

### Bootstrap installs; it never silently updates

`ensure_default_plugins` — run before every CLI dispatch, including
read-only commands — only installs a default plugin that is absent. An
already-installed plugin's content is never rewritten as a side effect of a
diagnostic command.

> `crates/uze-application/src/application.rs::bootstrap_never_mutates_an_already_installed_default_plugin`
> `crates/uze-application/src/application.rs::read_only_bootstrap_leaves_store_state_byte_identical_on_repeat`

### A newer snapshot is reported, never silently applied

`PluginSummary::update_available` is a pure read (a scratch-directory
comparison, discarded before returning); acting on it is a separate,
explicit `update_plugin` call.

> `crates/uze-application/src/application.rs::bootstrap_never_mutates_an_already_installed_default_plugin`

### A default plugin crossing the trust boundary is never installed silently

`PackageSource::Embedded` crosses the trust boundary like `Git`; a
non-interactive bootstrap authority (`NoTrustAuthority`) refuses rather than
granting, even for the official marketplace.

> `crates/uze-application/src/application.rs::a_default_plugin_that_would_cross_the_trust_boundary_is_not_installed_silently`

---

## Portable Hook delivery (ADR-033)

### Hook semantics are assessed per event/effect, never by event names alone

A `Stop` hook is never represented as an OpenCode tool callback and an
`ask`/`transform` effect never attaches where the target cannot preserve it;
a degraded or unsupported route states the exact loss.

> `tests/integrations/hooks.rs::compatibility_is_semantic_and_never_fabricates_a_stop_equivalence`
> `tests/integrations/hooks.rs::transform_is_adaptable_through_the_bridge_and_degraded_on_claude`

### Hook delivery is receipt-owned and content-identity safe

Merging adds only the exact rendered entry; inspection compares that exact
content; removal refuses drift and preserves foreign hooks, plugins,
entries, and ordering — and a UZE-created file holding nothing but UZE's
own entry is cleaned up when the last entry goes.

> `tests/integrations/hooks.rs::claude_merges_into_settings_json_preserving_foreign_content`
> `tests/integrations/hooks.rs::foreign_codex_hooks_survive_attach_and_detach`
> `crates/uze-integrations/src/hooks.rs::drift_blocks_removal_and_an_empty_file_is_removed`

### A package's hooks attach once per harness, idempotently

Re-attach never duplicates; an update replaces the previous version of the
same group instead of stacking it; the OpenCode bridge is package-scoped,
single-sourced (the auto-discovered global plugin directory — never a
second `plugin` config entry) and regenerates from the receipt set.

> `tests/integrations/hooks.rs::an_update_replaces_the_previous_version_of_the_samed_group`
> `tests/integrations/hooks.rs::opencode_bridge_is_package_scoped_and_regenerates_across_groups`

### The dispatcher never silently weakens a safety hook

Launch failure, timeout, oversized output, and a non-zero exit (except the
canonical deny exit) are fail-open for observational hooks and fail-closed
(a deny) for declared deny/ask/transform effects; the first deny stops
later handlers. A deny is translated into the harness's own blocking
contract (JSON decision plus exit 2 on the command-hook harnesses) —
internal exit codes never leak outward, because any other non-zero exit is
a non-blocking error there.

> `crates/uze-core/src/hook.rs::observation_fails_open_but_a_declared_deny_effect_fails_closed`
> `crates/uze-core/src/hook.rs::handlers_run_in_order_and_the_first_deny_stops_later_ones`
> `crates/uze-core/src/hook.rs::timeout_terminates_a_hung_handler_and_fails_closed_for_deny`
> `crates/uze-integrations/src/hooks.rs::adapters_render_native_decisions_and_block_exit_codes`

## Concurrent work isolation (`add-portable-worktree-policy`)

### Two agents never share a checkout

The primary checkout seats one agent. The first agent in a repository starts
there; every additional live agent starts in an isolated checkout of its own.
The guarantee is structural — it needs no harness to cooperate and no model
to agree — and it degrades to seating the agent rather than refusing to
launch it when isolation is impossible.

> `src/ui/orchestrator.rs::seat_tests::one_agent_in_the_primary_takes_the_seat`
> `src/ui/orchestrator.rs::seat_tests::an_agent_in_an_isolated_checkout_leaves_the_seat_free`
> `src/ui/orchestrator.rs::seat_tests::a_shell_tab_does_not_take_the_seat`
> `crates/uze-core/src/worktree.rs::an_isolated_checkout_does_not_occupy_the_seat`

### A pane that moves inside the repository keeps its seat

Pane directories are probed live, so occupancy is judged by which checkout a
pane is in, never by an exact path. Otherwise an agent that `cd`s one level
down would free the seat and let a second agent in beside it.

> `src/ui/orchestrator.rs::seat_tests::an_agent_that_moves_within_the_primary_keeps_the_seat`

### One repository is one terminal server

The server — and therefore the whole set of agent panes and their seat — is
keyed on the resolved workspace root, not the launch directory. Keyed on the
raw cwd, launching UZE from a repository and from a subdirectory of it
produced two servers over one checkout, each believing its seat was free.

> `crates/uze-core/src/workspace.rs::a_subdirectory_and_its_workspace_root_resolve_to_one_answer`

### Isolation never destroys and never silently reuses

A name already taken by a directory or a branch is suffixed, never reused;
a stale registry entry is pruned before creation; a repository with no commit
to branch from fails cleanly instead of half-creating.

> `crates/uze-core/src/worktree.rs::a_taken_name_is_suffixed_rather_than_reused_or_refused`
> `crates/uze-core/src/worktree.rs::isolation_fails_cleanly_on_a_repository_with_no_commits`

### An isolated checkout is invisible to the seat's own commits

Creating one ignores the isolation directory, idempotently and without
touching foreign entries. Unignored, `git add -A` in the primary stages
another agent's whole working tree as an embedded repository.

> `crates/uze-core/src/worktree.rs::isolation_creates_a_checkout_on_its_own_branch_and_ignores_the_directory`
> `crates/uze-core/src/worktree.rs::ignoring_the_directory_is_idempotent_and_preserves_existing_entries`

### The projection never triggers a harness's own isolation

The text projected into the shared baseline states where the reader already
is and how to isolate a subagent; it never asks for a top-level worktree. A
harness with its own worktree primitive activates on exactly that
instruction, and would isolate a second time on top of the checkout UZE
already placed the agent in.

> `tests/projection/worktree_policy.rs::the_projection_never_triggers_a_harnesss_own_isolation`
> `crates/uze-core/src/worktree.rs::the_projected_text_never_asks_for_a_top_level_worktree`

### A declaration stays editable

The region's identity carries the rendered content's digest, so changing the
lock reads as one region going stale and another appearing — never as drift
inside the region that already exists. Exactly one policy region exists at a
time, and a hand edit still drifts and is refused.

> `tests/projection/worktree_policy.rs::editing_the_declaration_replaces_its_region_rather_than_drifting`
> `tests/projection/worktree_policy.rs::an_edited_region_is_blocked_not_overwritten`

### A change view shows one checkout, never the repository

The Git changes extension is scoped to the checkout the active tab is in, and
resolves every `git` call against it. `git worktree list` answers
repository-wide from anywhere inside the repository, so listing linked
worktrees put the seat's diff and every sibling agent's — including checkouts
whose agent is long gone — inside a tab that owns exactly one of them.

> `crates/uze-extensions/src/git_diff.rs::a_view_is_scoped_to_the_checkout_it_was_opened_in`

### A replaced lock field is rejected, never silently dropped

`ProjectLock` does not deny unknown fields, so the superseded
worktree-directory key is refused by name — a declared policy can never
become no policy in silence.

> `crates/uze-core/src/project_lock.rs::the_replaced_directory_key_is_rejected_rather_than_silently_dropped`

---

## Decisions deliberately *not* taken

Recorded because absence is a decision, and because each one has been proposed
and set aside for a reason rather than forgotten.

| Not built | Why not, for now |
|---|---|
| third-party marketplace hosting | UZE's own official marketplace (M3) is a discovery/acquisition contract over its own repository; hosting *other* publishers' marketplaces is a different product |
| remote registry | nothing to serve until acquisition is dogfooded; M3's `marketplace.json` contract is shaped to allow a remote source later without touching Store/Engine/Integration |
| cache | correctness first; a cache is an optimization with a second-source-of-truth risk |
| content-addressable store | a Git commit is already a content identity, and a local path has no reproducibility to protect |
| lockfile | meaningful only with dependencies between packages, which do not exist |
| GitHub-specific source | a host is a URL; the mechanism is Git |
| automatic update | requires update semantics to be exercised by hand first |
| persistent trust database | one consent boundary, not a permission system |
| network-dependent gating test | local bare repositories prove the mechanism; a public repo would test that provider's availability |
| remote marketplace search | only one marketplace (embedded, official) exists; nothing to search yet |
| installable extensions | the TUI's Extensions screen catalogs bundled, compiled-in extensions (`ExtensionRegistry::builtin`); loading/enablement of user-installed extensions is not built |
| plugin version resolver | `plugin.json` carries no version field yet; nothing depends on one |
| marketplace federation | one official marketplace; combining several is unproven need |
| Git sparse checkout for marketplace sources | the `marketplace.json` contract is shaped to allow acquiring only a resolved plugin's subtree later; not implemented |
| reverse/foreign harness-format import | the acquisition contract is canonical `plugin.json` only (M2); a foreign-format importer (`ClaudePluginImporter`) existed as dead, unreachable code and was removed (ADR-005) — foreign import staying structurally separate from harness delivery is still the intended shape if it returns, but nothing is retained in production speculatively |
