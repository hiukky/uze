//! # What lives under `project/`
//!
//! Everything scoped to a *project directory* rather than to the machine:
//! what a project declares ([`manifest`]) and what resolving it produced
//! ([`project_lock`], [`worktree`]), where that
//! project begins ([`project_root`], [`workspace`]), the instruction
//! context it carries ([`context`], [`project_context`]), the mechanism
//! for owning a slice of a file UZE did not write ([`text_region`]), and
//! the work UZE runs inside it: the agents it launched ([`task`]),
//! the checkouts the isolated ones run in ([`checkout`]), the
//! conversation each agent is in ([`conversation`]), and how an isolated
//! agent's work reaches the target ([`landing`]).
//!
//! Module file names keep their full public spelling — `project/lock.rs`
//! would read better in the tree but would no longer match
//! `uze_core::project_lock`, and a name that changes between the inside and
//! the outside costs more than the prefix saves.
pub mod checkout;
pub mod context;
pub mod conversation;
pub mod landing;
pub mod manifest;
pub mod project_context;
pub mod project_lock;
pub mod project_root;
pub mod task;
pub mod text_region;
pub mod workspace;
pub mod worktree;
