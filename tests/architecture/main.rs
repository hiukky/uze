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
//! The debt these rules were built to count is now zero: no presentation
//! file reaches the domain. What remains is `sanctioned`, and it is not
//! debt — `src/shim.rs` and `src/bin/uze-harness-matrix.rs` are separate
//! binary entry points sharing this crate, and both legitimately name the
//! domain.
//!
//! That is also why the binary cannot simply drop its `uze-core`
//! dependency and let `rustc` enforce this instead: those two files need
//! it. Making the compiler the enforcer would mean moving them into crates
//! of their own, which is a decision about what the `uze` binary *is*,
//! not a tidy-up. Until someone takes it, this file is the enforcement.

mod layering;
