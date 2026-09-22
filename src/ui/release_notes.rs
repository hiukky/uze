//! A release's notes, read where the notice about it sits rather than in a
//! browser.
//!
//! Only the release the notice names: the reader asked what they just got,
//! and the history of every other release is the changelog's to keep, one
//! key away on the release page.
//!
//! Both clients carry it — the management screen as an overlay, the
//! workspace client as a modal of its own — and neither draws it: this
//! holds its state, answers its actions and draws it, and the host only
//! decides when it is open and where the notes come from.

use std::cell::Cell;

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph, Wrap},
};
use uze_keys::{Action, Scope};

use crate::self_update::ReleaseNotes;
use crate::ui::{
    extension_view,
    theme::{self, Token},
    widget::{POPUP_H_PAD, Scrollbar, Surface, hint},
};

/// The notes, as far as they have arrived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Notes {
    Loading,
    /// Nothing could be read: offline, or a changelog that does not carry
    /// the release. The release page is still one key away.
    Unavailable,
    Ready(ReleaseNotes),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReleaseNotesModal {
    /// The release the notice named.
    pub(crate) version: String,
    pub(crate) notes: Notes,
    scroll: u16,
    /// How far the last frame let it scroll, and how many rows a page was.
    /// Kept from drawing because that is where the size is known: the
    /// keys and the wheel arrive where it is not, in both clients.
    drawn: Cell<Drawn>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Drawn {
    limit: u16,
    page: u16,
}

/// What an action asks of the host.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    None,
    Close,
    OpenLink(String),
}

impl ReleaseNotesModal {
    pub(crate) fn opening(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            notes: Notes::Loading,
            scroll: 0,
            drawn: Cell::default(),
        }
    }

    /// The notes for `version` arrived — or did not. An answer about
    /// another release is one asked for a modal since closed, and dropped.
    pub(crate) fn absorb(&mut self, version: &str, notes: Option<ReleaseNotes>) -> bool {
        if version != self.version || self.notes != Notes::Loading {
            return false;
        }
        self.notes = notes.map_or(Notes::Unavailable, Notes::Ready);
        self.scroll = 0;
        true
    }

    pub(crate) fn act(&mut self, action: Action) -> Outcome {
        let page = self.drawn.get().page.max(1);
        match action {
            Action::Dismiss => return Outcome::Close,
            Action::Activate => {
                return Outcome::OpenLink(crate::self_update::release_page(&self.version));
            }
            Action::SelectNext => self.scroll_to(self.scroll.saturating_add(1)),
            Action::SelectPrevious => self.scroll = self.scroll.saturating_sub(1),
            Action::ScrollPageDown => self.scroll_to(self.scroll.saturating_add(page)),
            Action::ScrollPageUp => self.scroll = self.scroll.saturating_sub(page),
            _ => {}
        }
        Outcome::None
    }

    pub(crate) fn wheel(&mut self, down: bool) {
        match down {
            true => self.scroll_to(self.scroll.saturating_add(1)),
            false => self.scroll = self.scroll.saturating_sub(1),
        }
    }

    fn scroll_to(&mut self, scroll: u16) {
        self.scroll = scroll.min(self.drawn.get().limit);
    }
}

/// Where everything goes.
struct Layout {
    popup: Rect,
    header: Rect,
    body: Rect,
    lines: Vec<Line<'static>>,
    rows: u16,
}

impl Layout {
    fn scroll_limit(&self) -> u16 {
        self.rows.saturating_sub(self.body.height)
    }
}

const WIDEST: u16 = 96;

fn layout(area: Rect, modal: &ReleaseNotesModal) -> Layout {
    let width = area.width.saturating_sub(4).min(WIDEST);
    let height = (area.height.saturating_mul(4) / 5).max(area.height.min(8));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let inner = surface().into_block().inner(popup);
    let header = Rect::new(inner.x, inner.y, inner.width, 1.min(inner.height));
    // A blank row under the header, and a column kept for the scrollbar.
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(2).min(inner.bottom()),
        inner.width.saturating_sub(Scrollbar::width() + 1),
        inner.height.saturating_sub(2),
    );
    let lines = body_lines(modal);
    let wrap = usize::from(body.width.max(1));
    let rows = lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(wrap) as u16)
        .fold(0u16, u16::saturating_add);
    Layout {
        popup,
        header,
        body,
        lines,
        rows,
    }
}

fn surface() -> Surface {
    Surface::floating()
        .title(" what's new ")
        .hint(hint::line(
            &[Scope::ReleaseNotes],
            &[Action::Activate, Action::Dismiss],
        ))
        .padding(Padding::new(POPUP_H_PAD, POPUP_H_PAD, 1, 0))
}

