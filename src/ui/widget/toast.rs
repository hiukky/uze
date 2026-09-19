//! A toast: something happened, said where the reader is looking, and gone
//! again without being dismissed.
//!
//! # Why they are transient and what that costs
//!
//! A toast covers what is behind it, and in this product what is behind it
//! is a pane an agent is writing into. That is the whole tension: the
//! message is worth interrupting for, the output underneath is the reason
//! anybody is here. So a toast is narrow, right-aligned, and leaves on its
//! own — and the one kind that does not leave on its own is the one the
//! reader has to answer, which is exactly the trade worth making.
//!
//! The remaining seconds are *drawn*. A box that vanishes on a clock
//! nobody can see reads as a glitch the first time and as something the
//! reader has to hurry for after that; a box that says `4s` is one they
//! can decide to ignore. One that stays draws no clock at all — the `✕`
//! beside it already says the message can be ended, and a word saying
//! "this one has no clock" is a thing to read on every toast to learn
//! something about one of them.
//!
//! # What a toast is not
//!
//! Not a log — it is gone, so anything that must be readable afterwards
//! belongs somewhere that keeps it. Not a dialog — it never takes the
//! keyboard, and the screen behind it stays live and answerable. Not a
//! progress report — work still running is the header's own business,
//! where it can sit as long as it needs to.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use uze_theme::Token;

use super::{row, text};
use crate::ui::theme::{self, Symbol};

/// The one width every toast draws at.
///
/// One, not each its own. A stack of four boxes sized to their own words
/// has four left edges, and the eye reads four things rather than one
/// column of messages — the raggedness is the first thing it sees and it
/// says nothing, because a message is not longer for being more important.
///
/// A cap as well as a width: past this a box is wide enough to read a
/// paragraph in, which is a box that has taken the pane, and the pane is
/// why anybody is here. What a summary leaves out is reachable where it is
/// kept; the toast is gone in seconds either way.
const WIDTH: u16 = 54;

/// The columns between the message and what follows it on the right.
const GAP: u16 = 2;

/// The column of air inside each end of a toast, so its words never touch
/// the edge of their own ground.
const PAD: u16 = 1;

/// What kind of thing a toast is telling the reader.
///
/// The hue carries the class and the mark carries it again, because a
/// terminal is the one surface where colour may be the reader's own
/// palette rather than this design's: a toast that said "danger" only in
/// red would say nothing at all on a theme that maps red elsewhere.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToastKind {
    /// It worked.
    Done,
    /// It did not work.
    Failed,
    /// It worked, and something about it needs the reader.
    Warned,
    /// Neither — something happened that is worth saying once.
    Told,
}

impl ToastKind {
    fn hue(self) -> Token {
        match self {
            Self::Done => Token::StateSuccess,
            Self::Failed => Token::StateDanger,
            Self::Warned => Token::StateWarning,
            Self::Told => Token::Accent,
        }
    }

    fn mark(self) -> Symbol {
        match self {
            Self::Done => Symbol::MarkOk,
            // Its own symbol, not the close mark: close is a control the
            // reader presses, this is an outcome they are being told, and
            // only a glyph set that has both can spell them apart.
            Self::Failed => Symbol::MarkFailed,
            Self::Warned => Symbol::MarkAttention,
            Self::Told => Symbol::ChevronRight,
        }
    }
}

/// One message on its way out.
#[derive(Clone, Debug)]
pub(crate) struct Toast {
    kind: ToastKind,
    /// What happened, in as few words as say it.
    text: String,
    /// The line under it: which task, which remote, what the reason was.
    detail: String,
    /// What the reader can do about it, if anything — the words of the
    /// offer, underlined where it is drawn.
    action: Option<String>,
    /// Seconds left, or `None` for one that stays until it is answered.
    remaining: Option<u64>,
}

