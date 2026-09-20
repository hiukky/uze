//! What more than one extension needs.
//!
//! Created the day a second one actually reached for it, and no earlier:
//! a module named for sharing invites everything into it, and what one
//! extension happens to have written is not yet a shared concern.
//!
//! [`canvas`] is here because a drawing in cells stopped being the
//! architect's alone — the code surface's map is drawn the same way,
//! out of the same rectangles, borders and glyph sets. What it holds is
//! *how to put a glyph somewhere*, and nothing about what is being
//! drawn: no node, no edge, no tile. That is the line to hold. A helper
//! that knows what it is drawing belongs to the extension that knows.
//!
//! [`checkout`] joined it the day a second surface opened on a checkout
//! and had to name it in its title. What it holds is what a title says
//! about a checkout — its branch, and the three weights that sentence is
//! told in — because the reader compares those sentences across
//! surfaces, and two copies of one sentence stop being one sentence.

pub mod canvas;
pub mod checkout;
