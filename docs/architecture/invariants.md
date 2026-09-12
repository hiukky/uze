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

> `tests/integrations/vendor_neutral.rs::the_store_writes_no_harness_owned_artifact_of_its_own_accord`

### Integrations own vendor semantics

Which packages belong in a harness-owned view, what shape that view takes, and
when it is rebuilt are decisions of the integration that owns the harness.

> `tests/integrations/vendor_neutral.rs::a_package_without_the_native_envelope_is_not_published`

### Adding a harness requires no semantic change to Store, Engine, Router or the package model

Proven by adding a fifth harness with a materially different native delivery:
Codex needs a published catalogue, Antigravity CLI needs neither but
*copies* (its `agy plugin install` stages bytes — no link verb exists), and
both go through the same `IntegrationPort`.

> `tests/integrations/vendor_neutral.rs::the_store_contains_no_source_mechanism_semantics`
> `tests/integrations/vendor_neutral.rs::no_core_module_depends_on_acquisition`

### Package publication and package-native delivery are independent

`republish_packages` maintains a derived view; `attach_package` performs a
native delivery. Codex uses both. Antigravity uses only the second — its
`republish_packages` is never overridden.

> `tests/integrations/vendor_neutral.rs::a_derived_view_is_rebuilt_from_the_package_set_alone`
> `tests/integrations/vendor_neutral.rs::republish_is_a_noop_for_an_integration_that_publishes_nothing`

### A failed derived view never invalidates an installation

One harness failing to publish leaves the package installed, the other
harnesses untouched, and the failure observable through `doctor`.

> `tests/integrations/vendor_neutral.rs::a_failed_publication_leaves_the_package_installed_and_says_so`

### The Codex generated envelope is self-contained

Codex stages a plugin into its own cache without following symlinks, so a
generated Codex envelope carries real bytes: every default-policy Skill and
the `.mcp.json` are mirrored from the Store, a symlink the package keeps
inside itself is resolved to the file it names, and a link that escapes the
package is refused by name rather than dropped. The envelope is still a
Derived Artifact (ADR-013 §5, amended): rebuilt wholesale from the Store on
every materialization, never authoritative.

> `crates/uze-integrations/src/codex/generate.rs::generated_native_tests::materialize_generated_package_never_writes_into_the_store_package`
> `crates/uze-integrations/src/codex/generate.rs::generated_native_tests::envelope_mirrors_supporting_files_and_resolves_in_package_symlinks`
> `crates/uze-integrations/src/codex/generate.rs::generated_native_tests::envelope_refuses_a_symlink_that_escapes_the_package`
> Real-harness proof: `conformance/harnesses/codex/scenarios.py`, phase
> `skill-invocation-policy` (`default-skill-offered` through Codex's own
> plugin cache) and the `/mcp` inventory check in phase `tui`.

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

> `tests/integrations/harness/codex.rs::codex_routes_every_combination_honestly`
> `tests/integrations/harness/antigravity.rs::antigravity_routes_every_combination_honestly`
> `tests/integrations/harness/opencode.rs::opencode_routes_each_combination_by_what_the_vendor_carries`
> `tests/integrations/harness/claude.rs::claude_routes_every_combination_at_capability_level`
> `tests/integrations/harness/codex.rs::codex_generated_package_never_claims_a_model_only_skill`
> `tests/integrations/harness/antigravity.rs::antigravity_generated_package_never_claims_a_user_only_skill`
> `tests/integrations/harness/claude.rs::claude_generated_package_covers_a_user_only_skill_and_materializes_the_marker`
> `crates/uze-integrations/src/claude/plugin.rs::claude_native_coverage_tests::explicit_user_only_skill_with_the_vendor_marker_is_covered`
> `crates/uze-integrations/src/claude/plugin.rs::claude_native_coverage_tests::explicit_user_only_skill_without_the_vendor_marker_is_not_covered`
> `crates/uze-integrations/src/claude/generate.rs::generated_native_tests::user_only_skill_is_materialized_with_the_claude_marker`

### A Skill nobody may invoke is never projected (ADR-030)

`invoke: {model: false, user: false}` is invalid — it is parsed and kept
explicit (never silently defaulted to a model-visible or user-visible
combination), every integration routes it Unsupported, and no receipt,
symlink or generated file is ever created for it.

> `tests/integrations/policy.rs::invalid_policy_never_creates_a_receipt_anywhere`
> `crates/uze-core/src/capability/skill.rs::invalid_combination_is_kept_explicit_never_defaulted`

### Existing Skills without `invoke:` behave exactly as before (ADR-030)

The canonical default is model+user; a SKILL.md with no `invoke:` block is
discovered with no parsed policy (defaults apply), delivered byte-preserving
where it always was, and never gains a policy sidecar it did not declare.

> `tests/integrations/policy.rs::default_skill_package_installs_cleanly_on_every_harness_as_before`
> `tests/integrations/policy.rs::absent_invoke_block_defaults_to_model_and_user_and_behaves_as_before`
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

> `tests/projection/invocation.rs::installing_another_plugin_never_renames_an_existing_one`
> `tests/projection/invocation.rs::labels_never_touch_canonical_identity_store_or_receipts`
> `tests/projection/invocation.rs::claude_declares_plain_and_namespaces_natively_without_double_prefix`
> `tests/projection/invocation.rs::physical_representations_preserve_the_semantic_label`
> `tests/projection/naming.rs::two_packages_with_the_same_skill_name_coexist_deterministically`

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

> `tests/integrations/vendor_neutral.rs::the_store_contains_no_source_mechanism_semantics`

### Remote mutable references resolve to immutable Git commits

A branch, a tag and an unspecified reference all persist as a commit SHA. The
default branch is read from the remote, never guessed from a hardcoded name.

> `tests/packages/acquisition.rs::a_branch_resolves_to_an_immutable_commit`
> `tests/packages/acquisition.rs::a_tag_resolves_to_an_immutable_commit`
> `tests/packages/acquisition.rs::an_unspecified_reference_resolves_the_repositorys_own_default_branch`

### Reinstall uses resolved provenance; update re-resolves requested provenance

Reinstalling stays at the recorded commit even after the branch has moved.
Updating asks the original request again and may land on a new commit.

> `tests/packages/acquisition.rs::reinstalling_a_resolved_commit_stays_at_that_commit`
> `tests/packages/acquisition.rs::updating_re_resolves_the_request_and_moves_with_the_branch`

A local path has no immutable revision, and the model does not pretend
otherwise: reinstall and update both re-read the directory as it is now.

### Installed packages are self-contained

No symlink the Store persists may resolve outside the package root. Every
source is held to the same rule, and validation runs before any byte is
written, so a rejected package leaves nothing behind.

> `tests/package_containment.rs` — absolute escape, `..` escape, chained
> escape, escape through a symlinked directory, and a local package held to
> the identical rule.

An absolute symlink target is refused whatever it names, the source's own
content included: the Store copies a link's target verbatim, so only a
relative link still points inside the package once copied.

> `tests/packages/containment.rs::an_absolute_symlink_into_the_source_itself_is_rejected`

### Package discovery never follows directory symlinks

Discovery uses `symlink_metadata` and never descends into a symlink, which
makes the traversal acyclic by construction. Containment forbids leaving the
root but does not forbid a cycle inside it.

> `tests/packages/containment.rs::a_mutual_symlink_cycle_does_not_hang_discovery`
> and the self-link and ancestor-link cases beside it.

The documented cost: content reachable only through a symlinked directory is
not discovered. The symlink is still preserved as package content.

> `tests/packages/containment.rs::content_reachable_only_through_a_symlinked_directory_is_not_discovered`

Digesting walks by the same rule. `tree_sha256` runs over a freshly
materialized remote checkout before any other guard has had a say, so a link
to an ancestor would be an unbounded descent and a process abort rather than
a digest.

> `crates/uze-core/src/digest.rs::tests::a_directory_symlink_pointing_at_the_tree_itself_does_not_recurse`
> and `tests/packages/containment.rs::a_symlinked_directory_pointing_at_its_own_ancestor_does_not_hang_discovery`

### Remote executable capabilities cross an explicit consent boundary

A remote package declaring an MCP `command` requires explicit consent before
anything is written or attached. A declarative package requires none.

> `tests/packages/acquisition.rs::a_remote_package_with_an_mcp_command_requires_trust`
> `tests/packages/acquisition.rs::a_remote_package_with_only_a_skill_requires_no_trust`
> `tests/packages/acquisition.rs::denied_trust_leaves_the_store_completely_untouched`
> `tests/packages/acquisition.rs::a_non_interactive_process_reports_trust_required_rather_than_assuming_consent`

**This is a consent boundary, not a security sandbox, and not a provenance
guarantee.** It is scoped to remote acquisition: `uze add ./local` is treated
as an operator-controlled source and asks nothing, even with an MCP server.
Cloning a repository by hand and installing the result as a local path
deliberately changes the classification of that origin. UZE does not
fingerprint downloads, mark them, track origins out of band, or persist trust
decisions — and should not be described as if it did.

