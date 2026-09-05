//! Harness-agnostic UZE domain, persistence, planning, and integration
//! contracts. Concrete vendor integrations and presentation layers depend on
//! this crate; it never depends on them.
//!
//! # How to read this crate
//!
//! Five concerns, each with its own module documenting what belongs in it.
//! Read the concern before the module: the answer to "what kind of thing is
//! `hook`?" is that it sits under [`capability`], which is a different
//! question from where its file happens to be.
//!
//! | concern | what it owns |
//! |---|---|
//! | [`package`] | where a package's bytes come from, and where they live |
//! | [`capability`] | what a plugin declares, portably |
//! | [`delivery`] | how a capability reaches a harness |
//! | [`project`] | what a project declares, and what UZE writes into it |
//! | [`machine`] | the local environment outside UZE's own state |
//!
//! The public paths below are flat and unchanged — `uze_core::store`, not
//! `uze_core::package::store`. The grouping says how the crate reads from
//! the inside; renaming 581 call sites across the workspace would have
//! bought nothing but a diff nobody can review. Removing a re-export later
//! lets the compiler drive that migration one module at a time, if it ever
//! earns its keep.

pub mod capability;
pub mod delivery;
pub mod machine;
pub mod package;
pub mod project;

/// Content identity, shared by everything that needs a stable name for
/// some bytes — a managed region, a store entry, a project cache
/// directory. Deliberately not under a concern: it is a primitive, and
/// filing it under one of them would imply the others should not use it.
pub mod digest;
pub mod error;

/// Universal user preferences and their per-harness translation. A product
/// feature (Profiles) rather than part of the portable model — kept at the
/// root, visibly, rather than filed under a concern it does not belong to.
pub mod preference;
pub mod profile_state;

/// Per-workspace prompt log for agent tabs. Borderline: only the TUI reads
/// it, which argued for evicting it — but it is UZE-owned state under
/// `UzeHome`, like [`profile_state`], and the crate that would host it
/// (`uze-terminal`) depends on nothing here today, which is a property
/// worth more than this module's tidiness. Left at the root, named, rather
/// than filed under a concern it does not belong to.
pub mod prompt_history;

/// What the workspace client's sidebar keeps between runs. Here for the
/// same reason [`prompt_history`] is: UZE-owned state under `UzeHome`,
/// read by the one client that draws it.
pub mod sidebar_layout;

// Flat public API. Each line also says which concern the module belongs to,
// which is the second reason for keeping them: the crate root is where a
// reader looks first.
pub use capability::{hook, skill};
pub use delivery::{engine, exposure, integration, persistence, reconciliation, router, state};
pub use machine::{detection_cache, harness_runtime, home, provisioning, shell_path, subprocess};
pub use package::{acquisition, bundle, importer, importers, naming, store, trust};
pub use project::{
    checkout, context, landing, project_context, project_lock, project_root, task, text_region,
    workspace, worktree,
};

pub use acquisition::{MaterializedPackage, PackageSource, Provenance, ResolvedSource};
pub use engine::UzeEngine;
pub use error::{ProjectionConflictDetails, Result, UzeError};
pub use exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan, PreparedExposure};
pub use home::UzeHome;
pub use project::{
    EffectiveEnvironment, ResolvedProject, Resource, ResourceOrigin, resolve_project,
    resolve_project_resources,
};
pub use skill::SkillInvocationPolicy;
pub use store::{PackageId, StoredPackage, UzeStore};
