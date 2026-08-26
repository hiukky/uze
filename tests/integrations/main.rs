//! Integration seam tests: vendor-neutral contracts, per-harness capability
//! and lifecycle conformance, the runtime-shim boundary, the invocation
//! policy carrier used by the per-harness modules, and the structural
//! vendor-neutrality scans.

mod agents;
mod capability_conformance;
mod contract;
mod hooks;
mod identity;
mod lifecycle_conformance;
mod policy;
mod runtime_boundary;
mod vendor_neutral;

pub(crate) mod harness {
    //! Per-harness semantic conformance (invocation policy routing and
    //! wrapper materialization) — split from the former
    //! `tests/skill_invocation_conformance.rs`.

    pub mod antigravity;
    pub mod claude;
    pub mod codex;
    pub mod opencode;
}