> `tests/packages/acquisition.rs::a_local_package_with_an_mcp_command_still_requires_no_trust`

Consent is not inherited across an update. A revision introducing execution
the installed one did not have asks again.

> `tests/packages/acquisition.rs::an_update_introducing_executable_capability_asks_again`

### Acquisition never executes package code

Cloning does not run hooks, does not recurse submodules, and reads
configuration from nothing but explicit flags. Capability inspection parses
declarations; it invokes nothing.

> `tests/packages/acquisition.rs::submodules_are_not_recursed_into`

### Cache is not required for correctness

`~/.uze/cache` holds three caches, each reconstructable from a live read:
harness detection (`harness_detection.json`), attachment inspection
(`inspection.json`) and the catalogue of every marketplace registered by
URL (`marketplaces/<name>/`). Deleting the directory costs one probe, one
inspection or one clone; nothing installed depends on it, and no mutating
path trusts it — removal planning re-inspects live, and a mutation
invalidates the entries it touched.

> `crates/uze-application/src/application/marketplace_catalogue.rs::tests::a_stored_catalogue_answers_without_the_source_being_reachable`
> `crates/uze-application/src/application/doctor.rs::tests::installation_invalidates_the_inspection_cache`

---

## Lifecycle safety (ADR-009, carried forward)

UZE never destroys external state it cannot positively identify. Drift,
conflict, and an unreadable ledger all block a destructive operation rather
than authorizing one. Every M2 addition preserved this: a failed acquisition,
a rejected package and a refused consent all mutate nothing.

### A blocked mutation says so in the exit status

`Blocked` means nothing was removed or updated, so `uze plugin remove` and
`uze plugin update` render the report and then exit non-zero — in both text
and JSON. A caller chaining `uze plugin remove x && uze plugin install y`
must not run the second half after the first did nothing.

> `tests/cli/machine.rs::a_blocked_removal_reports_and_fails`
> `tests/cli/machine.rs::a_blocked_update_reports_and_fails`

### An update that fails leaves the plugin installed

The removal is what makes an update destructive, so everything that can
refuse without needing the removed state refuses before it: re-resolving,
materializing, the trust question, and preparing the detected harnesses.
What genuinely cannot be asked first — the ingest, and the environment the
new revision composes — runs with the previous revision's bytes kept aside,
and a failure puts them back: bytes, registration and attachments, reported
as blocked. A Git- or path-sourced plugin has nothing on the machine that
would heal it otherwise.

> `crates/uze-application/src/application/tests.rs::an_update_a_harness_refuses_removes_nothing`
> `crates/uze-application/src/application/tests.rs::an_update_that_fails_after_the_removal_puts_the_revision_back`

---

## The command line's edges

### The updater fetches from the release page and nowhere else

`self_update` downloads a binary and renames it over the one in `PATH`, so
its download root is a constant. `UZE_BASE_URL` is `install.sh`'s testing
override and is honoured only by a debug build: were it a runtime input, an
environment variable would choose which binary a person runs from then on,
and the checksum could not tell, since `SHASUMS256.txt` comes from the same
root.

> `src/self_update.rs::tests::an_untrusted_base_url_is_ignored`

### Ordinary shell usage is never a panic

A closed pipe (`uze doctor | head -3`) ends the process at `SIGPIPE`, and an
argument that is not UTF-8 is clap's error to report — neither is an exit
101 with a panic message.

> `tests/cli/grammar.rs::a_reader_that_leaves_ends_the_command_quietly`
> `tests/cli/grammar.rs::a_non_utf8_argument_is_refused_rather_than_panicked_on`

### No credential reaches the trace

A marketplace or plugin source may carry userinfo (`https://user:token@…`),
and the command span is appended to `uze.log` and exported over OTLP. The
argument line is redacted before it is recorded.

> `src/telemetry.rs::tests::a_credential_in_the_argument_line_never_reaches_the_trace`

---

## Prompt history (ADR-038 companion)

### A prompt is recorded only when its reconstruction is trustworthy

UZE forwards keystrokes to a PTY whose line editor it cannot observe, so the
submitted text is reconstructed client-side. Anything that could rewrite the
line invisibly — history recall, completion, an escape, a control chord —
discards the reconstruction instead of persisting a prompt the user never
typed. The history is a navigation aid, never a record of a session.

> `src/ui/orchestrator/tests.rs::prompt_buffer_tests::history_recall_discards_the_reconstruction`
> `src/ui/orchestrator/tests.rs::prompt_buffer_tests::a_control_or_alt_chord_discards_the_reconstruction`

### Prompt text is owner-only, workspace-scoped, and deletable

Each workspace keeps its own append-only file; one workspace can never evict
another's entries, the file and its directory are `0600`/`0700`, and
`prompt_history::clear` deletes a workspace's history outright.

> `crates/uze-core/src/prompt_history.rs::tests::history_is_owner_only`
> `crates/uze-core/src/prompt_history.rs::tests::each_workspace_keeps_its_own_history`
> `crates/uze-core/src/prompt_history.rs::tests::clear_removes_only_the_named_workspace_and_tolerates_absence`

---

## The workspace client (ADR-038 companion)

### Nothing the workspace client draws waits on a repository

Every Git read the workspace makes — the tab strip's badge, the sidebar's
commit timeline, a commit's account, the changes overlay and its per-file
diff — runs on a thread of its own and answers through a channel. So does
placing a new agent, which is `git worktree add` plus the project's link
materialization and its `setup` command, and so is slot reconciliation,
which rewrites the task store and collects garbage.

The client used to read Git inline: `refresh_git_badge` ran inside the
`dirty` branch immediately before `terminal.draw`, and selecting a file in
the overlay loaded and highlighted its diff from the key handler. On an
ordinary repository `git status --untracked-files=all` outlasts several
frames, which made that a stalled UI by construction rather than by
accident.

Three rules hold it: the render and input halves of the client may not
name the extension host at all, nor build an application of their own
through `tui_application` — the other way into the domain, by which
finishing and discarding a preserved task ran `git worktree remove` and a
recursive directory removal on the thread that draws — and in the file
they are driven from every mention of the host is inside a
`thread::spawn`.

> `tests/architecture/layering.rs::architecture_rules_hold` ("drawing the workspace reaches nothing")
> `tests/architecture/layering.rs::architecture_rules_hold` ("drawing the workspace reaches nothing, by any name")
> `tests/architecture/layering.rs::the_workspace_client_reaches_for_git_only_from_a_thread`
> `src/ui/orchestrator/tests.rs::workspace_tests::scheduling_a_git_read_reserves_the_checkout_and_answers_nothing`
> `src/ui/orchestrator/tests.rs::workspace_tests::discarding_a_preserved_task_is_asked_for_rather_than_done_on_the_keystroke`
> `crates/uze-extensions/src/code/tests.rs::selecting_a_file_asks_for_its_diff_rather_than_reading_it`

### An answer that arrives late is dropped, never drawn

Every background read is tagged with the question it answers — the
checkout, the commit hash, the placement the viewer was at. A read whose
question the viewer has since moved past releases its key and is
discarded; it is not wrong, it is no longer what is being asked. This is
what makes an unbounded read safe to start from a keystroke.

> `src/ui/orchestrator/tests.rs::workspace_tests::a_git_answer_for_another_checkout_is_released_and_dropped`
> `src/ui/orchestrator/tests.rs::workspace_tests::a_commit_account_arrives_only_for_the_row_last_clicked`

### Every background read answers, and the answer is what releases its key

A reservation is taken before a read starts and released when its answer
lands, so a read that ends in silence — the application would not open,
the repository was removed under the agent, the work panicked — leaves
that feature reserved for the rest of the session. Every answer therefore
carries the key it was reserved under rather than only the data it found,
every spawned read answers on every path, and the work is run so that a
panic still produces the "nothing found" answer the absorber already
draws.

> `src/ui/orchestrator/tests.rs::workspace_tests::a_delivery_that_answered_nothing_still_gives_the_task_back`
> `src/ui/orchestrator/tests.rs::workspace_tests::a_terminal_runtime_that_went_away_is_said_rather_than_waited_on`

### An extension describes every surface it has, including a sidebar section

`view::View` was never the whole contract: the commit timeline was drawn
by hand from the extension's raw data, which put the palette, the eliding
and the hit rectangles on the host's side of a boundary whose whole point
is that they are not there. A section is now `view::Section`, drawn by
`extension_view::render_section` like any other extension surface, and its
hits come back as `ExtensionHit` — one variant per surface, so
"row 3 was clicked" cannot be confused between a file list and a list of
commits.

> `src/ui/orchestrator/tests.rs::workspace_tests::the_timeline_speaks_only_the_extensions_vocabulary`
> `crates/uze-extensions/src/code/tests.rs::the_timeline_section_names_meaning_rather_than_colour`

