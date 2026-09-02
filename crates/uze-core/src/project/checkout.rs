//! Isolated checkouts as slots: long-lived working trees under the primary
//! checkout, each named by an identifier that never changes, reused by one
//! task after another.
//!
//! This file carries the identity only, so the task model can refer to a
//! slot before the slot mechanics exist; acquisition, reuse, parking and
//! adoption arrive with the checkout work itself.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The generated, immutable name of a slot — the directory under the
/// isolation directory. Never derived from a task or a label, so a slot
/// outlives every task that runs in it.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CheckoutId(String);

impl CheckoutId {
    pub fn generate() -> Self {
        Self(crate::task::generated_identifier(b"checkout"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CheckoutId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