impl Toast {
    /// A toast saying `text`, with `detail` under it.
    ///
    /// Both, always. They are its own field rather than one longer title
    /// because the two are read differently — the title is scanned across
    /// a stack of four, the detail only by whoever stopped at this one —
    /// and a toast that had only the first drew a blank band where the
    /// second belongs, which reads as a rendering fault rather than as a
    /// message with nothing more to say.
    ///
    /// Required rather than optional for the same reason a title is: a
    /// message worth interrupting for is worth saying what it is *about*.
    /// Where there is genuinely nothing more, the detail is what the
    /// outcome applies to — the task, the branch, the remote — which is
    /// the thing the reader needs and the title has no room for.
    pub(crate) fn new(kind: ToastKind, text: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            detail: detail.into(),
            action: None,
            remaining: None,
        }
    }

    /// The offer beside the message. A toast with one is a toast the
    /// reader can answer without going to find where the thing lives.
    pub(crate) fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    /// How long this one has left. Absent, it stays.
    pub(crate) fn remaining(mut self, seconds: Option<u64>) -> Self {
        self.remaining = seconds;
        self
    }

    /// The seconds left, or nothing at all for a toast that stays.
    ///
    /// Nothing rather than a word: the `✕` beside it already says the
    /// message can be ended, so an empty clock reads as "no clock" without
    /// spending columns to say so. A word there was one more thing to read
    /// on every toast to learn something about one of them.
    fn clock(&self) -> String {
        self.remaining
            .map(|seconds| format!("{seconds}s"))
            .unwrap_or_default()
    }

    /// The mark and the space after it. The detail is indented by the same
    /// amount, so it hangs under the title rather than under the mark.
    fn lead(&self) -> u16 {
        theme::width(self.kind.mark()) + 1
    }

    /// The clock and the close mark, which ride with the title. A toast
    /// that stays spends nothing on the clock it does not have.
    fn head_tail(&self) -> u16 {
        let clock = self.clock().chars().count() as u16;
        let gap = if clock == 0 { 0 } else { 1 };
        clock + gap + close_width()
    }

    /// The offer, which rides with the detail.
    fn foot_tail(&self) -> u16 {
        self.action
            .as_ref()
            .map_or(0, |label| label.chars().count() as u16)
    }

    /// Draws into `area` — two rows, no frame — and answers with the rects
    /// its marks took.
    ///
    /// **No border.** A frame encloses something that was already on
    /// screen; this arrived over it. What separates a toast from the pane
    /// is its ground alone.
    ///
    /// **A neutral ground, whatever it is saying.** The hue is on the mark
    /// and nowhere else. Four tinted grounds stacked read as a wall of
    /// colour and make the reader parse the *surface* before the words,
    /// which is backwards — and a message is not more urgent for being
    /// wider. One surface, four marks.
    ///
    /// **Title, then detail.** The first row is what happened and the
    /// controls that belong to the toast itself — the clock it is running
    /// on and the mark that ends it early. The second is what the title
    /// left out, indented under it, with the offer at its right. The two
    /// rows answer different questions, so nothing on one competes for the
    /// other's columns.
    fn render(&self, frame: &mut ratatui::Frame<'_>, area: Rect) -> Marks {
        let nothing = Marks {
            action: None,
            close: Rect::new(area.x, area.y, 0, 1),
        };
        if area.width == 0 || area.height < ROWS {
            return nothing;
        }
        let ground = theme::color(Token::SurfaceRaised);
        let on = |style: Style| style.bg(ground);
        let pad = |columns: u16| {
            Span::styled(
                " ".repeat(usize::from(columns)),
                Style::default().bg(ground),
            )
        };

        super::fill(frame, area, Token::SurfaceRaised);
        let head = Rect::new(area.x, area.y, area.width, 1);
        let foot = Rect::new(area.x, area.y + 1, area.width, 1);

        // ── the title, the clock, and the mark that ends it early ────
        let clock = self.clock();
        let close = Rect::new(
            area.right().saturating_sub(PAD + close_width()),
            head.y,
            close_width(),
            1,
        );
        let room = area
            .width
            .saturating_sub(PAD + self.lead() + GAP + self.head_tail() + PAD);
        let mut title = vec![
            pad(PAD),
            Span::styled(
                format!("{} ", theme::glyph(self.kind.mark())),
                on(theme::fg(self.kind.hue())),
            ),
            Span::styled(
                text::elide(&self.text, room.max(1) as usize),
                on(theme::fg_bold(Token::TextBright)),
            ),
        ];
        let used: u16 = title.iter().map(|span| span.width() as u16).sum();
        let clocked = clock.chars().count() as u16;
        let tail_x = close
            .x
            .saturating_sub(if clocked == 0 { 0 } else { clocked + 1 });
        title.push(pad(tail_x.saturating_sub(area.x + used).max(1)));
        if clocked > 0 {
            title.push(Span::styled(clock, on(theme::fg(Token::TextDim))));
            title.push(pad(1));
        }
        title.push(Span::styled(
            theme::glyph(Symbol::MarkClose),
            on(theme::fg(Token::TextDim)),
        ));
        frame.render_widget(Paragraph::new(Line::from(title)), head);

        // ── the detail, under the title, with the offer at its right ──
        let mut action_rect = None;
        let room = area
            .width
            .saturating_sub(PAD + self.lead() + GAP + self.foot_tail() + PAD);
        let mut under = vec![
            pad(PAD + self.lead()),
            Span::styled(
                text::elide(&self.detail, room.max(1) as usize),
                on(theme::fg(Token::TextMuted)),
            ),
        ];
        if let Some(label) = &self.action {
            let width = label.chars().count() as u16;
            let at = area.right().saturating_sub(PAD + width);
            let used: u16 = under.iter().map(|span| span.width() as u16).sum();
            under.push(pad(at.saturating_sub(area.x + used).max(1)));
            // The offer wears the accent rather than the kind's hue: it is
            // something to do, and in this design what can be done is the
            // accent wherever it appears.
            under.push(Span::styled(
                label.clone(),
                on(Style::default()
                    .fg(theme::color(Token::Accent))
                    .add_modifier(Modifier::UNDERLINED)),
            ));
            action_rect = Some(Rect::new(at, foot.y, width, 1));
        }
        row::pad_to(&mut under, area.width, ground);
        frame.render_widget(Paragraph::new(Line::from(under)), foot);

        Marks {
            action: action_rect,
            close,
        }
    }
}