### Slot lifecycle is the application's, not the client's

Which pane sits in which checkout is a client fact. What that *means* for
a task — that several paths of one repository reconcile once, that a
release precedes the collection acting on it, that only removals which
cannot lose work are taken, and that a directory a pane is still in is
never collected or reused whatever its record says — is domain, and a
caller getting the order wrong hands one agent's slot to another.

> `tests/acceptance/engine.rs::one_reconciliation_pass_answers_a_repository_once_however_it_is_named`
> `crates/uze-application/src/application/services/tasks.rs::placement_tests::a_delivered_tasks_slot_stays_its_agents_while_a_pane_sits_in_it`

### One task document, one writer at a time

Evaluation, delivery, placement and occupancy reconciliation are four
threads of the client, each reading the project's task document, changing
it and writing it back. The write is atomic, which is not the same as the
pair being one operation: the later writer used to erase the other's
record, so a delivered task came back `Ready` and the next tick offered to
push and open its request a second time, and a placed agent's row could
vanish while its slot stayed taken. Every mutation now runs inside
`task::locked`, which holds the document across the whole read-modify-write.

That lock is the **outer** one: everything under it speaks to Git, and Git
serializes itself under `uze_git`'s repository write lock. Nothing unbounded
belongs inside it — a placement runs the project's `setup` after the lock is
released.

A mutation that could not be written is never reported as having happened:
a delivery carries the reason in `DeliveryReport::warnings`, and a placement
whose task could not be recorded gives the slot back and answers
`Unisolated`.

