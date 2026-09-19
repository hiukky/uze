//! An edge rule: one hairline along one side of a rect, and the rect
//! left over.
//!
//! What separates a sidebar from the column beside it, a footer from the
//! rows above it, a cell from its neighbour. A [`Surface`](super::Surface)
//! with a single border would draw the same cells, but it would say the
//! wrong thing: a surface encloses its content and a rule divides two
//! regions that are not inside anything.

use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Padding},
};
use uze_theme::Token;

use crate::ui::theme;

/// Which side the hairline runs along — the side the rule *is*, not the
/// side the content is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    fn borders(self) -> Borders {
        match self {
            Self::Left => Borders::LEFT,
            Self::Right => Borders::RIGHT,
            Self::Top => Borders::TOP,
            Self::Bottom => Borders::BOTTOM,
        }
    }
}

/// A hairline along one edge. Drawn once with [`Rule::render`], which
/// answers with the rect beside it.
#[derive(Clone, Debug)]
pub(crate) struct Rule {
    edge: Edge,
    tone: Token,
    ground: Option<Token>,
    padding: Padding,
}

impl Rule {
    /// A divider between two regions, in the fainter of the two hairlines:
    /// a rule inside a screen separates, and separating does not need the
    /// weight that enclosing does.
    pub(crate) fn new(edge: Edge) -> Self {
        Self {
            edge,
            tone: Token::BorderFaint,
            ground: None,
            padding: Padding::ZERO,
        }
    }

    /// A divider the pointer can grab. It goes accent while it is being
    /// dragged, which is the one piece of feedback saying the drag took —
    /// the panel itself does not move until the mouse does.
    pub(crate) fn draggable(edge: Edge, dragging: bool) -> Self {
        Self::new(edge).tone(if dragging {
            Token::Accent
        } else {
            Token::BorderFaint
        })
    }

    pub(crate) fn tone(mut self, tone: Token) -> Self {
        self.tone = tone;
        self
    }

    /// The ground to fill beside the rule, when the region it divides off
    /// carries one of its own.
    pub(crate) fn ground(mut self, ground: Token) -> Self {
        self.ground = Some(ground);
        self
    }

    /// Room between the rule and what sits beside it. Worth a comment
    /// wherever it is not zero: two panes whose dividers pad differently
    /// drift out of alignment by a row or a column, and that reads as a
    /// rendering fault rather than as a choice.
    pub(crate) fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }

    /// Draws into `area` and answers with the rect beside the hairline.
    pub(crate) fn render(self, frame: &mut ratatui::Frame<'_>, area: Rect) -> Rect {
        let block = self.block();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    }

    fn block(self) -> Block<'static> {
        let mut block = Block::default()
            .borders(self.edge.borders())
            .border_style(theme::fg(self.tone))
            .padding(self.padding);
        if let Some(ground) = self.ground {
            block = block.style(theme::bg(ground));
        }
        block
    }
}
