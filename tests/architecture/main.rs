//! Architecture rules, enforced deterministically.
//!
//! The dependency direction this project documents is only a fact if
//! something checks it. These are structural scans over production source,
//! in the same shape as the vendor-neutrality scans in
//! `tests/integrations/identity.rs` — no external tooling, no annotations to
//! forget, and a failure that names the rule, the reason, and what to do.
//!
//! Rules are data rather than hand-written assertions so the set is
//! readable as a list of what this codebase does not allow. Each carries
//! two lists that mean opposite things: `sanctioned` is architecture and
//! should never be "fixed", while `budget` is debt with a number on it.
//!
//! The end state of the layering rules is not a passing test. It is the
//! deletion of `uze-core` from the binary's `[dependencies]`, after which
//! `rustc` enforces them and this file is redundant. Until then it reports
//! how far away that is.

mod layering;