> `crates/uze-core/src/project/task.rs::tests::overlapping_mutations_do_not_erase_each_other`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::an_evaluation_that_overlaps_a_delivery_does_not_erase_it`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::a_delivery_that_could_not_be_recorded_says_so`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::a_placement_that_could_not_be_recorded_gives_the_slot_back`

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

> `tests/projection/naming.rs::store_engine_router_and_integrations_stay_marketplace_neutral`

### Adding a plugin to the marketplace needs no Rust change

Resolution is generic over plugin content: files + one `marketplace.json`
entry, nothing more.

> `crates/uze-core/src/package/acquisition/marketplace.rs::two_distinct_plugins_resolve_independently_with_no_special_casing`

### Default plugins are policy, not marketplace fact

`bootstrap::DEFAULT_PLUGIN_IDS` names which marketplace plugins install on a
fresh `UZE_HOME`; the marketplace itself may offer more.

> `crates/uze-application/src/application/tests.rs::bootstrap_installs_exactly_the_default_policy_and_is_idempotent`

### Bootstrap installs; it never silently updates

`ensure_default_plugins` — run before every CLI dispatch, including
read-only commands — only installs a default plugin that is absent. An
already-installed plugin's content is never rewritten as a side effect of a
diagnostic command.

> `crates/uze-application/src/application/tests.rs::bootstrap_never_mutates_an_already_installed_default_plugin`
> `crates/uze-application/src/application/tests.rs::read_only_bootstrap_leaves_store_state_byte_identical_on_repeat`

### A newer snapshot is reported, never silently applied

`PluginSummary::update_available` is a pure read (a scratch-directory
comparison, discarded before returning); acting on it is a separate,
explicit `update_plugin` call. Every CLI dispatch — `doctor`/`list`
included — stays on the reporting side of that line.

> `crates/uze-application/src/application/tests.rs::bootstrap_never_mutates_an_already_installed_default_plugin`

### Automatic update is local-only, and never grants trust

`auto_update_plugins` — the one caller of `update_plugin` that no person
typed, run when the TUI opens — touches only `PackageSource::Embedded`
packages (bytes already inside the running binary; no network, no
re-resolution of a Git or path source) and runs under `NoTrustAuthority`,
so a revision introducing new executable capability is reported and left
for an explicit confirmation rather than applied.

> `crates/uze-application/src/application/tests.rs::auto_update_applies_a_pending_official_snapshot_update`
> `crates/uze-application/src/application/tests.rs::auto_update_never_re_resolves_a_source_it_would_have_to_fetch`

### A default plugin crossing the trust boundary is never installed silently

`PackageSource::Embedded` crosses the trust boundary like `Git`; a
non-interactive bootstrap authority (`NoTrustAuthority`) refuses rather than
granting, even for the official marketplace.

> `crates/uze-application/src/application/tests.rs::a_default_plugin_that_would_cross_the_trust_boundary_is_not_installed_silently`

---

## Portable Hook delivery (ADR-033, ADR-040)

### Hook semantics are assessed per event/effect, never by event names alone

A `Stop` hook is never represented as an OpenCode tool callback and an
`ask`/`transform` effect never attaches where the target cannot preserve it;
a degraded or unsupported route states the exact loss.

> `tests/integrations/hooks.rs::compatibility_is_semantic_and_never_fabricates_a_stop_equivalence`
> `tests/integrations/hooks.rs::transform_degrades_on_every_harness_while_it_has_no_answer_channel`

### A delivered hook runs without the packager

The harness invokes a wrapper vendored in the delivered artifact, never the
`uze` binary, and nothing in that artifact names the packager. The wrapper
is a per-harness constant, owned alongside the entry that names it: written
on attach, drift-checked on inspect, removed with the last entry. Where no
wrapper template covers the platform, nothing is attached at all (see
below).

> `tests/integrations/hooks.rs::the_generated_wrapper_is_owned_alongside_the_entry_it_serves`
> `tests/integrations/hooks.rs::reinstalling_replaces_a_previous_packager_entry_and_leaves_foreign_ones`
> `crates/uze-integrations/src/hooks.rs::the_wrapper_is_one_byte_identical_file_per_harness`
> `crates/uze-integrations/src/hooks.rs::a_wrapper_that_lost_its_executable_bit_is_drift_and_is_repaired`
> `crates/uze-integrations/src/hooks.rs::the_last_detached_hook_entry_takes_the_shared_wrapper_with_it`

### One vocabulary drives matchers, wrappers and handlers

Each alias names the portable fields it guarantees; each harness names the
tool it matches and the native input field each portable field is read
from. A matcher intercepts every native name its alias binds, a handler
receives the same `HOOK_*` values on every harness that delivers the hook,
and a `native:` tool yields raw input only.

> `crates/uze-integrations/src/hooks.rs::every_alias_is_bound_on_every_harness_and_carries_its_portable_fields`
> `crates/uze-integrations/src/hooks.rs::a_renamed_vendor_tool_still_normalizes_to_its_alias`
> `crates/uze-integrations/src/hooks.rs::a_native_tool_the_vocabulary_does_not_bind_carries_raw_input_only`

### Hook delivery is receipt-owned and content-identity safe

Merging adds only the exact rendered entry; inspection compares that exact
content; removal refuses drift and preserves foreign hooks, plugins,
entries, and ordering — and a UZE-created file holding nothing but UZE's
own entry is cleaned up when the last entry goes. A merge into a JSON
config the *user* owns re-emits the document with their own key order
intact, so one attached hook does not reshuffle a hand-organised file.

> `tests/integrations/hooks.rs::claude_merges_into_settings_json_preserving_foreign_content`
> `tests/integrations/hooks.rs::foreign_codex_hooks_survive_attach_and_detach`
> `crates/uze-integrations/src/hooks.rs::drift_blocks_removal_and_an_empty_file_is_removed`
> `crates/uze-integrations/src/hooks.rs::a_merge_keeps_the_users_own_key_order`
> `crates/uze-integrations/src/shared/json_config.rs::a_merge_keeps_the_users_own_key_order`

### A package's hooks attach once per harness, idempotently

Re-attach never duplicates; an update replaces the previous version of the
same group instead of stacking it; the OpenCode bridge is package-scoped,
single-sourced (the auto-discovered global plugin directory — never a
second `plugin` config entry) and regenerates from the receipt set.

> `tests/integrations/hooks.rs::an_update_replaces_the_previous_version_of_the_samed_group`
> `tests/integrations/hooks.rs::opencode_bridge_is_package_scoped_and_regenerates_across_groups`

### The generated wrapper never silently weakens a safety hook

A handler answers with its exit code: `0` allows, `3` denies with the reason
on stderr. A failure to start, a timeout, and any other exit are fail-open
for observational hooks and fail-closed (a deny) for a declared
deny/ask/transform effect; the first deny stops later handlers, whatever
order the harness itself would have used. The wrapper's own dependency
(`jq`) follows the same rule. A deny is translated into the harness's own
blocking contract (its decision document plus exit 2 on the command-hook
harnesses) — internal exit codes never leak outward, because any other
non-zero exit is a non-blocking error there. This holds for every command
shape the ABI allows: a `sh <script> --flag` invocation, a relative path,
because a handler's `command` is a shell command line run from the package
root. Each handler is bounded by the timeout its own author declared, not by
a fixed default and not by the harness's backstop: past it the handler and
everything it started are stopped, and the group's effect decides.

What the wrapper answers for every recorded fixture — the native decision
document, the exit status and the reason — is a golden per harness, taken
from the in-binary runtime that used to be the second implementation of this
contract, before it was removed (ADR-040, amended).

> `crates/uze-integrations/src/hooks.rs::wrapper_tests::the_wrapper_answers_every_fixture_as_recorded`
> `crates/uze-integrations/src/hooks.rs::wrapper_tests::a_handler_that_cannot_run_follows_the_groups_effect`
> `crates/uze-integrations/src/hooks.rs::wrapper_tests::a_missing_wrapper_dependency_follows_the_groups_effect`
> `crates/uze-integrations/src/hooks.rs::wrapper_tests::a_denial_is_relayed_in_each_harnesss_own_dialect`
> `crates/uze-integrations/src/hooks.rs::wrapper_tests::a_handler_is_stopped_at_the_deadline_its_author_declared`

### A hook UZE cannot deliver is never half-delivered

There is one implementation of the hook contract, and it is the generated
wrapper. A platform or harness the template does not cover gets no native
entry at all and is reported Unsupported with that reason — never an entry
pointing at a second implementation, and never a Native verdict for a
delivery that did not happen.

> `crates/uze-integrations/src/hooks.rs::a_platform_without_a_wrapper_template_delivers_no_hook`
> `crates/uze-integrations/src/hooks.rs::a_hook_that_cannot_be_delivered_is_reported_unsupported`

## Concurrent work isolation (`add-portable-worktree-policy`)

### Every agent is isolated, and the primary checkout belongs to the operator

An agent UZE launches in a Git repository with a commit starts in a slot of
its own, created before its harness does. The primary checkout is never
assigned to an agent, so the operator's uncommitted work is exactly what they
left after any number of agents have run. Where isolation is impossible the
agent starts in place and its tab says so.

> `crates/uze-application/src/application/services/tasks.rs::placement_tests::the_first_agent_is_isolated`
> `crates/uze-application/src/application/services/tasks.rs::placement_tests::three_agents_get_three_distinct_checkouts_and_none_is_the_primary`
> `crates/uze-application/src/application/services/tasks.rs::placement_tests::the_operators_uncommitted_work_survives_agents_launching`
> `crates/uze-application/src/application/services/tasks.rs::placement_tests::a_repository_without_a_commit_launches_in_place_with_the_reason`
> `src/ui/orchestrator/tests.rs::workspace_tests::an_agent_in_a_slot_carries_no_marker`

### A checkout is a slot; a task is what comes and goes

Slots are long-lived directories named by an identifier that never changes.
A new agent takes a free slot before a directory is created: the tree is put
at the base with none of the previous task's tracked or untracked files, and
ignored artifacts survive. A slot holding work is never reused.

> `crates/uze-core/src/project/checkout.rs::a_free_slot_is_reused_and_ignored_artifacts_survive`
> `crates/uze-core/src/project/checkout.rs::a_previous_tasks_edits_never_reach_the_next`
> `crates/uze-core/src/project/checkout.rs::a_new_directory_appears_only_when_none_is_free_and_the_cap_holds`
> `crates/uze-application/src/application/services/tasks.rs::placement_tests::a_delivered_tasks_slot_is_reused_by_the_next_agent`

### Work in the target is recognized by its patch, not by its commits

A forge that squashes or rebases what it merges gives the target commits of
its own, so a delivered branch stays "ahead" of it forever. Integration is
therefore read from the patch the target carries — the branch's own commits,
then the single patch a squash would have made of them — and that is what
frees a slot, prunes a branch and keeps a delivered task delivered. Read by
reachability alone, one squash merge parked a slot for the life of the
repository and every new agent paid for a checkout of its own.

> `crates/uze-core/src/project/checkout.rs::a_squash_merged_branch_frees_its_slot_and_is_pruned`
> `crates/uze-core/src/project/checkout.rs::a_rebase_merged_branch_frees_its_slot`

### An agent is placed on the target as the remote has it

The local target is fast-forwarded onto the remote's before a new agent's
branch is cut from it, and by nothing but a fast-forward: a target carrying
commits the remote lacks is left where it stands and the placement reports
how far behind the agent starts.

> `crates/uze-core/src/project/landing.rs::the_local_target_is_fast_forwarded_onto_the_remotes`
> `crates/uze-core/src/project/landing.rs::a_target_carrying_its_own_commits_is_left_alone_and_reported`
> `crates/uze-application/src/application/services/tasks.rs::placement_tests::a_new_agent_starts_from_the_target_as_the_remote_has_it`

### Publication is read from the remote, never from UZE's own records

Whether a task's branch is on the remote, under which name, and how much of
it the remote already carries are read from the repository's remote-tracking
refs; the request number is asked of the remote itself. So a branch its own
agent pushed, and a request its own agent opened, count exactly as much as
ones a delivery made — the operator is a party to this, and the button has to
report the remote's state rather than UZE's history. Read from UZE's record
of its own pushes, the delivery button went on offering to send commits the
request already carried, and a delivery after an agent's own push was refused
as a non-fast-forward. The one network question — is a request open — is
asked only where the completion publishes, only for a branch that is on the
remote, at most once a minute, and never again once answered.

> `crates/uze-core/src/project/landing.rs::a_branch_its_own_agent_pushed_is_published_and_in_sync`
> `crates/uze-core/src/project/landing.rs::a_request_the_agent_opened_is_discovered_on_the_evaluation_pass`
> `crates/uze-core/src/project/landing.rs::the_remote_is_asked_about_a_missing_request_at_most_once_a_minute`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::an_agents_own_push_and_request_are_what_the_delivery_view_reports`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::a_merge_project_never_measures_its_work_against_the_remote`

### Every surface reads one state, and a delivery in flight is the client's

What the branch holds and what the remote holds are folded into a single
`TaskStateView` before any surface sees it, so the sidebar mark, the header
button and the preserved list cannot disagree: a published branch level with
its request reads as `Published` everywhere, not as `Ready` in one column and
a sync in another. The exception is the delivery itself — `Integrating` is set
in memory and overwritten by the outcome before the store is saved, so no
evaluation can read it back, and the client that started the delivery is the
only party that knows one is running.

> `src/ui/orchestrator/tests.rs::workspace_tests::a_published_task_is_marked_as_gone_not_as_waiting_to_be_delivered`
> `src/ui/orchestrator/tests.rs::workspace_tests::a_delivery_in_flight_is_drawn_from_the_client_that_started_it`
> `src/ui/orchestrator/tests.rs::workspace_tests::a_branch_level_with_its_request_reports_the_sync_instead_of_a_count`

### Nothing that can hold work is removed automatically

A dirty orphan is parked with every file preserved. A branch with commits the
target lacks outlives its directory. The two automatic removals are a branch
fully reachable from the target and the directory of a clean slot idle beyond
an age, whose branch stays.

Both removals are authorized by one predicate, and it fails closed: a
question Git could not answer — most often a `worktrees.target` this clone
does not have — is answered "not integrated", never "nothing ahead". Read
the other way, a declared target the repository lacks made every branch in
it collectable.

> `crates/uze-core/src/project/checkout.rs::a_checkout_holding_work_is_parked_with_every_file_preserved`
> `crates/uze-core/src/project/checkout.rs::an_unintegrated_branch_outlives_its_directory`
> `crates/uze-core/src/project/checkout.rs::a_parked_slot_is_never_removed_for_being_idle`
> `crates/uze-core/src/project/checkout.rs::an_integrated_branch_is_pruned_and_an_unintegrated_one_is_not`
> `crates/uze-core/src/project/checkout.rs::a_target_this_clone_does_not_have_collects_nothing_and_frees_no_slot`

### Reconciliation adopts before it prunes

Checkouts nobody recorded become tasks — parked when they hold work — and a
legacy checkout keeps its branch name, since it may have been pushed. Git's
worktree registry is pruned only after every directory has been looked at, so
a stale entry can never be dropped before its work is.

> `crates/uze-core/src/project/checkout.rs::prune_runs_after_adoption_and_an_orphaned_task_keeps_its_branch`
> `crates/uze-core/src/project/checkout.rs::a_legacy_checkout_is_adopted_under_its_branch_name`

### A slot is invisible to the primary's own commits

The isolation directory is excluded through the repository's own
`info/exclude`, never through the operator's `.gitignore`, so the primary's
status stays what the operator left and `git add -A` there never stages a
slot as an embedded repository.

> `crates/uze-core/src/project/checkout.rs::the_isolation_directory_is_excluded_without_touching_the_primary_tree`

### Task identity is immutable; the label is derived

A task's identifier keys its branch, its slot and its persisted state, and a
new label changes none of them. State lives under UZE's own `state/`, outside
every checkout, and is written atomically: a reader sees the previous
document or the new one, never a torn one.

> `crates/uze-core/src/project/task.rs::the_identifier_is_stable_while_the_label_changes`
> `crates/uze-core/src/project/task.rs::state_survives_checkout_removal`
> `crates/uze-core/src/project/task.rs::a_kill_mid_write_leaves_the_previous_or_the_new_document`

### Repository writes are serialized under one lock

Every write to a repository goes through `uze_git::write`, which takes an
inter-process lock keyed on the common directory; reads never wait for it.
A critical section re-enters its own lock, a panic releases it, and a lock
left by a dead process is reclaimed.

> `crates/uze-git/src/lib.rs::concurrent_critical_sections_never_interleave`
> `crates/uze-git/src/lib.rs::concurrent_worktree_adds_do_not_collide`
> `crates/uze-git/src/lib.rs::a_busy_lock_is_reported_by_name_after_the_timeout_and_reads_never_wait`
> `crates/uze-git/src/lib.rs::a_lock_held_by_a_dead_process_is_reclaimed`

### Readiness is a Git fact, never an announcement

A task is ready when its checkout has commits ahead of the base on a clean
tree. That is read from the checkout when the pane goes quiet or on demand,
and never from anything the agent says; a paused rebase reads as exactly
that.

> `crates/uze-core/src/project/landing.rs::readiness_is_read_from_the_checkout`
> `crates/uze-core/src/project/landing.rs::a_task_without_commits_is_not_delivered`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::evaluation_reads_the_checkout_and_merge_delivers`

