//! TUI — the chrome vocabulary: the surfaces, rules and controls every
//! screen is assembled from.
//!
//! # What belongs here
//!
//! A widget names what a drawn thing *is* — a surface, an edge rule, a
//! button — the way a [`Token`](uze_theme::Token) names what a colour is.
//! `theme` already decided that nothing outside it may say *what a thing
//! looks like*; this module is the other half, so that nothing outside it
//! says what a thing is *made of* either. A caller asks for a surface and
//! is handed the rect inside it; the border style, the ground and the
//! padding are not its business, and it cannot drift from the screen next
//! door by choosing them differently.
//!
//! That drift is why this exists rather than being a tidy-up. Before it,
//! nine full-bordered boxes were built by hand across six files: one wore
//! [`Token::BorderFaint`] where the other eight wore
//! [`Token::BorderDefault`], one grounded itself with `theme::on` where the
//! rest used `theme::bg`, and their paddings — `(1,1,1,0)`,
//! `horizontal(2)`, `(2,2,1,1)`, `(2,2,0,0)`, none — agreed on nothing. Not
//! one of those differences was a decision anybody made.
//!
//! # Primitives and composites
//!
//! Most of this is primitives — a surface, a rule, a chip, a row's ground.
//! [`action_index`] is not: it is a whole overlay, assembled from them. It
//! earns its place on the same evidence, which is that both clients drew
//! it and until now both *built* it, and the two builds had already
//! drifted where a reader could not see.
//!
//! A composite answers to the same rules as a primitive — it takes its
//! rows as data, is generic over the caller's hit type, and knows nothing
//! about either client's model. Anything that cannot meet those is a
//! screen, and belongs with its screen.
//!
//! # When to move something here
//!
//! On the second *file*, not the second call. A helper three callers in
//! one screen share is that screen's own, and hoisting it here only makes
//! the vocabulary longer to read. The signal that something has outgrown
//! its home is a screen *exporting* it: `clip_line` sat in
//! `management.rs`, and four other screens reached across for it as
//! `super::super::management::clip_line`.
//!
//! # What does not belong here
//!
//! **Hit-testing.** A widget never owns what clicking it means. [`Hit`] is
//! a flat enum of the app's own targets, and a button is drawn in a dozen
//! places to say a dozen different things — so a caller passes the [`Hit`]
//! in and gets the rects back to register. A widget that chose its own
//! would have to know the screen it is on, which is exactly the coupling
//! this module exists to remove.
//!
//! **State.** ratatui is immediate mode: the frame is redrawn whole, every
//! time. There is no identity to reconcile between frames and so nothing
//! for a widget to remember — which is why these are builders that draw
//! and return, not objects that live. Selection, focus and hover are the
//! model's, and arrive as arguments.
//!
//! **Layout.** A widget fills the rect it is handed. Deciding *which* rect
//! is the screen's own job, because only the screen knows what it is
//! arranging.
//!
//! # Where it may live
//!
//! Here, and not in a crate. `ratatui` appears in exactly one manifest —
//! the root's — and that is deliberate: [`uze_theme`] resolves no path and
//! names no rendering library, and `uze_extensions` depends on no crate in
//! the workspace at all. A widget crate would have to name ratatui, which
//! would either promote it to a public contract of the workspace or force
//! the adaptation to be written twice. `src/ui/` already owns ratatui, so
//! what draws belongs here.

/// The gap a row keeps between its right-most content and the divider (or
/// the frame) beside it. One place, because a row that reserves a different
/// amount than the row above it reads as ragged rather than as deliberate.
pub(crate) const TRAILING_PAD: u16 = 1;

/// The inset every anchored popup keeps between its border and its content.
/// Four popups had grown their own copy of this pair; they were all the same
/// number, which is the point — a popup that pads differently reads as a
/// different kind of surface.
pub(crate) const POPUP_H_PAD: u16 = 2;
pub(crate) const POPUP_V_PAD: u16 = 1;

pub(crate) mod action_index;
pub(crate) mod button;
pub(crate) mod chip;
pub(crate) mod field;
pub(crate) mod hint;
pub(crate) mod mark;
pub(crate) mod row;
pub(crate) mod rule;
pub(crate) mod screen_header;
pub(crate) mod scrim;
pub(crate) mod scrollbar;
pub(crate) mod surface;
pub(crate) mod text;

pub(crate) use button::{Align, Button, button_row};
pub(crate) use chip::{Chip, ChipState};
pub(crate) use field::Field;
pub(crate) use row::RowState;
pub(crate) use rule::{Edge, Rule};
pub(crate) use scrollbar::Scrollbar;
pub(crate) use surface::{Surface, fill, root};

#[cfg(test)]
mod tests;
