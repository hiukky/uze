//! What more than one extension needs.
//!
//! The rule for what lands here is the same one [`crate::view`] states for
//! its own vocabulary: one extension wanting something is a special case,
//! two are evidence. Everything in this module is here because a second
//! extension reached for it — nothing is put here in anticipation.
//!
//! It is deliberately *not* a home for anything an extension could reach
//! on its own. A capability still arrives through [`crate::Host`]; this
//! is only shared computation over what the host already handed over.

pub mod highlight;