### The target is written only in deliver, and only by UZE

Delivery rebases the task's branch inside its own checkout, runs the declared
gate on the rebased commits, and only then advances the target by
fast-forward. A conflict or a failed gate leaves the target untouched and
returns the task to the agent that owns it, with the rebase paused in its
checkout. `handoff` never touches the target; `pr` publishes against the
remote's tip and never pulls the operator's local branch.

What moves is the declared target, not whatever the primary checkout has
checked out: `git merge` advances `HEAD`, so the branch is asked for first
and the ref moved directly when the operator is standing elsewhere, and the
target's new tip is read back before the task is recorded `Integrated`.

> `crates/uze-core/src/project/landing.rs::handoff_never_touches_the_target`
> `crates/uze-core/src/project/landing.rs::merge_advances_the_target_linearly_after_the_gate`
> `crates/uze-core/src/project/landing.rs::merge_moves_the_target_while_the_primary_stands_on_a_detached_head`
> `crates/uze-core/src/project/landing.rs::merge_never_moves_the_branch_the_primary_happens_to_be_on`
> `crates/uze-core/src/project/landing.rs::the_gate_runs_after_the_rebase_not_before`
> `crates/uze-core/src/project/landing.rs::a_gate_failure_leaves_the_target_untouched_and_returns_to_the_owner`
> `crates/uze-core/src/project/landing.rs::a_conflict_leaves_the_rebase_paused_and_the_target_untouched`
> `crates/uze-core/src/project/landing.rs::pr_publishes_and_leaves_the_request_to_the_agent`

### A delivery never collides with the operator's own edits

A fast-forward into the checked-out target updates the operator's working
tree, so a task touching a file the operator has uncommitted changes to is
refused before anything is written.

> `crates/uze-core/src/project/landing.rs::overlap_with_the_operators_uncommitted_work_refuses_and_writes_nothing`

### Sibling tasks share work only through the target

The second task delivered is rebased onto a target that already contains the
first; a live, clean task follows a moved target on its own, and one mid-edit
is never rebased under its agent. No task's branch ever carries another
task's commits directly.

> `crates/uze-core/src/project/landing.rs::the_second_task_sees_the_first`
> `crates/uze-core/src/project/landing.rs::a_live_task_follows_the_target_when_clean_and_is_left_alone_when_dirty`
> `crates/uze-application/src/application/services/tasks.rs::task_service_tests::evaluation_lets_a_clean_task_follow_the_target`

### A linked file is ignored by the repository

A path in `worktrees.link` must be relative, stay inside the repository and
be ignored by it; a violation is a malformed lock at read time, not a
surprise at launch. Linked or not, a failed `setup` warns and never blocks a
launch.

> `crates/uze-core/src/project/manifest.rs::a_link_escaping_the_repository_is_rejected`
> `crates/uze-core/src/project/manifest.rs::a_link_to_a_tracked_file_is_rejected_and_an_ignored_one_loads`
> `crates/uze-core/src/project/checkout.rs::a_failing_setup_warns_with_its_last_line_and_a_passing_one_is_silent`

### The projection never triggers a harness's own isolation

The text projected into the shared baseline states where the reader already
is and how to isolate a subagent; it never asks for a top-level worktree. A
harness with its own worktree primitive activates on exactly that
instruction, and would isolate a second time on top of the slot UZE already
placed the agent in.

> `tests/projection/worktree_policy.rs::the_projection_never_triggers_a_harnesss_own_isolation`
> `crates/uze-core/src/project/worktree.rs::the_projected_text_never_asks_for_a_top_level_worktree`

### A declaration stays editable

The region's identity carries the rendered content's digest, so changing the
lock reads as one region going stale and another appearing — never as drift
inside the region that already exists. Exactly one policy region exists at a
time, and a hand edit still drifts and is refused.

> `tests/projection/worktree_policy.rs::editing_the_declaration_replaces_its_region_rather_than_drifting`
> `tests/projection/worktree_policy.rs::an_edited_region_is_blocked_not_overwritten`

### A change view shows one checkout, never the repository

The code surface is scoped to the checkout the active tab is in, and
resolves every `git` call against it. `git worktree list` answers
repository-wide from anywhere inside the repository, so listing linked
worktrees would put the operator's diff and every sibling agent's — including
slots whose agent is long gone — inside a tab that owns exactly one of them.

> `crates/uze-extensions/src/code/changes.rs::repository_tests::discovers_main_and_configured_linked_worktrees`

### An unknown lock field is rejected, never silently dropped

A key `ProjectLock` does not understand is refused at every level.
Tolerating one for forward compatibility buys nothing here: the
`version` field already says whether this UZE can read the file, and a lock is
regenerated rather than preserved. `WorktreePolicy` denies unknown fields for
the opposite reason — everything a project may declare about isolation is
already named there. Both halves say the same thing: a declared policy can
never become no policy in silence.

> `crates/uze-core/src/project/project_lock.rs::a_key_this_uze_does_not_understand_is_refused`
> `crates/uze-core/src/project/manifest.rs::an_unknown_key_inside_the_policy_block_is_refused_by_name`

---

## Terminal runtime (`add-terminal-runtime`)

### One server per user; a space has a root

The terminal server is one per `UZE_HOME`, and every client attaches to it
whatever directory it was started in. A space is born from a root: starting
`uze` ensures a space rooted at the launch directory's workspace root exists
and selects it for that client, creating it only when no space has that
root. Behaviour derives from the root; there is no global space.

> `tests/acceptance/engine.rs::two_clients_keep_their_own_focus_and_a_nested_launch_opens_a_space`
> `crates/uze-terminal/src/runtime.rs::a_restarted_server_relaunches_the_same_spaces_tabs_and_agent_commands`

### Focus is per client

Which space and tab a client looks at is the client's own; the session it
receives carries its selection overlaid on the shared structure, and another
client's selection never moves it.

> `crates/uze-terminal/src/runtime.rs::a_clients_view_overlays_its_own_selection_and_heals_a_stale_one`
> `tests/acceptance/engine.rs::two_clients_keep_their_own_focus_and_a_nested_launch_opens_a_space`

### A launch inside a pane opens a space, never a client

Every pane carries `UZE_PANE`; a `uze` that finds it asks the running server
for a space at its workspace root and exits, so a client is never opened
inside a client.

> `tests/acceptance/engine.rs::two_clients_keep_their_own_focus_and_a_nested_launch_opens_a_space`

### Every question the kernel answers is asked in one place

Four facts about a process the runtime did not spawn — who is on the other
end of a socket, what image a pid runs, where it is standing, what it
inherited — come only from the kernel, and each platform exposes them
differently (`/proc` on Linux, `libproc`/`sysctl` on macOS).
`process_probe` holds every one of those readings; the decisions built on
them are written once and run unchanged everywhere.

The rule this enforces is that `None` means *unknown*, never *no*. A probe
that cannot answer must not be read as a negative answer — the endpoint keeps
the state it had rather than tearing down a healthy server, and a pane
reports no foreground status rather than an invented one.

Adding a platform means teaching `process_probe`, never widening a `cfg` at
a call site.

> `crates/uze-terminal/src/process_probe.rs::tests::the_platform_answers_about_this_process`
> `crates/uze-terminal/src/process_probe.rs::tests::a_key_matches_only_itself`
> `crates/uze-terminal/src/runtime.rs::foreground_status_prefers_the_shim_identity_over_a_version_named_comm`

### Nothing a peer sends is acted on before it is bounded

A frame's length prefix is refused before it is allocated, and the same
bound holds on the way out, so the two sides cannot disagree about what is
sendable; the bound is derived from the largest thing the protocol
legitimately carries — a full repaint of the largest pane it allows. Pane
dimensions are bounded at the edge that receives them and again where a
pane is created, because `Term::resize` allocates a cell per position and
clamps nothing of its own.