/// The targets one drawn toast offers.
#[derive(Clone, Copy, Debug)]
struct Marks {
    action: Option<Rect>,
    close: Rect,
}

/// The columns the close mark takes.
fn close_width() -> u16 {
    theme::width(Symbol::MarkClose)
}

/// Where one toast landed: the row it took, the rect of its offer when it
/// made one, and the mark that puts it away.
///
/// All three, because the caller has to tell those clicks apart — and the
/// row is a target of its own, since a message you can only put away by
/// hitting one cell is one you fight with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    pub(crate) box_rect: Rect,
    pub(crate) action: Option<Rect>,
    /// The `✕`. Redundant as a target — the whole row dismisses — and not
    /// redundant at all as a sign: without it, nothing says the message
    /// can be put away early, and the reader waits out a clock they did
    /// not have to.
    pub(crate) close: Rect,
}

/// Draws `toasts` stacked against the top-right of `area`, in the order
/// given, and answers with where each one landed.
///
/// Top-anchored: the stack grows *down* from where it starts, so a toast
/// arriving does not move the ones already being read. A stack that grew
/// upward put every box in motion on every arrival, which is the one thing
/// a transient message must not do.
///
/// One row each with one between them. The gap is what keeps four grounds
/// of four hues reading as four messages; without it they meet and the
/// stack is a block of colour.
pub(crate) fn stack(frame: &mut ratatui::Frame<'_>, area: Rect, toasts: &[Toast]) -> Vec<Placed> {
    // One width for the whole column, decided before the first box is
    // drawn: a toast arriving changes neither where the others sit nor how
    // wide they are.
    let width = WIDTH.min(area.width);
    let mut placed = Vec::with_capacity(toasts.len());
    for (index, toast) in toasts.iter().enumerate() {
        let top = area.y + index as u16 * (ROWS + GAP_ROWS);
        if top + ROWS > area.bottom() {
            break;
        }
        let box_rect = Rect::new(area.right() - width, top, width, ROWS);
        let marks = toast.render(frame, box_rect);
        placed.push(Placed {
            box_rect,
            action: marks.action,
            close: marks.close,
        });
    }
    placed
}

/// The rows one toast takes: the message, then what is about it.
const ROWS: u16 = 2;

/// The blank row between one toast and the next.
const GAP_ROWS: u16 = 1;
