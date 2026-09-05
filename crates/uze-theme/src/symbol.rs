//! The glyph vocabulary — an icon set, referenced by name.
//!
//! Every mark UZE draws as chrome is named here and resolved from the active
//! theme, for the same reason colours are: a glyph written inline is a glyph
//! nobody can change. A theme replaces any of them, which is what makes a
//! pure-ASCII UZE possible on a terminal with no Unicode font.
//!
//! A symbol carries its display width alongside its glyph. Today the widths
//! are implicit in the literals — several call sites lay a column out from
//! the length of the string they typed — so swapping a glyph for a wider one
//! would quietly shear every row that contains it. Resolving the width with
//! the glyph is what makes a replacement safe, and letting a theme override
//! it is the only thing that can be right for a font whose private-use
//! glyphs lie about their own width.

use unicode_width::UnicodeWidthStr;

use crate::vocab::vocabulary;

/// A resolved symbol: what to draw, and how many cells it occupies.
///
/// The frames are the animation. Most symbols have exactly one; a spinner
/// has as many as the theme gave it, so replacing an animation replaces it
/// coherently rather than as ten unrelated entries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolDef {
    frames: Vec<String>,
    width: u16,
}

impl SymbolDef {
    /// A still symbol, its width measured from the glyph.
    pub fn new(glyph: impl Into<String>) -> Self {
        let glyph = glyph.into();
        let width = glyph.width() as u16;
        Self {
            frames: vec![glyph],
            width,
        }
    }

    /// An animated symbol. Its width is the widest frame, so a row laid out
    /// from it does not change size as the animation runs.
    pub fn animated(frames: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let frames: Vec<String> = frames.into_iter().map(Into::into).collect();
        let width = frames
            .iter()
            .map(|frame| frame.width() as u16)
            .max()
            .unwrap_or(0);
        Self { frames, width }
    }

    /// Overrides the measured width. For a font whose glyph occupies a
    /// different number of cells than Unicode says it should.
    pub fn with_width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// What to draw when the symbol does not animate, or the animation's
    /// first frame.
    pub fn glyph(&self) -> &str {
        &self.frames[0]
    }

    /// The frame for `tick`, wrapping. A still symbol answers with its one
    /// glyph for every tick.
    pub fn frame(&self, tick: usize) -> &str {
        &self.frames[tick % self.frames.len()]
    }

    /// Cells this symbol occupies. Lay columns out from this, never from
    /// the length of the glyph at the call site.
    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn frames(&self) -> &[String] {
        &self.frames
    }
}

vocabulary! {
    /// A named glyph. Resolved against the active [`Theme`](crate::Theme).
    pub enum Symbol {
        // ── marks: what a thing's standing is ──────────────────────────
        /// A capability delivered through the harness's own mechanism.
        MarkNative = "mark.native",
        /// A package or marketplace UZE vouches for.
        MarkOfficial = "mark.official",
        /// Delivered, but not through the harness's own mechanism.
        MarkAdapted = "mark.adapted",
        /// No route exists at all.
        MarkUnsupported = "mark.unsupported",
        /// Needs attention but is not broken. A pictograph by default, so it
        /// belongs in a label that has room for one — never in a
        /// one-column status mark, where terminals draw the emoji family at
        /// a width that varies and a glyph that ignores the hue carrying
        /// the meaning. Use [`MarkAttention`](Self::MarkAttention) there.
        MarkWarning = "mark.warning",
        /// The one-column form of "this needs you": a paused rebase, a
        /// capability the environment is shadowing, an alert worth reading.
        MarkAttention = "mark.attention",
        /// Dismisses what it sits on.
        MarkClose = "mark.close",
        /// A list bullet in running text.
        MarkDot = "mark.dot",
        /// Multiplication/removal in a count or a label, not a button.
        MarkCross = "mark.cross",
        /// Something new is created here.
        MarkSparkle = "mark.sparkle",
        /// Selectable, currently off.
        MarkToggleOff = "mark.toggle-off",
        /// Selectable, currently on.
        MarkToggleOn = "mark.toggle-on",

        // ── an agent's standing in the sidebar ─────────────────────────
        /// Producing output right now. The one animated symbol.
        StatusWorking = "status.working",
        /// Finished while the user was looking elsewhere.
        StatusCompleted = "status.completed",
        /// The tab the user is on, with nothing in flight.
        StatusSelected = "status.selected",
        /// A quiet tab the user is not on.
        StatusIdle = "status.idle",

        // ── structure ──────────────────────────────────────────────────
        /// A tree row with siblings below it.
        TreeBranch = "tree.branch",
        /// The last tree row in its group.
        TreeLast = "tree.last",
        /// The line a tree's children hang from.
        TreeVertical = "tree.vertical",
        /// A horizontal rule.
        TreeDivider = "tree.divider",
        /// The vertical rule between two columns.
        TreeColumnDivider = "tree.column-divider",

        // ── bars: a filled edge marking where you are ──────────────────
        BarThick = "bar.thick",
        BarMedium = "bar.medium",
        BarThin = "bar.thin",
        /// The caret in a text input.
        CursorText = "cursor.text",

        // ── direction and affordance ───────────────────────────────────
        ArrowUp = "arrow.up",
        ArrowDown = "arrow.down",
        /// Leads somewhere outside UZE.
        ArrowExternal = "arrow.external",
        /// Two things exchange places.
        ArrowSwap = "arrow.swap",
        /// The shift key, in a hint line.
        ArrowShift = "arrow.shift",
        /// Commits ahead of the upstream.
        ArrowAhead = "arrow.ahead",
        /// Commits behind the upstream.
        ArrowBehind = "arrow.behind",
        /// Points from a thing to where it is going — a delivery's target,
        /// a mapping's right-hand side.
        ArrowTo = "arrow.to",
        /// Points at the item under discussion.
        ChevronRight = "chevron.right",
        /// A section that is folded shut.
        ChevronCollapsed = "chevron.collapsed",
        /// A section that is open.
        ChevronExpanded = "chevron.expanded",
        /// The prompt caret before an input.
        Prompt = "prompt",
        /// Opens a menu.
        Menu = "menu",

        // ── typography ─────────────────────────────────────────────────
        /// Elided text.
        Ellipsis = "text.ellipsis",
        /// The dash that introduces an aside.
        EmDash = "text.em-dash",
        /// What separates the clauses of a hint line.
        HintSeparator = "text.separator",
        /// A value that is present but not a number, or a range.
        PlusMinus = "text.plus-minus",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_symbol_answers_the_same_glyph_for_every_tick() {
        let symbol = SymbolDef::new("●");
        assert_eq!(symbol.glyph(), "●");
        assert_eq!(symbol.frame(0), "●");
        assert_eq!(symbol.frame(97), "●");
        assert_eq!(symbol.width(), 1);
    }

    #[test]
    fn an_animation_wraps_and_keeps_one_width() {
        let symbol = SymbolDef::animated(["⠋", "⠙", "⠹"]);
        assert_eq!(symbol.frame(0), "⠋");
        assert_eq!(symbol.frame(2), "⠹");
        assert_eq!(symbol.frame(3), "⠋");
        assert_eq!(symbol.width(), 1);
    }

    #[test]
    fn width_comes_from_the_glyph_and_can_be_overridden() {
        assert_eq!(SymbolDef::new("├─").width(), 2);
        // A private-use glyph Unicode reports as narrow but a Nerd Font
        // draws double-wide: the only thing that can be right here is what
        // the theme's author says.
        assert_eq!(SymbolDef::new("\u{e0a0}").with_width(2).width(), 2);
    }
}