> `crates/uze-terminal/src/runtime.rs::a_length_prefix_past_the_frame_limit_is_refused_before_it_is_allocated`
> `crates/uze-terminal/src/runtime.rs::a_full_repaint_of_the_largest_pane_fits_in_one_frame`
> `crates/uze-terminal/src/runtime.rs::a_resize_to_the_largest_number_on_the_wire_leaves_the_server_answering`

### A pid file is a claim; the process table is the fact

Nothing is signalled, and no endpoint is trusted, on a pid file's word
alone: the file outlives the process it names, and pids are recycled. A pid
is signalled only where the kernel says it runs `uze`, and a process
without an executable image — a crashed server nobody reaped — is not alive
however addressable it remains. The directory the endpoint lives in is
proved to be this user's own, unreachable by anyone else, and not a
symlink, before a socket carrying every pane's contents is put in it.

> `crates/uze-terminal/src/runtime.rs::replace_incompatible_server_leaves_a_process_that_is_not_a_server_running`
> `crates/uze-terminal/src/runtime.rs::a_crashed_server_nobody_reaped_does_not_count_as_alive`
> `crates/uze-terminal/src/runtime.rs::a_runtime_directory_that_is_not_ours_to_own_is_stepped_over`

### One workspace, one server

A server holds an advisory claim on the persisted workspace for as long as
it exists, taken beside the workspace under `$UZE_HOME` rather than in the
runtime directory a `/tmp` cleaner can take away. A second server refuses to
restore a workspace another live one holds, so an endpoint that vanishes
under a running server can never turn one set of agents into two — and the
server that holds it puts itself back at the endpoint instead.

> `crates/uze-terminal/src/runtime.rs::a_second_server_refuses_to_restore_a_workspace_another_one_holds`
> `crates/uze-terminal/src/runtime.rs::a_workspace_claim_is_exclusive_and_released_with_its_holder`

### A pane's identity is its own

A pane's environment carries only what that pane's launch put there, and a
shim identity stamp is read only from the process it was stamped for.
Without both, every plain shell under a `uze` started inside a shimmed agent
reported as that agent, persisted as one, and was relaunched as one.

> `crates/uze-terminal/src/runtime.rs::a_pane_does_not_inherit_the_servers_shim_identity`
> `crates/uze-terminal/src/runtime.rs::foreground_status_ignores_a_shim_identity_stamped_for_another_process`

---

## Agent session continuity (`add-agent-session-continuity`)

### A conversation belongs to a task, never to a directory

What an agent resumes is recorded against the task, in UZE's own state
outside every checkout. A slot reset keeps it, a slot recycled for the next
task never inherits it, and a task given its checkout back finds what it
left.

> `crates/uze-core/src/project/conversation.rs::a_recycled_slots_new_task_finds_nothing_the_previous_one_left`
> `crates/uze-core/src/project/conversation.rs::the_newest_task_naming_a_checkout_owns_it`

### A relaunch resumes; the decision is made where every relaunch passes

Resume-or-start is decided inside the launched process, on the launch
boundary — the only place the terminal runtime's own restore reaches, with
no client present. A first launch names or records the conversation; the
next one continues it.

> `tests/acceptance/session_continuity.rs::a_relaunched_agent_resumes_the_conversation_its_task_was_left_in`
> `crates/uze-core/src/delivery/continuity.rs::a_second_launch_resumes_what_the_first_recorded`

### Continuity never rewrites an invocation and never blocks a launch

An invocation carrying anything of the operator's own is launched exactly as
typed, a directory no task owns is untouched, and every failure — a
conversation the harness no longer holds, unreadable state, a harness that
declares no mechanism — starts the agent anyway.

> `tests/acceptance/session_continuity.rs::an_invocation_the_operator_composed_is_launched_exactly_as_typed`
> `tests/acceptance/session_continuity.rs::a_directory_no_task_owns_launches_the_harness_untouched`
> `crates/uze-core/src/delivery/continuity.rs::a_conversation_the_harness_no_longer_holds_starts_a_new_one_and_says_so`
> `crates/uze-core/src/delivery/continuity.rs::unreadable_state_still_launches_the_agent`

### The record follows the agent, not the assignment

An identifier written once at launch goes stale the moment the person
clears, forks or switches the conversation. What is recorded is the last one
observed for the task, so what resumes is where the work was left.

> `crates/uze-core/src/delivery/continuity.rs::a_conversation_the_agent_moved_to_replaces_the_one_it_started_in`
> `crates/uze-core/src/project/conversation.rs::an_answer_to_a_replaced_launch_is_dropped`

### Continuity is declared per harness, and the matrix is derived from it

A harness says whether UZE may name its conversation, must read the name
back, or has no mechanism at all; the default is none, so a harness nobody
has looked into contributes no argument and reports no conversation. The
published matrix reads that declaration rather than restating it.

> `crates/uze-core/src/delivery/integration.rs::an_integration_that_declares_nothing_contributes_no_session_argument`
> `src/bin/uze-harness-matrix.rs --check` (pre-push; stale docs fail the push)

---

## Runtime projection lifecycle (ADR-014)

### A projection belongs to a project root, and no two share one

Each project root gets its own runtime directory, keyed by its canonical
path. A worktree is a root of its own, so a branch's `AGENTS.md` and Skills
never reach a session working on another — and two sessions on the *same*
root compute the same directory and the same content, so there is nothing to
coordinate between them.

> `crates/uze-integrations/src/claude/runtime.rs::runtime_projection_tests::repeated_projection_for_the_same_project_is_idempotent`
> `crates/uze-integrations/src/claude/runtime.rs::runtime_projection_tests::concurrent_projection_calls_never_degrade_to_passthrough`
> `tests/integrations/runtime_projection.rs::a_destroyed_checkout_loses_its_projection_and_its_repository_keeps_one`

### A projection outlives its project only until the next sweep

Every project's runtime directory records the canonical root it was built
for, and a sweep removes each one whose root is gone — the checkout of a
delivered agent, a worktree deleted by hand, a project moved. Existence of
the root is the only criterion: a projection already current is skipped
rather than rewritten, so mtime says when the project last changed, not when
it was last used, and an age rule would collect exactly the projections that
work. A directory naming no root it can be identified by is swept on the
same terms, and rebuilt by the next launch that needs it.

> `crates/uze-core/src/machine/harness_runtime.rs::tests::a_projection_outlives_its_project_only_until_the_next_sweep`
> `crates/uze-core/src/machine/harness_runtime.rs::tests::a_project_that_still_exists_is_never_swept`
> `crates/uze-core/src/machine/harness_runtime.rs::tests::a_project_directory_that_names_no_root_is_swept`
> `tests/integrations/runtime_projection.rs::a_swept_projection_is_rebuilt_by_the_next_launch`

### The runtime tree's two tenants are never confused for one another

`runtime/projects/` holds derived projections that outlive every invocation
and die with their project root; `runtime/sessions/` holds the receipts that
let a filesystem projection be undone, and dies with the invocation that made
it. They are named siblings rather than sibling ids under one integration, so
the sweep can never take one for the other, and nothing project-owned is
reached through a projection it collects.

> `crates/uze-core/src/machine/harness_runtime.rs::tests::the_sweep_keeps_both_tenants_and_nothing_else`
> `tests/integrations/runtime_projection.rs::sweeping_a_dead_projection_never_touches_the_project_it_pointed_at`
> `tests/packages/store.rs::uze_home_derives_every_owned_path_from_one_root`

---

## Architecture seams (`enforce-architecture-seams`)

### The layer direction is a fact, not a convention

No presentation file names `uze_core::` or `uze_integrations`: the CLI and
the TUI consume `uze-application` and nothing below it. The budget that
carried the transition is empty; what remains is `sanctioned` and named —
the runtime shim and the harness matrix, which share the binary crate but
are separate entry points that report on the domain rather than present
it.

> `tests/architecture/layering.rs::architecture_rules_hold`

### Appearance is data, and one vocabulary is the only way to name it

Nothing UZE draws carries a colour value or a glyph of its own. A surface
names what it *is* — a `uze_theme::Token`, a `uze_theme::Symbol` — and the
active theme decides what that looks like, so the TUI's chrome, the CLI's
output, a pane's default and indexed colours, and the palette syntax
highlighting is rendered with cannot drift apart the way four hand-kept
copies of one palette always did. Two adapters translate the vocabulary
into what each surface draws with (ratatui, anstyle) and are the only
places allowed to construct a colour; a colour that genuinely came from
content rather than from the design system passes through
`theme::content`, named so it cannot be mistaken for chrome.

