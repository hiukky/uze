//! A text input: what has been typed, where typing would land, and the
//! hint standing in while nothing has been.
//!
//! # The empty focused field
//!
//! The state that decides whether something reads as an input is the one
//! with nothing in it. A field holding text is obvious; a field holding
//! nothing is a caption unless something says otherwise, and the thing
//! that says otherwise is the caret.
//!
//! Four inputs answered that differently. The picker's query row and the
//! rename buffers showed a caret from the first frame. The list screens'
//! filter showed none, but drew an underline, so its extent said "field"
//! even empty. The open index showed neither — no underline, no caret,
//! just muted words — and was found to be typable only by someone typing
//! into it.
//!
//! So the rule is here rather than at four call sites: **a focused field
//! always shows its caret**, before the placeholder when empty and after
//! the text when not. What a caller still chooses is the field's *lead* —
//! an underline, a `❯`, a gutter that lines it up with the rows below —
//! because that is about where the field sits, not about what it is.
//!
//! # One caret
//!
//! [`Symbol::CursorText`] is documented in the theme as "the caret in a
//! text input"; [`Symbol::BarThin`] is one of the bars that mark the edge
//! of a row. Three of the seven inputs drew the bar. They are not
//! interchangeable — a glyph set may spell them differently, and the
//! reason to name a symbol at all is that the theme decides what it looks
//! like.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use uze_theme::Token;

use super::{Edge, Rule};
use crate::ui::theme::{self, Symbol};

/// The caret: where typing would land.
pub(crate) fn caret() -> Span<'static> {
    Span::styled(theme::glyph(Symbol::CursorText), theme::fg(Token::Accent))
}

/// A text input's content, as spans a caller places into its own row.
#[derive(Clone, Debug)]
pub(crate) struct Field<'a> {
    text: &'a str,
    placeholder: &'a str,
    focused: bool,
    ink: Token,
}

impl<'a> Field<'a> {
    /// A field showing `text`, or `placeholder` while nothing is typed.
    ///
    /// An empty placeholder is fine: a field that is always focused —
    /// a modal's own — has nothing to say while empty but its caret.
    pub(crate) fn new(text: &'a str, placeholder: &'a str) -> Self {
        Self {
            text,
            placeholder,
            focused: true,
            ink: Token::TextPrimary,
        }
    }

    /// Whether keys reach this field right now. An unfocused field shows
    /// no caret — two carets on one screen is two claims about where
    /// typing goes.
    pub(crate) fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// The typed text's colour, where the field is the thing the surface
    /// is about rather than one control among several.
    pub(crate) fn ink(mut self, ink: Token) -> Self {
        self.ink = ink;
        self
    }

    pub(crate) fn spans(&self) -> Vec<Span<'static>> {
        let mut spans = Vec::with_capacity(3);
        if self.text.is_empty() {
            if self.focused {
                spans.push(caret());
            }
            if !self.placeholder.is_empty() {
                spans.push(Span::styled(
                    self.placeholder.to_owned(),
                    theme::fg(Token::TextMuted),
                ));
            }
        } else {
            spans.push(Span::styled(self.text.to_owned(), theme::fg(self.ink)));
            if self.focused {
                spans.push(caret());
            }
        }
        spans
    }

    pub(crate) fn line(&self) -> Line<'static> {
        Line::from(self.spans())
    }

    /// Draws the field into `area` under its own rule, and answers with
    /// the row the content took.
    ///
    /// The rule *is* the field: it says where the field extends, which a
    /// caret alone cannot, and it goes accent while the field has focus,
    /// which is the only thing on screen saying keys land here rather than
    /// somewhere else. Every input the product has wears it, so that
    /// recognising one is a thing the reader learns once.
    ///
    /// A caller with a lead of its own — a `❯`, a gutter aligning the
    /// field with the rows under it — builds its row from
    /// [`spans`](Self::spans) instead and keeps its own chrome.
    pub(crate) fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect) -> Rect {
        let inner = Rule::new(Edge::Bottom)
            .tone(if self.focused {
                Token::Accent
            } else {
                Token::BorderDefault
            })
            .render(frame, area);
        frame.render_widget(Paragraph::new(self.line()), inner);
        inner
    }
}
