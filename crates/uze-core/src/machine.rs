//! The local environment UZE touches outside its own state.
//!
//! Infrastructure rather than domain: [`home`] owns UZE's paths,
//! [`detection_cache`] remembers which harnesses are installed,
//! [`provisioning`] and [`subprocess`] are the discipline for running
//! something, [`shell_path`] is the reversible `PATH` integration,
//! [`harness_runtime`] is the experimental PATH shim, and [`features`]
//! says which unfinished surfaces this build offers.
//!
//! A module belongs here when it is about *this machine* — not about a
//! package, a capability, or a project.

pub mod detection_cache;
pub mod features;
pub mod harness_runtime;
pub mod home;
pub mod provisioning;
pub mod shell_path;
pub mod subprocess;
