//! How a stored capability reaches a harness.
//!
//! The half of the product that the package model exists to serve.
//! [`integration`] is the contract a harness vertical implements;
//! [`router`] decides which mechanism a capability is compatible with and
//! [`exposure`] plans the concrete artifacts; [`engine`] carries the plan
//! out; [`state`] and [`persistence`] record what was written, as typed
//! receipts; [`reconciliation`] compares that record against what is
//! actually on disk, which is what makes removal safe.
//!
//! Vendor-neutral throughout — no module here names a harness. The concrete
//! verticals live in `uze-integrations`, behind
//! [`integration::IntegrationPort`].

pub mod engine;
pub mod exposure;
pub mod integration;
pub mod persistence;
pub mod reconciliation;
pub mod router;
pub mod state;