fn body_lines(modal: &ReleaseNotesModal) -> Vec<Line<'static>> {
    let muted = |text: &str| {
        vec![Line::from(Span::styled(
            text.to_owned(),
            theme::fg(Token::TextMuted),
        ))]
    };
    match &modal.notes {
        Notes::Loading => muted("Reading the notes…"),
        Notes::Unavailable => {
            muted("The notes could not be read. The release page has them — it opens from here.")
        }
        Notes::Ready(notes) => extension_view::prose(&uze_extensions::code::markdown(
            &notes.body,
            uze_theme::active().syntax_theme(),
        )),
    }
}

/// The release's version and, once the notes say it, its date.
fn header_line(modal: &ReleaseNotesModal) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("v{}", modal.version),
        theme::fg_bold(Token::TextPrimary),
    )];
    if let Notes::Ready(ReleaseNotes {
        date: Some(date), ..
    }) = &modal.notes
    {
        spans.push(Span::styled(
            format!("  {date}"),
            theme::fg(Token::TextFaint),
        ));
    }
    Line::from(spans)
}

/// Draws the modal centred in `area` and answers the rectangle it took, so
/// the host can tell a click inside it from one that closes it.
pub(crate) fn render(frame: &mut Frame<'_>, area: Rect, modal: &ReleaseNotesModal) -> Rect {
    let layout = layout(area, modal);
    frame.render_widget(Clear, layout.popup);
    surface().render(frame, layout.popup);
    frame.render_widget(Paragraph::new(header_line(modal)), layout.header);
    let scroll = modal.scroll.min(layout.scroll_limit());
    modal.drawn.set(Drawn {
        limit: layout.scroll_limit(),
        page: layout.body.height.saturating_sub(1),
    });
    frame.render_widget(
        Paragraph::new(layout.lines.clone())
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        layout.body,
    );
    let track = Rect::new(
        layout.body.right() + 1,
        layout.body.y,
        Scrollbar::width(),
        layout.body.height,
    );
    if let Some(scrollbar) = Scrollbar::measure(
        track,
        usize::from(layout.body.height),
        usize::from(layout.rows),
    ) {
        scrollbar.render(frame, usize::from(scroll));
    }
    layout.popup
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn notes(version: &str, body: &str) -> ReleaseNotes {
        ReleaseNotes {
            version: version.to_owned(),
            date: Some("2026-09-22".to_owned()),
            body: body.to_owned(),
        }
    }

    fn drawn(modal: &ReleaseNotesModal) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                render(frame, frame.area(), modal);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn it_shows_the_notes_of_the_release_the_notice_named_rendered() {
        let mut modal = ReleaseNotesModal::opening("9.0.1");
        assert!(drawn(&modal).contains("Reading the notes"));

        assert!(modal.absorb(
            "9.0.1",
            Some(notes("9.0.1", "### Fixes\n\n- **terminal:** the fix"))
        ));
        let screen = drawn(&modal);
        assert!(
            screen.contains("v9.0.1") && screen.contains("2026-09-22"),
            "{screen}"
        );
        assert!(screen.contains("the fix"), "rendered, not raw: {screen}");
        assert!(!screen.contains("**terminal:**"), "{screen}");

        assert_eq!(
            modal.act(Action::Activate),
            Outcome::OpenLink(crate::self_update::release_page("9.0.1"))
        );
        assert_eq!(modal.act(Action::Dismiss), Outcome::Close);
    }

    #[test]
    fn notes_that_cannot_be_read_still_offer_the_release_page() {
        let mut modal = ReleaseNotesModal::opening("9.0.1");
        assert!(
            !modal.absorb("9.0.0", None),
            "an answer for another release"
        );
        assert!(modal.absorb("9.0.1", None));
        assert!(drawn(&modal).contains("could not be read"));
        assert_eq!(
            modal.act(Action::Activate),
            Outcome::OpenLink(crate::self_update::release_page("9.0.1"))
        );
    }

    #[test]
    fn the_scroll_stops_where_the_notes_end() {
        let area = Rect::new(0, 0, 100, 30);
        let body = (0..80).map(|n| format!("- item {n}\n")).collect::<String>();
        let mut modal = ReleaseNotesModal::opening("9.0.1");
        modal.absorb("9.0.1", Some(notes("9.0.1", &body)));
        drawn(&modal);
        for _ in 0..500 {
            modal.act(Action::ScrollPageDown);
        }
        let limit = layout(area, &modal).scroll_limit();
        assert!(limit > 0);
        assert_eq!(modal.scroll, limit);
        assert!(drawn(&modal).contains("item 79"));
        modal.act(Action::ScrollPageUp);
        assert!(modal.scroll < limit);
        let paged = modal.scroll;
        modal.act(Action::SelectPrevious);
        modal.wheel(false);
        assert_eq!(
            modal.scroll,
            paged - 2,
            "the arrows and the wheel scroll too"
        );
    }
}
