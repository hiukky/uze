//! TUI view — shared helpers used by every route's render function.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ui::hit::Hit;
use crate::ui::model::{ResizablePanel, TuiModel};
use crate::ui::side_panel_area;
use crate::ui::theme::{self, Symbol, Token};
use uze_application::application::offers::ActionOffer;

pub mod appearance;
pub mod extensions;
pub mod harnesses;
pub mod health;
pub mod keys;
pub mod overview;
pub mod plugins;
pub mod profiles;

pub(crate) const DRAWER_DEFAULT_WIDTH: u16 = 52;
/// The narrowest a drawer, or the list beside it, is ever drawn.
const DRAWER_MIN_WIDTH: u16 = 24;

/// How wide `panel`'s drawer is drawn over `content`: where it was dragged
/// to, or the default, never squeezing itself or the list beside it past
/// the minimum.
pub(crate) fn drawer_width(panel: ResizablePanel, model: &TuiModel, content: Rect) -> u16 {
    panel.width(model).unwrap_or(DRAWER_DEFAULT_WIDTH).clamp(
        DRAWER_MIN_WIDTH,
        content
            .width
            .saturating_sub(DRAWER_MIN_WIDTH)
            .max(DRAWER_MIN_WIDTH),
    )
}

/// A detail drawer's shell off the right of `content`: a recessed slab
/// behind a left rule that is its drag handle, lit while it is dragged.
/// Returns the padded rectangle its content goes in.
pub(crate) fn drawer(
    frame: &mut ratatui::Frame<'_>,
    content: Rect,
    panel: ResizablePanel,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) -> Rect {
    let area = side_panel_area(content, drawer_width(panel, model, content));
    frame.render_widget(Clear, area);
    let rule = if model.dragging_panel == Some(panel) {
        Token::Accent
    } else {
        Token::SurfaceRecessed
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(theme::fg(rule))
            .style(theme::bg(Token::SurfaceRecessed)),
        area,
    );
    // First, so the rule answers the pointer before the rows behind it do.
    hits.insert(
        0,
        (
            Rect::new(area.x, area.y, 1, area.height),
            Hit::ResizePanel(panel),
        ),
    );
    Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(3),
        area.height.saturating_sub(2),
    )
}

/// A drawer's content split into its body and the footer
/// [`render_drawer_footer`] draws beneath it.
pub(crate) fn drawer_body_and_footer(
    inner: Rect,
    offers: &[ActionOffer],
    nothing_to_do: Option<&str>,
) -> (Rect, Rect) {
    let footer_height = drawer_footer_height(offers, nothing_to_do);
    let body = Rect {
        height: inner.height.saturating_sub(footer_height),
        ..inner
    };
    (
        body,
        Rect::new(inner.x, body.bottom(), inner.width, footer_height),
    )
}