A theme is a file, and a partial one is a whole one. Appearance resolves
as a stack — the built-in default, then the selected glyph set, then any
theme a theme is a variation of, then the theme, then the operator's own
overrides — and merging happens between *declarations* at every level,
never between resolved colours. That is what keeps an ancestor's references alive: repaint the
accent in a variation and everything two layers down written `@accent`
follows. Surfaces and borders are declared as a separation from the
theme's own background rather than as the value they resolve to, which is
what lets a background declaration alone carry a light theme.

The palette and the glyph set are two selections, not one, because they
are facts of different kinds: which marks a terminal can draw belongs to
the font someone installed, and a palette belongs to this afternoon.
Neither selection reads or writes the other's half. And UZE never infers
the set — no terminal can be asked which font it renders with, so the
choice is made by looking at each set drawn in its own glyphs.

UZE's own themes and glyph sets carry no emoji — an emoji is a different
font family, a width that varies by terminal, and a picture that ignores
the hue carrying the meaning. Colours bound to a hue by contract are the one thing that does
not follow a meaning: the sixteen a program inside a pane names by index are literal,
because index 2 is *green* to whatever emitted it.

> `tests/architecture/layering.rs::architecture_rules_hold`
> `tests/architecture/layering.rs::no_chrome_glyph_is_written_where_it_is_drawn`
> `crates/uze-theme/src/load.rs::the_default_carries_the_palette_that_shipped`
> `crates/uze-theme/src/load.rs::a_light_theme_gets_light_surfaces_from_the_background_alone`
> `crates/uze-theme/src/load.rs::the_terminals_own_sixteen_keep_their_hues_when_a_theme_repaints_a_meaning`
> `crates/uze-theme/src/load.rs::no_bundled_glyph_is_an_emoji`
> `src/theme.rs::a_variation_resolves_over_the_theme_it_varies`
> `src/theme.rs::the_operators_overrides_outlast_the_theme_they_are_applied_over`
> `src/theme.rs::a_glyph_set_applies_under_a_theme_that_declares_no_symbols`
> `src/theme.rs::a_themes_own_symbol_wins_over_the_selected_set`
> `src/theme.rs::the_operators_overrides_win_over_the_set_and_the_theme_both`
> `crates/uze-core/src/theme_state.rs::the_two_choices_do_not_overwrite_each_other`
> `src/ui/tests.rs::each_glyph_set_is_previewed_in_its_own_glyphs`
> `src/progress.rs::the_cli_and_the_tui_resolve_a_shared_token_to_the_same_colour`
> `src/ui/orchestrator/tests.rs::the_palette_a_pane_is_told_about_is_the_one_being_drawn`
> `crates/uze-terminal/src/runtime.rs::osc_background_and_foreground_queries_get_answered_instead_of_hanging`

### An extension describes; the host draws

An extension answers with a `view::View` and never receives a frame,
computes a rectangle, or names a colour. The host resolves semantic roles
against the one palette, wraps the content, and derives every click target
from what it actually drew — so layout has a single owner rather than two
sides computing the same geometry. Syntax highlighting is the one thing
that travels as colour, because it comes from a theme the extension ships
rather than from the host's design system.

> `src/ui/extension_view.rs::a_click_target_comes_from_what_the_host_drew`
> `src/ui/extension_view.rs::chrome_uses_the_hosts_palette_and_content_keeps_its_own`
> `crates/uze-extensions/src/code/tests.rs::the_view_names_meaning_rather_than_colour`

### An extension reaches nothing it was not handed

No `UzeHome`, no Store, no receipts, no `uze-application` — and no process,
no filesystem, no environment. Everything outside the extension's own
memory arrives through `uze_extensions::Host`, which the workspace client
implements in one named place.

An extension is code UZE runs in its own process, a different trust class
from plugin bytes a harness reads. Being a pure function of what it is
handed is what makes that tractable: a capability the host never granted is
one it can withhold, and a sandbox is a property of the loading mechanism
rather than something added afterwards. A `&mut Frame` could not cross a
process boundary; neither can a `fork()`.

The grant is no longer read-only — the file explorer saves and deletes —
and that is exactly why it is worth stating where the writing happens.
`Host::write_file` refuses a path that is not already a file, so a save
can only ever mean "save this file" and never "create whatever this string
names"; `Host::delete_file` refuses a directory, because a recursive
removal is a different act from the one a single keystroke describes. Both
live in `src/ui/extension_host.rs` and nowhere else.

> `tests/architecture/layering.rs::architecture_rules_hold`
> `src/ui/extension_host.rs::the_write_grant_is_narrower_than_the_filesystem`

### Git's exit code is reported, never classified by the transport

`uze-git` hands back what Git said. What a non-zero exit *means* is a
property of the subcommand — `diff` reporting differences, `rebase`
reporting a conflict, `rev-parse --verify` reporting a missing ref — and
flattening that into one error is how two callers ended up with two
incompatible conventions over the same binary. Reads and writes are
separate entry points so a repository write lock has one place to live,
and so a status view never blocks behind one.

> `crates/uze-git/src/lib.rs::a_non_zero_exit_is_reported_not_flattened_into_an_error`

### Git is spawned in exactly two places, for two different threat models

`uze-git` drives the operator's own checkout, so their configuration
applies. `acquisition::git` clones untrusted remote repositories, so it
strips the environment instead — `env_clear`, `GIT_CONFIG_NOSYSTEM`, hooks
disabled, no credential prompt. Merging them would be wrong in both
directions; a third spawn is what the rule prevents.

> `tests/architecture/layering.rs::architecture_rules_hold`

### No command ships without a performance decision

The leaf list is derived from `clap`'s own grammar rather than maintained
beside it, and checked both ways: a command added without a
`PerformanceClass` fails by name, and a classification for a command that
no longer exists fails by name. Verified by adding each in turn and
watching the test refuse.

> `src/command_performance.rs::every_cli_command_is_classified`
> `src/command_performance.rs::every_named_performance_test_exists`

### Every budgeted path is timed on its own, in a world where a probe shows

Each `Budgeted` command's application call is timed on a fresh
application — no in-process memo, only what is on disk — against a
harness that sleeps half a second per detection probe and a marketplace
whose repository was deleted after registration. The ceiling is 25 ms,
best of three, in a debug build: half of what the spec promises a person
on a release build, set where a regression shows. The bootstrap that
precedes every command is held to the same ceiling and to a second claim
a clock cannot make: run warm, it leaves every file under `UZE_HOME` as it
found it.

> `crates/uze-application/src/application/performance_tests.rs`
> `tests/cli/budget.rs::a_warm_read_only_command_writes_nothing_under_uze_home`

### A marketplace listing never clones

What a marketplace registered by URL offers is answered from a cached
checkout, filled by the clone `market add` already makes and refilled at
most once an hour; the repository can be unreachable, or gone, and the
listing, the plugin picker and a plugin's inspection still answer. A
local marketplace is read where it is, every time.

> `tests/cli/budget.rs::a_marketplace_registered_by_url_is_listed_without_its_repository`
> `crates/uze-application/src/application/performance_tests.rs::market_list_and_inspect_meet_the_budget_without_the_repository`
> `crates/uze-application/src/application/marketplace_catalogue.rs::tests::a_local_marketplace_is_read_where_it_is`

### Every action is one trace, and every entry point is a span

A command, a key in the TUI and a shim launch each open a root span;
every public method of an application service opens one under it, named
for what the person asked, with the id or path it acts on and the error
when it fails; every process UZE runs — Git, a clone, a vendor CLI, a
provisioning installer, a hook handler — is a span with how it ended. A
TUI worker thread enters the span that started it, so what a key caused
stays under the key. Held by a scan, not by memory: an entry point added
without `#[tracing::instrument]` fails by name.

> `tests/architecture/instrumentation.rs::every_application_entry_point_is_a_span`
> `crates/uze-application/src/application/tracing_tests.rs::a_service_call_is_one_span_tree`
> `crates/uze-git/src/lib.rs::tests::every_invocation_is_a_span_with_its_exit_code`

### A harness lives on this machine's filesystems

Resolving a harness executable — for detection, its fingerprint, the
runtime shim and the check that the shim is first — skips `PATH` entries
on a `9p` mount, the filesystem a Windows drive is inside WSL. A stat
there is a network round trip, a `PATH` inherited from Windows carries a
dozen of them, and a harness UZE integrates keeps its state under `$HOME`
on this side.

> `crates/uze-core/src/machine/harness_runtime.rs::tests::a_windows_drive_mounted_into_wsl_is_not_where_a_harness_is_looked_for`

## Harness conformance (`assert-one-capability-contract`)

### Every harness answers the same questions

A capability contract states an outcome and names no vendor; a harness's
bindings say how that harness is driven and carry no assertion. The
contract runs identically against all four, so a check can no longer live
in one vertical with nothing to disagree with it — which is how a Skill
that never reached the model read as an invocation policy working, on
three harnesses, for months.

> `conformance/contract/skill.py`
> `conformance/contract/mcp.py`

### A harness declines out loud, or not at all

