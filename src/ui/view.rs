//! TUI view — shared helpers used by every route's render function.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::ui::hit::Hit;
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

/// Where a detail drawer's selected thing stands, in the words its footer
/// prints: a coloured headline and a muted note beneath it.
pub(crate) struct DrawerStatus<'a> {
    pub color: Color,
    pub headline: &'a str,
    pub subtitle: &'a str,
}

/// Rows [`render_drawer_footer`] needs: the divider and the two status
/// lines, plus a gap and a row of buttons when anything can be done now.
pub(crate) fn drawer_footer_height(offers: &[ActionOffer]) -> u16 {
    if drawer_buttons(offers).is_empty() {
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
    let mut x = inner.x;
    for (index, action) in drawer_buttons(offers).into_iter().enumerate() {
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

/// Folds `text` to `width` the way the drawer's paragraph would, but
/// *before* it is authored — so every line the drawer pushes is one drawn
/// row, and a row index is a screen row.
///
/// The drawer renders through `Wrap`, and its two clickable rows (the
/// marketplace name, the address under it) were anchored by counting
/// authored lines. A description long enough to fold pushed the drawn rows
/// down and left the targets sitting above them: the address read as a
/// link and answered nothing. Wrapping the free text here keeps the two
/// counts the same number by construction, with no measurement of what
/// ratatui did afterwards.
pub(crate) fn fold(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        // A word wider than the row is broken across rows rather than
        // left to overflow — the paragraph's own wrapper does the same,
        // and a bare URL in a description is exactly that word.
        let mut word = word;
        while word.chars().count() > width {
            if !row.is_empty() {
                rows.push(std::mem::take(&mut row));
            }
            let split = word
                .char_indices()
                .nth(width)
                .map_or(word.len(), |(index, _)| index);
            let (head, tail) = word.split_at(split);
            rows.push(head.to_owned());
            word = tail;
        }
        let projected = row.chars().count() + usize::from(!row.is_empty()) + word.chars().count();
        if projected > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}
