//! Where a package's bytes come from, and where they live once they are
//! UZE's.
//!
//! The original core of the product, and still its foundation: [`acquisition`]
//! resolves a source into materialized bytes, [`trust`] is the one consent
//! boundary that resolution made necessary, [`naming`] is the collision
//! boundary an ingest must pass, and [`store`] reads a package's
//! `plugin.json` and owns the installed bytes as the single source of truth.
//! [`hosts`] is the machine's table of host aliases, which turns what a
//! person types into the URL acquisition reads.
//!
//! Nothing here knows what a harness is. Turning a stored package into
//! something a harness can use is `crate::delivery`.

pub mod acquisition;
pub mod hosts;
pub mod naming;
pub mod store;
pub mod trust;
