//! The small glyphs that annotate a line.
//!
//! Each is one span or one string, which is exactly why they drift. A
//! glyph set may spell any of them differently — that is what a glyph set
//! *is* — and a copy written out by hand keeps whatever it was written
//! with. Four files spelled the disclosure chevron themselves, two of them
//! with the predicate inverted, and nothing said they were the same mark.

use crate::ui::theme::{self, Symbol};

/// The chevron that says whether a group is open.
///
/// It takes `expanded` rather than `collapsed` because half its callers
/// already hold the positive question — a profile is *selected*, a drawer
/// is *open* — and a marker that had to be asked the negative one is how
/// two of them ended up writing the branches in the opposite order.
pub(crate) fn disclosure(expanded: bool) -> String {
    theme::glyph(if expanded {
        Symbol::ChevronExpanded
    } else {
        Symbol::ChevronCollapsed
    })
}
