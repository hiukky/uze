//! TUI view — shared helpers used by every route's render function.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::ui::hit::Hit;
use crate::ui::model::{ResizablePanel, TuiModel};
use crate::ui::side_panel_area;
use crate::ui::theme::{self, Symbol, Token};
use crate::ui::widget::{Align, Button, Edge, Field, Rule, button_row};
use uze_application::application::offers::ActionOffer;

pub mod appearance;
pub mod extensions;
pub mod harnesses;
pub mod health;
pub mod keys;
pub mod overview;
pub mod plugins;
pub mod profiles;

pub(crate) const DRAWER_DEFAULT_WIDTH: u16 = 44;
/// The narrowest a drawer, or the list beside it, is ever drawn.
const DRAWER_MIN_WIDTH: u16 = 24;

/// How wide `panel`'s drawer is drawn over `content`: where it was dragged
/// to, or the default, never squeezing itself or the list beside it past
/// the minimum, and never taking more than half of what there is.
///
/// The half is the part the minimum alone could not do. Leaving the list
/// its 24 columns is a floor, not a share: on a narrow screen a drawer at
/// its default took two thirds of the content and the list it is a detail
/// *of* got what was left over. A detail panel is read a paragraph at a
/// time and the list beside it is read as a whole, so when there is not
/// enough for both, the list is the one that keeps its shape.
pub(crate) fn drawer_width(panel: ResizablePanel, model: &TuiModel, content: Rect) -> u16 {
    let ceiling = (content.width / 2)
        .min(content.width.saturating_sub(DRAWER_MIN_WIDTH))
        .max(DRAWER_MIN_WIDTH);
    panel
        .width(model)
        .unwrap_or(DRAWER_DEFAULT_WIDTH)
        .clamp(DRAWER_MIN_WIDTH, ceiling)
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
    Rule::new(Edge::Left)
        .tone(rule)
        .ground(Token::SurfaceRecessed)
        .render(frame, area);
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
pub(crate) fn drawer_body_and_footer(inner: Rect, offers: &[ActionOffer]) -> (Rect, Rect) {
    let footer_height = drawer_footer_height(offers);
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
    Field::new(text, placeholder)
        .focused(active)
        .render(frame, area);
}

/// Where a detail drawer's selected thing stands, in the words its footer
/// prints: a coloured headline and a muted note beneath it.
pub(crate) struct DrawerStatus<'a> {
    pub color: Color,
    pub headline: &'a str,
    pub subtitle: &'a str,
}

/// Rows [`render_drawer_footer`] needs: the divider, the headline and the
/// two rows its note is allowed to take, plus a gap and a row of buttons
/// when anything can be done now.
///
/// Two rows for the note, because it is a sentence and the drawer is a
/// width the operator drags. On one row it was clipped mid-word the
/// moment the drawer was narrower than the longest note anyone had
/// written — "setting it up installs it" became "setting it up install",
/// which is not a shorter sentence but a broken one.
pub(crate) fn drawer_footer_height(offers: &[ActionOffer]) -> u16 {
    if drawer_buttons(offers).is_empty() {
        NOTE_ROWS + 2
    } else {
        NOTE_ROWS + 4
    }
}

/// How many rows a drawer's note may wrap onto before it is elided.
const NOTE_ROWS: u16 = 2;

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
    let inner = Rule::new(Edge::Top)
        .tone(Token::BorderDefault)
        .render(frame, area);
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
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.min(1 + NOTE_ROWS),
        ),
    );

    let row_y = inner.y + 1 + NOTE_ROWS;
    if row_y >= area.bottom() {
        return;
    }
    let buttons: Vec<_> = drawer_buttons(offers)
        .into_iter()
        .enumerate()
        .map(|(index, action)| {
            let hue = if engaged == Some(action) {
                Token::StateWarning
            } else if action.destructive() {
                Token::StateDanger
            } else if index == 0 {
                Token::Accent
            } else {
                Token::TextSecondary
            };
            (
                Button::new(action.label(), hue)
                    .strong(hovered == Some(action) || engaged == Some(action))
                    .ground(Token::SurfaceRecessed),
                Hit::OfferedAction(action),
            )
        })
        .collect();
    let row = Rect::new(inner.x, row_y, inner.width, 1);
    hits.extend(button_row(frame, row, &buttons, Align::Left));
}

#[cfg(test)]
mod drawer_tests {
    use super::*;

    /// A drawer never takes more than half of what there is, whatever it
    /// was dragged to or defaults to.
    ///
    /// Leaving the list its minimum is a floor, not a share: on a narrow
    /// screen the default took two thirds of the content and the list it
    /// is a detail *of* got the remainder. The default itself is the
    /// width a paragraph reads at, not the width there is.
    #[test]
    fn a_drawer_never_takes_more_than_half_the_content() {
        let model = TuiModel::default();
        let panel = ResizablePanel::AppearanceDrawer;
        for width in [60u16, 80, 100, 140, 200] {
            let content = Rect::new(0, 0, width, 40);
            let drawn = drawer_width(panel, &model, content);
            assert!(
                drawn <= (width / 2).max(DRAWER_MIN_WIDTH),
                "{width} wide: {drawn}"
            );
            assert!(
                content.width.saturating_sub(drawn) >= DRAWER_MIN_WIDTH,
                "and the list keeps its own minimum at {width}: {drawn}"
            );
        }
        // Wide enough for both, it is the default and not a share of the
        // screen: a detail panel does not get better by getting wider.
        assert_eq!(
            drawer_width(panel, &model, Rect::new(0, 0, 200, 40)),
            DRAWER_DEFAULT_WIDTH
        );
    }
}
