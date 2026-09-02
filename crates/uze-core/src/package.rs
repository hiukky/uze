//! Where a package's bytes come from, and where they live once they are
//! UZE's.
//!
//! The original core of the product, and still its foundation: [`acquisition`]
//! resolves a source into materialized bytes, [`trust`] is the one consent
//! boundary that resolution made necessary, [`importer`]/[`importers`]
//! recognize the standard layout inside them, [`bundle`] is what that
//! recognition produces, [`naming`] is the collision boundary an ingest must
//! pass, and [`store`] owns the installed bytes as the single source of
//! truth.
//!
//! Nothing here knows what a harness is. Turning a stored package into
//! something a harness can use is `crate::delivery`.

pub mod acquisition;
pub mod bundle;
pub mod importer;
pub mod importers;
pub mod naming;
pub mod store;
pub mod trust;