A harness that cannot deliver part of a contract returns a reason from
`bindings.unsupported`, and the run records it beside the passes. Omitting
the check is not available: an omission cannot be reviewed, and the
divergence it hides is exactly what the Lab exists to surface.

This is how the suite now states that `invoke.user: false` is enforced on
one harness of four — a fact no vertical asked about before.

> `conformance/contract/skill.py::_declined`

### An absence proves nothing until something proves the surface

Every absence assertion in a contract is gated on a presence assertion.
"the policy hid it" and "the surface was empty" are the same observation
otherwise. `check_absence` already refuses an unsettled turn (ADR-035);
this is the other half of the same defence.

> `conformance/contract/skill.py::_assert_catalog`

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
| a registry-driven CLI grammar | a feature cannot contribute a command today: `Command` is a closed `clap` derive. Building it dynamically instead would rewrite a 2400-line dispatch and trade `clap`'s typed parsing for a builder — to serve an author who does not exist yet. What already holds is the part that mattered: the leaf list is derived from the real grammar, so a command added without a `PerformanceClass` fails by name, and a stale entry fails by name too. Revisit when something outside `main.rs` actually needs to register a command — an installable extension, or a second binary sharing the grammar |
| installable extensions | the TUI's Extensions screen catalogs bundled, compiled-in extensions (`ExtensionRegistry::builtin`); loading/enablement of user-installed extensions is not built |
| plugin version resolver | `plugin.json` carries no version field yet; nothing depends on one |
| marketplace federation | one official marketplace; combining several is unproven need |
| Git sparse checkout for marketplace sources | the `marketplace.json` contract is shaped to allow acquiring only a resolved plugin's subtree later; not implemented |
| reverse/foreign harness-format import | the acquisition contract is canonical `plugin.json` only (M2); a foreign-format importer (`ClaudePluginImporter`) existed as dead, unreachable code and was removed (ADR-005) — foreign import staying structurally separate from harness delivery is still the intended shape if it returns, but nothing is retained in production speculatively |

### The name is asked for as the agent's first action

The projected region's first bullet is the naming clause, and it asks for the
name before the agent reads a file, plans or edits. The moment carries the
rule: a name states an intention, which is what an agent holds at its first
turn and the one thing no amount of reading improves — while any later moment
is one the agent reaches with work already under way, and weighs against it.
There is no mechanism behind this, which is exactly why the wording and the
position are pinned by a test rather than left to whoever edits the string
next.

> `uze-core::project::worktree::naming_tests::naming_is_the_first_thing_the_projected_text_asks_for`

### Work that reaches a commit unnamed is named from that commit

Naming has an automatic half, and it is a Git fact read on the evaluation
pass that already runs — no harness is asked anything, so it behaves the
same on all four and on the next one, and it covers every completion
behaviour rather than only the one that publishes. It applies at `Ready`
and nowhere else, and a derived name the project's vocabulary would refuse
from an agent is never written on its behalf.

> `uze-application::…::derived_naming_tests::the_first_commit_names_work_nobody_named`
> `uze-application::…::derived_naming_tests::a_commit_outside_the_vocabulary_leaves_the_generated_name`

### A name nobody generated is never overwritten

`agent/` is UZE's own branch namespace: a branch inside it is still UZE's to
name, and putting a name outside it is what naming does. So "outside the
prefix" and "somebody chose this" are the same fact, and every mechanism that
could rename — the agent's own command, the publish-time fallback — asks that
one predicate rather than inventing a second notion of "unnamed". Only one
automatic rename can ever happen to a task.

> `uze-core::project::task::naming_tests::only_a_branch_outside_uzes_namespace_reads_as_named`
> `uze-application::…::naming_tests::a_second_name_is_refused_and_the_first_one_stands`

### The checkout's HEAD is the truth about a task's branch

`task.branch` is a cache of a Git fact, re-read on every evaluation. A branch
renamed outside UZE reaches the sidebar, delivery and sync — and, more
importantly, never leaves UZE asking Git about a ref that no longer exists,
where `commits_ahead` answers `0` and a task with work reads as having none.

> `uze-application::…::naming_tests::a_branch_renamed_by_hand_is_adopted_and_still_reaches_ready`

### Drift along the environment chain is reported, never applied on its own

Declared (`agents.yaml`) → locked (`agents.lock`) → installed (the Store) →
delivered (the projected region) is compared from the manifest down, and the
comparison is two file reads and a set difference — no acquisition, no
network. Converging a removal edits the lock and nothing on the machine:
the Store keeps the package and every harness keeps reading it, because
other projects share both and machine scope is `uze plugin remove`'s.

> `tests/workspace/consumer.rs::drift::install_converges_the_lock_and_leaves_the_machine_alone`
> `tests/workspace/consumer.rs::drift::a_policy_change_reads_as_a_stale_projection_until_install_clears_it`


## Input (M6)

### A keystroke reaches an action through the keymap, and nothing else reads a key

`uze-keys` names every action, says where each is live, and answers both
directions: what a keystroke means, and what key reaches a meaning. One
adapter (`src/ui/keys.rs`) speaks the terminal's dialect — the mirror of
`src/ui/theme.rs` for colour — and the PTY encoder is sanctioned by name
because translating a keystroke into bytes for a pane is not binding it.

> `tests/architecture/layering.rs::architecture_rules_hold` (rule: *one module names a physical key*)

### Every key uze prints came from the keymap

No rendering module contains a chord written as a literal. A help line, a
footer hint and a menu entry all ask `chord_for`, so a rebound key is right
everywhere at once and an invented one cannot be printed at all. The
hand-typed list this replaced had already fallen out of step with the
dispatcher for nine of its bindings.

> `tests/architecture/layering.rs::architecture_rules_hold` (rules: *no surface writes a key down*, *no surface draws its own arrow keys*)
> `src/ui/tests.rs::a_hint_line_reads_its_keys_off_the_keymap`

### Every chord has something to click; not every control needs a chord

An action bound outside the pane names the control that performs it, or is
declared keyboard-only with a written reason. The converse is deliberately
not required: "new space" is a button and an index entry with no key, and
that is a finished design. This is what keeps the keyboard an accelerator
rather than the way in.

> `tests/architecture/affordances.rs::every_action_says_where_its_pointer_lands`
> `tests/architecture/affordances.rs::a_bound_action_is_never_reachable_by_keyboard_alone_without_a_reason`

### Modality is a value, not an order of match arms

What is open is a stack of scopes; the innermost answers first, a sealing
surface answers for everything but `global`, and the pane is last and
total. This replaced two hand-ordered `match` guards — one of which was
wrong, firing three chords through an open overlay while five others were
correctly swallowed.

> `uze-keys::keymap::tests::a_sealed_surface_answers_for_everything_except_global`
> `uze-keys::keymap::tests::the_pane_receives_anything_nothing_claims`

### A key means one thing per keyboard

Two actions may not share a mnemonic — a letter, a digit, a function key —
within management or within the workspace. Structural keys stay
contextual. A keymap that breaks this is refused rather than resolved by
order, which is what retired `r` meaning both *remove* and *refresh*.

> `uze-keys::keymap::tests::one_chord_names_one_action_within_a_keyboard`
> `src/ui/tests.rs::a_letter_names_one_action_and_refreshing_has_its_own`

### A keymap that cannot be used never takes the keyboard away

A chord that is another key on a terminal, or one that conflicts, is an
error and the keymap already in force stays in force. A chord this
terminal cannot send is never bound. An entry naming something this build
does not know is a warning, so a keymap written for a newer uze still
loads.

> `uze-keys::load::tests::a_file_that_cannot_be_used_leaves_the_keyboard_alone`
> `uze-keys::load::tests::a_keymap_written_for_a_newer_uze_still_loads`
> `src/keymap.rs::tests::a_file_that_would_take_the_keyboard_away_says_so_and_changes_nothing`
> `src/ui/tests.rs::a_key_this_terminal_cannot_send_is_never_bound`

### What can be done to a thing is answered once

An entity's offers come from the application layer, and the detail view's
buttons and the index both read that one list. A detail view draws a button
for what can be done now and for nothing else.

> `uze-application::application::offers::tests::an_action_that_cannot_run_says_why_rather_than_doing_nothing`
> `src/ui/tests.rs::every_drawer_draws_what_its_row_can_do_as_buttons`
> `src/ui/tests.rs::the_drawer_offers_what_can_be_done_as_buttons`

---

## This page

### Every citation on this page names a test that exists

Each entry above is tied to the test that proves it, and a tie nothing
checks is one that quietly comes undone: a refactor moves a file, the
property still reads as guarded, and nothing guards it. Every
`` `<path>.rs::<name>` `` here is resolved against the tree, so a moved or
deleted test surfaces on the change that moved it — the same discipline
`journey validate` enforces for a journey's `proves:` one tier up.

Structural only, deliberately. It cannot tell you a property drifted away
from the test that still bears its name, and nothing can.

> `tests/architecture/invariants_citations.rs::every_invariant_cites_a_test_that_exists`