/// A list's search field: what has been typed, or `placeholder` when
/// nothing has, over a rule that takes the accent while it is `active`.
pub(crate) fn filter_box(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    text: &str,
    placeholder: &str,
    active: bool,
) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(theme::fg(if active {
            Token::Accent
        } else {
            Token::BorderDefault
        }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let line = if text.is_empty() {
        Line::from(Span::styled(
            placeholder.to_owned(),
            theme::fg(Token::TextMuted),
        ))
    } else {
        let mut spans = vec![Span::styled(text.to_owned(), theme::fg(Token::TextPrimary))];
        if active {
            spans.push(Span::styled(
                theme::glyph(Symbol::BarThin),
                theme::fg(Token::Accent),
            ));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(line), inner);
}

/// Where a detail drawer's selected thing stands, in the words its footer
/// prints: a coloured headline, a muted note beneath it, and — when
/// nothing can be done now — what to put on the row the buttons would
/// have been on.
pub(crate) struct DrawerStatus<'a> {
    pub color: Color,
    pub headline: &'a str,
    pub subtitle: &'a str,
    /// A row that simply goes empty reads as a button that failed to draw
    /// rather than as an answer, and the person went looking there. Said
    /// by the caller, never derived here: only the caller knows whether
    /// its actions being unavailable is news (a harness that cannot be
    /// set up because it is not on the machine) or the ordinary state of
    /// a healthy row (a plugin installed and up to date).
    pub nothing_to_do: Option<&'a str>,
}

/// Rows [`render_drawer_footer`] needs: the divider and the two status
/// lines, plus a gap and one row more when there is something to put on
/// it — the buttons for what can be done now, or the sentence saying why
/// there are none.
pub(crate) fn drawer_footer_height(offers: &[ActionOffer], nothing_to_do: Option<&str>) -> u16 {
    if drawer_buttons(offers).is_empty() && nothing_to_do.is_none() {
        3
    } else {
        5
    }
}

/// The offers a drawer draws as buttons: the available ones, what builds
/// before what destroys. `Activate` is left out — it is what opened the
/// drawer, and a button that opens what is already open does nothing.
fn drawer_buttons(offers: &[ActionOffer]) -> Vec<uze_keys::Action> {
    let mut buttons: Vec<uze_keys::Action> = offers
        .iter()
        .filter(|offer| offer.is_available() && offer.action != uze_keys::Action::Activate)
        .map(|offer| offer.action)
        .collect();
    buttons.sort_by_key(|action| action.destructive());
    buttons
}

/// A button's look: soft at rest, the full hue under the pointer — the
/// step between the two is what says it can be clicked. One under way
/// stays `strong`, so the button that started it reads as the one that
/// stops it. `ground` is the surface it sits on, which the soft tint is
/// mixed against.
pub(crate) fn button_style(hue: Token, strong: bool, ground: Token) -> Style {
    let style = if strong {
        theme::on(Token::SurfaceBackground, hue)
    } else {
        Style::default()
            .fg(theme::color(hue))
            .bg(theme::softened(hue, ground))
    };
    style.add_modifier(Modifier::BOLD)
}

/// Every detail drawer ends the same way: where the thing stands, then
/// what can be done about it, as buttons.
///
/// The drawer is the one place a row's actions are performed with the
/// pointer, so its buttons are the selected thing's offers — the available
/// ones only: a button that cannot run is a caption pretending to be a
/// control. The first thing that builds wears the accent, anything else
/// is neutral, and what destroys comes last in the danger colour, so the
/// weight of each is seen before it is clicked. `engaged` is an action
/// already under way (a key being captured), drawn in the warning colour.
pub(crate) fn render_drawer_footer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    status: DrawerStatus<'_>,
    offers: &[ActionOffer],
    hovered: Option<uze_keys::Action>,
    engaged: Option<uze_keys::Action>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme::fg(Token::BorderDefault));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", theme::glyph(Symbol::StatusSelected)),
                Style::default().fg(status.color),
            ),
            Span::styled(
                status.headline.to_owned(),
                Style::default()
                    .fg(status.color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            status.subtitle.to_owned(),
            theme::fg(Token::TextMuted),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x, inner.y, inner.width, inner.height.min(2)),
    );

    let row_y = inner.y + 3;
    if row_y >= area.bottom() {
        return;
    }
    let buttons = drawer_buttons(offers);
    if buttons.is_empty() {
        if let Some(note) = status.nothing_to_do {
            frame.render_widget(
                Paragraph::new(Span::styled(note.to_owned(), theme::fg(Token::TextMuted))),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
        return;
    }
    let mut x = inner.x;
    for (index, action) in buttons.into_iter().enumerate() {
        let label = format!("  {}  ", action.label());
        let width = label.chars().count() as u16;
        if x + width > inner.right() {
            break;
        }
        let hue = if engaged == Some(action) {
            Token::StateWarning
        } else if action.destructive() {
            Token::StateDanger
        } else if index == 0 {
            Token::Accent
        } else {
            Token::TextSecondary
        };
        let style = button_style(
            hue,
            hovered == Some(action) || engaged == Some(action),
            Token::SurfaceRecessed,
        );
        let rect = Rect::new(x, row_y, width, 1);
        frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
        hits.push((rect, Hit::OfferedAction(action)));
        x += width + 2;
    }
}
