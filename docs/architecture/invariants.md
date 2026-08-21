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

Proven by adding a fourth harness with a materially different native delivery:
Codex needs a published catalogue, Gemini CLI needs none, and both go through
the same `IntegrationPort`.

> `tests/vendor_neutral_core.rs::the_store_contains_no_source_mechanism_semantics`
> `tests/vendor_neutral_core.rs::no_core_module_depends_on_acquisition`

### Package publication and package-native delivery are independent

`republish_packages` maintains a derived view; `attach_package` performs a
native delivery. Codex uses both. Gemini uses only the second — its
`republish_packages` is never overridden.

> `tests/vendor_neutral_core.rs::a_derived_view_is_rebuilt_from_the_package_set_alone`
> `tests/vendor_neutral_core.rs::republish_is_a_noop_for_an_integration_that_publishes_nothing`

### A failed derived view never invalidates an installation

One harness failing to publish leaves the package installed, the other
harnesses untouched, and the failure observable through `doctor`.

> `tests/vendor_neutral_core.rs::a_failed_publication_leaves_the_package_installed_and_says_so`

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

## Decisions deliberately *not* taken

Recorded because absence is a decision, and because each one has been proposed
and set aside for a reason rather than forgotten.

| Not built | Why not, for now |
|---|---|
| marketplace | UZE consumes packages; publishing is a different product |
| remote registry | nothing to serve until acquisition is dogfooded |
| cache | correctness first; a cache is an optimization with a second-source-of-truth risk |
| content-addressable store | a Git commit is already a content identity, and a local path has no reproducibility to protect |
| lockfile | meaningful only with dependencies between packages, which do not exist |
| GitHub-specific source | a host is a URL; the mechanism is Git |
| automatic update | requires update semantics to be exercised by hand first |
| persistent trust database | one consent boundary, not a permission system |
| network-dependent gating test | local bare repositories prove the mechanism; a public repo would test that provider's availability |
