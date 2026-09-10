//! Drawing an extension's [`View`].
//!
//! Everything geometric about an extension overlay lives here: the split
//! between navigator and content, how a row scrolls into sight, how wide a
//! wrapped diff line ends up, and which rectangle a click belongs to. An
//! extension answers with content and never sees a coordinate, so this is
//! the only side that can be wrong about layout — which is the point, since
//! it used to be two sides deriving the same rectangles independently.
//!
//! Colour resolution lives here too: [`Role`] is the extension's
//! vocabulary, and mapping it onto the palette below is what keeps an
//! overlay looking like the rest of the TUI without an extension holding a
//! copy of the colour table.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as TextSpan},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};
use uze_extensions::view::{
    Caret, Command, Content, ContentLine, LineTone, Mode, Navigator, NavigatorRow, Role, RowIcon,
    ScrollTarget, Section, Size, Span, View, ViewHit,
};

use crate::ui::scrollbar::Scrollbar;
use crate::ui::theme::{self, Symbol, Token};

/// Narrowest/widest the navigator can be dragged, and the floor left for
/// the content column — the same shape as the host TUI's own
/// `clamp_sidebar_width`, scoped to an extension overlay.
const MIN_NAVIGATOR_WIDTH: u16 = 20;
const MAX_NAVIGATOR_WIDTH: u16 = 50;
const MIN_EXTENSION_CONTENT_WIDTH: u16 = 40;

const GUTTER_WIDTH: u16 = 7;

/// Margin on each side of unnumbered content.
///
/// A numbered line already starts a gutter's width in, and ends well
/// short of the edge because code is short; that is where every other
/// mode's breathing room comes from. A rendered document has neither — it
/// has no gutter, and its paragraphs wrap to the full width — so without
/// this it runs into both borders. Two columns, the same as the
/// management screens' own content inset, so the two surfaces indent
/// their text by the same amount.
const PROSE_INSET: u16 = 2;

/// Columns of padding on each side of a mode segment's label. The padding
/// is part of the button — it is filled, and clicked, like the label is.
const MODE_PAD: u16 = 1;

/// The extension's palette, resolved. An extension names meaning; the host
/// names colour, exactly once, here.
fn color(role: Role) -> Color {
    match role {
        Role::Default => theme::color(Token::TextBright),
        Role::Muted => theme::color(Token::TextMuted),
        Role::Secondary => theme::color(Token::TextSecondary),
        Role::Bright => theme::color(Token::TextBright),
        Role::Inactive => theme::color(Token::TextInactive),
        Role::Accent => theme::color(Token::Accent),
        Role::Dim => theme::color(Token::TextDim),
        Role::Faint => theme::color(Token::TextFaint),
        Role::Info => theme::color(Token::StateInfo),
        Role::Success => theme::color(Token::StateSuccess),
        Role::Warning => theme::color(Token::StateWarning),
        Role::Danger => theme::color(Token::StateDanger),
    }
}

fn styled(span: &Span) -> TextSpan<'static> {
    let mut style = Style::default().fg(span
        .color
        .map(|rgb| theme::content(rgb.0, rgb.1, rgb.2))
        .unwrap_or_else(|| color(span.role)));
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    TextSpan::styled(span.text.clone(), style)
}

pub(crate) fn clamp_navigator_width(width: u16, total_width: u16) -> u16 {
    let max = total_width
        .saturating_sub(MIN_EXTENSION_CONTENT_WIDTH)
        .clamp(MIN_NAVIGATOR_WIDTH, MAX_NAVIGATOR_WIDTH);
    width.clamp(MIN_NAVIGATOR_WIDTH, max)
}

/// The navigator/content split, derived from the outer overlay area so
/// drawing and hit-testing always share the exact same geometry. This
/// splits horizontally first, so the navigator column spans the entire
/// inner height and its right-hand divider reaches edge to edge; only the
/// content side is split again to carve out a footer that belongs to that
/// column alone rather than reading as a global app bar.
pub(crate) fn content_columns(
    frame_area: Rect,
    navigator_width_override: Option<u16>,
) -> (Rect, Rect, Rect) {
    // The frame takes a row and a column at each edge; one more column
    // of breathing room inside it, and one blank row under the title it
    // carries.
    let inner = Rect::new(
        frame_area.x + 2,
        frame_area.y + 2,
        frame_area.width.saturating_sub(4),
        frame_area.height.saturating_sub(3),
    );
    let navigator_width = navigator_width_override
        .map(|width| clamp_navigator_width(width, inner.width))
        .unwrap_or_else(|| (inner.width / 4).clamp(MIN_NAVIGATOR_WIDTH, MAX_NAVIGATOR_WIDTH));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(navigator_width), Constraint::Min(10)])
        .split(inner);
    let content_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(columns[1]);
    (columns[0], content_rows[0], content_rows[1])
}

/// How much room the content column has, for an extension deciding how
/// much to produce.
pub(crate) fn content_space(frame_area: Rect, navigator_width_override: Option<u16>) -> Size {
    let (_, content, _) = content_columns(frame_area, navigator_width_override);
    Size {
        width: content.width,
        height: content.height,
    }
}

/// Which half of the overlay the pointer is over — the host's answer,
/// because the host owns the layout.
pub(crate) fn scroll_target(
    frame_area: Rect,
    navigator_width_override: Option<u16>,
    column: u16,
    row: u16,
) -> Option<ScrollTarget> {
    let (navigator, content, _) = content_columns(frame_area, navigator_width_override);
    let inside = |rect: Rect| {
        rect.x <= column
            && column < rect.x + rect.width
            && rect.y <= row
            && row < rect.y + rect.height
    };
    if inside(navigator) {
        Some(ScrollTarget::Navigator)
    } else if inside(content) {
        Some(ScrollTarget::Content)
    } else {
        None
    }
}

/// Draws the overlay across the entire frame — every other row this frame
/// would otherwise have drawn is skipped by the caller rather than drawn
/// and covered.
/// Where the navigator's list is scrolled to, which the host keeps
/// because only the host knows how many rows fit (see
/// [`Navigator::anchor`]). Handed into a frame and handed back settled:
/// clamped to the rows that exist, and moved just far enough to show an
/// anchor the extension has changed since the last frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct NavigatorScroll {
    /// The first row shown.
    pub(crate) first: usize,
    /// The anchor the list was last scrolled to reveal, so the same
    /// anchor asked for again does not pull the list back to it.
    pub(crate) revealed: Option<usize>,
}

impl NavigatorScroll {
    pub(crate) fn scrolled(self, direction: uze_extensions::view::ScrollDirection) -> Self {
        use uze_extensions::view::ScrollDirection;
        Self {
            first: match direction {
                ScrollDirection::Up => self.first.saturating_sub(1),
                ScrollDirection::Down => self.first.saturating_add(1),
            },
            ..self
        }
    }

    /// The scroll a list of `rows` rows in `visible` lines settles at.
    fn settled(self, anchor: Option<usize>, rows: usize, visible: usize) -> Self {
        let mut first = self.first.min(rows.saturating_sub(visible));
        if anchor != self.revealed
            && let Some(anchor) = anchor
        {
            if anchor < first {
                first = anchor;
            } else if anchor >= first + visible {
                first = (anchor + 1).saturating_sub(visible);
            }
        }
        Self {
            first,
            revealed: anchor,
        }
    }
}

/// What one frame of an extension surface left behind for the next event
/// to read: where its list settled, and the two scrollbars it drew.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Rendered {
    pub(crate) navigator_scroll: NavigatorScroll,
    pub(crate) navigator_bar: Option<Scrollbar>,
    pub(crate) content_bar: Option<Scrollbar>,
}

pub(crate) fn render(
    frame: &mut ratatui::Frame<'_>,
    view: &View,
    area: Rect,
    navigator_width_override: Option<u16>,
    navigator_scroll: NavigatorScroll,
    hits: &mut Vec<(Rect, ViewHit)>,
) -> Rendered {
    frame.render_widget(Clear, area);
    // The frame is back, and quiet. What made it wrong before was not
    // that it existed but that every line here weighed the same: a box,
    // a divider and two grooves in one hue, arguing. The weights are a
    // hierarchy now — the frame is the faintest thing on screen, the
    // divider is ordinary, and the scroll handle is the only bright rule
    // — so the box reads as the edge of a surface rather than as another
    // control.
    let mut title: Vec<TextSpan<'static>> = vec![TextSpan::raw(" ")];
    title.extend(view.title.iter().map(styled));
    title.push(TextSpan::raw(" "));
    frame.render_widget(
        Block::default()
            .title(Line::from(title))
            .borders(Borders::ALL)
            .border_style(theme::fg(Token::BorderFaint))
            .style(theme::bg(Token::SurfaceBackground)),
        area,
    );
    // This closes the whole overlay, so make it an explicit, comfortably
    // clickable control rather than the compact tab-close glyph.
    let close_rect = Rect::new(area.right().saturating_sub(10), area.y, 9, 1);
    frame.render_widget(
        Paragraph::new(TextSpan::styled(
            format!(" {} close ", theme::glyph(Symbol::MarkClose)),
            theme::fg(Token::StateDanger),
        )),
        close_rect,
    );
    hits.push((close_rect, ViewHit::Close));

    let (navigator_area, content_area, footer) = content_columns(area, navigator_width_override);

    let mut rendered = Rendered {
        navigator_scroll,
        ..Rendered::default()
    };
    if let Some(navigator) = view.navigator.as_ref() {
        let (settled, bar) =
            render_navigator(frame, navigator_area, navigator, navigator_scroll, hits);
        rendered.navigator_scroll = settled;
        rendered.navigator_bar = bar;
    }
    // One target for the whole edge, because the edge is one line doing
    // two jobs: the split moves sideways, the list scrolls down. Which a
    // press meant is the first movement's to say — see
    // [`ViewHit::GrabNavigatorEdge`].
    hits.push((
        Rect::new(
            navigator_area.right().saturating_sub(1),
            navigator_area.y,
            1,
            navigator_area.height,
        ),
        ViewHit::GrabNavigatorEdge,
    ));
    match &view.content {
        Content::Message { text, hint, role } => {
            render_message(frame, content_area, text, hint.as_deref(), color(*role))
        }
        Content::Lines {
            heading,
            scroll,
            lines,
            total,
            caret,
        } => {
            rendered.content_bar = render_lines(
                frame,
                content_area,
                Lines {
                    heading,
                    scroll: *scroll,
                    lines,
                    total: *total,
                    caret: *caret,
                    modes: &view.modes,
                },
                hits,
            );
        }
    }
    render_footer(frame, footer, &view.footer);
    rendered
}

fn render_navigator(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    navigator: &Navigator,
    scroll: NavigatorScroll,
    hits: &mut Vec<(Rect, ViewHit)>,
) -> (NavigatorScroll, Option<Scrollbar>) {
    let panel = Block::default()
        .borders(Borders::RIGHT)
        .border_style(theme::fg(Token::BorderDefault))
        .padding(Padding::new(1, 1, 0, 0))
        .style(theme::bg(Token::SurfaceBackground));
    let inner = panel.inner(area);
    frame.render_widget(panel, area);

    let mut heading = vec![TextSpan::styled(
        navigator.heading.clone(),
        Style::default()
            .fg(theme::color(Token::TextSecondary))
            .add_modifier(Modifier::BOLD),
    )];
    push_right_aligned(
        &mut heading,
        navigator.badge.clone(),
        inner.width,
        theme::color(Token::TextMuted),
    );
    frame.render_widget(
        Paragraph::new(Line::from(heading)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let rows = Rect::new(
        inner.x,
        inner.y.saturating_add(1),
        inner.width,
        inner.height.saturating_sub(1),
    );
    // The groove comes out of the list's own width, so a row is never
    // drawn under the handle that would sit on top of it.
    // On the divider itself, not beside it. Beside it was two lines, and
    // adjacent was still two lines; drawn *on* it, the divider is the
    // line and the handle is the stretch of it that says where you are —
    // which is the only thing a scrollbar was ever adding.
    //
    // Sharing the column is what makes the two gestures separable rather
    // than ambiguous: the handle is grabbed to scroll, and the rest of
    // the line is grabbed to move the divider. What it costs is clicking
    // the empty groove to jump, which is the lesser of the two.
    let bar = Scrollbar::measure(
        Rect::new(
            area.right().saturating_sub(Scrollbar::width()),
            rows.y,
            Scrollbar::width(),
            rows.height,
        ),
        rows.height as usize,
        navigator.rows.len(),
    );
    let list = rows;
    let visible = list.height as usize;
    let settled = scroll.settled(navigator.anchor, navigator.rows.len(), visible);
    for (offset, row) in navigator
        .rows
        .iter()
        .skip(settled.first)
        .take(visible)
        .enumerate()
    {
        let rect = Rect::new(list.x, list.y + offset as u16, list.width, 1);
        match row {
            NavigatorRow::Group {
                id,
                name,
                depth,
                collapsed,
                icon,
            } => {
                let fold = theme::glyph(if *collapsed {
                    Symbol::ChevronCollapsed
                } else {
                    Symbol::ChevronExpanded
                });
                let mut spans = vec![
                    TextSpan::raw(" "),
                    TextSpan::raw("  ".repeat(*depth)),
                    TextSpan::styled(format!("{fold} "), theme::fg(Token::TextMuted)),
                ];
                spans.extend(row_icon(*icon));
                spans.push(TextSpan::styled(
                    name.clone(),
                    theme::fg(Token::TextSecondary),
                ));
                frame.render_widget(Paragraph::new(Line::from(spans)), rect);
                hits.push((rect, ViewHit::ToggleGroup(*id)));
            }
            NavigatorRow::Item {
                id,
                name,
                depth,
                marker,
                selected,
                icon,
            } => {
                let label_style = match (*selected, navigator.focused) {
                    (true, true) => Style::default()
                        .fg(theme::color(Token::TextBright))
                        .add_modifier(Modifier::BOLD),
                    (true, false) => theme::fg(Token::TextBright),
                    (false, _) => theme::fg(Token::TextInactive),
                };
                // The selected row is marked the way every other list in
                // the product marks its selection — the accent bar and the
                // selected surface the plugin list uses — rather than a
                // neutral lift that the diff beside it easily outshone.
                let mut spans = vec![
                    TextSpan::styled(
                        if *selected {
                            theme::glyph(Symbol::TreeColumnDivider)
                        } else {
                            " ".to_owned()
                        },
                        theme::fg(Token::Accent),
                    ),
                    TextSpan::raw("  ".repeat(*depth)),
                    styled(&Span {
                        text: format!("{} ", marker.text),
                        ..marker.clone()
                    }),
                ];
                spans.extend(row_icon(*icon));
                spans.push(TextSpan::styled(name.clone(), label_style));
                if *selected {
                    crate::ui::fill_row_bg(
                        &mut spans,
                        rect.width,
                        theme::color(Token::SurfaceSelected),
                    );
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), rect);
                hits.push((rect, ViewHit::SelectItem(*id)));
            }
        }
    }
    if let Some(bar) = bar {
        bar.render(frame, settled.first);
    }
    (settled, bar)
}

/// The content column: a heading, then as many lines as fit.
///
/// Also the one place a click inside the content means anything — every
/// drawn line pushes a [`ViewHit::SelectLine`] for the rows it occupies,
/// so an extension that puts a caret somewhere can be told where the
/// pointer wanted it without ever seeing a coordinate.
/// The [`Content::Lines`] a frame is drawing, borrowed together — they
/// arrive as one thing from the extension and are laid out as one thing
/// here, so they travel as one rather than as five arguments in an order
/// somebody has to keep.
struct Lines<'a> {
    heading: &'a str,
    scroll: u16,
    lines: &'a [ContentLine],
    total: usize,
    caret: Option<Caret>,
    modes: &'a [Mode],
}

fn render_lines(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    content_lines: Lines<'_>,
    hits: &mut Vec<(Rect, ViewHit)>,
) -> Option<Scrollbar> {
    let Lines {
        heading,
        scroll,
        lines,
        total,
        caret,
        modes,
    } = content_lines;
    frame.render_widget(
        Paragraph::new(TextSpan::styled(
            heading.to_owned(),
            theme::fg(Token::TextSecondary),
        )),
        Rect::new(
            area.x,
            area.y,
            area.width.saturating_sub(modes_width(modes)),
            1,
        ),
    );
    render_modes(frame, area, modes, hits);
    let body = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    // The overlay's own right padding, for the same reason the
    // navigator's groove sits in its panel's: flush against the edge
    // rather than a column short of it, and the content keeps its full
    // width because that column was never the content's.
    let bar = Scrollbar::measure(
        Rect::new(body.right(), body.y, Scrollbar::width(), body.height),
        body.height as usize,
        total,
    );
    let gutter = gutter_width(lines);
    // Unnumbered content is prose, and prose is the case the gutter was
    // silently paying for everywhere else.
    let inset = if gutter == 0 { PROSE_INSET } else { 0 };
    let content = if body.width > inset.saturating_mul(2) {
        Rect::new(
            body.x.saturating_add(inset),
            body.y,
            body.width.saturating_sub(inset.saturating_mul(2)),
            body.height,
        )
    } else {
        body
    };
    let text_width = text_width(content.width, gutter);
    let mut y = content.y;
    for (offset, line) in lines.iter().enumerate().skip(scroll as usize) {
        let height = line_height(line, content.width, gutter);
        if y.saturating_add(height) > content.bottom() {
            break;
        }
        let row = Rect::new(content.x, y, content.width, height);
        render_line(frame, row, line, gutter);
        // One hit per *visual* row, not per line: a wrapped line covers
        // several, and which one the pointer is on is half of where in
        // the text it landed. The cell offset here is the row's own
        // start; the caller adds the horizontal distance, which is the
        // only part of the answer that needs the pointer.
        for wrapped in 0..height {
            hits.push((
                Rect::new(row.x, row.y + wrapped, row.width, 1),
                ViewHit::PlaceCaret {
                    line: offset,
                    cell: wrapped as usize * text_width,
                },
            ));
        }
        if let Some(caret) = caret.filter(|caret| caret.line == offset) {
            render_caret(frame, row, line, caret.column, gutter);
        }
        y = y.saturating_add(height);
    }
    render_scrollbar(
        frame,
        bar,
        scroll as usize,
        hits,
        ViewHit::DragContentScrollbar,
    )
}

/// An empty surface, or one that failed: what is the matter, and — when
/// there is something to do about it — what to do.
///
/// Set a third of the way down rather than pinned to the top edge. A line
/// of text against the top-left corner reads as a document that got cut
/// off, which is the one thing an empty surface must not look like; the
/// same words with space above and below read as the state they are.
fn render_message(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    text: &str,
    hint: Option<&str>,
    colour: Color,
) {
    if area.height == 0 {
        return;
    }
    let mut lines = vec![Line::from(TextSpan::styled(
        text.to_owned(),
        Style::default().fg(colour).add_modifier(Modifier::BOLD),
    ))];
    if let Some(hint) = hint {
        lines.push(Line::from(""));
        lines.push(Line::from(TextSpan::styled(
            hint.to_owned(),
            theme::fg(Token::TextMuted),
        )));
    }
    let top = area.y.saturating_add(area.height / 3);
    let body = Rect::new(
        area.x,
        top,
        area.width,
        area.height.saturating_sub(area.height / 3),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        body,
    );
}

/// The mark a navigator row carries before its name.
///
/// The one place a [`RowIcon`] becomes a glyph: the extension said what
/// the row *is*, and the vocabulary says what that looks like under the
/// active theme. A set that draws none of them — every built-in one but
/// `nerd`, since plain Unicode has no folder mark that is not an emoji —
/// resolves to nothing, and nothing is what gets drawn, without a column
/// held open for it.
fn row_icon(icon: RowIcon) -> Option<TextSpan<'static>> {
    let symbol = match icon {
        RowIcon::None => return None,
        RowIcon::Directory => Symbol::FileDirectory,
        RowIcon::DirectoryOpen => Symbol::FileDirectoryOpen,
        RowIcon::File => Symbol::FileDefault,
        RowIcon::Code => Symbol::FileCode,
        RowIcon::Markup => Symbol::FileMarkup,
        RowIcon::Config => Symbol::FileConfig,
        RowIcon::Lock => Symbol::FileLock,
        RowIcon::Data => Symbol::FileData,
        RowIcon::Image => Symbol::FileImage,
        RowIcon::Archive => Symbol::FileArchive,
        RowIcon::Git => Symbol::FileGit,
        RowIcon::Legal => Symbol::FileLegal,
    };
    let glyph = theme::glyph(symbol);
    if glyph.trim().is_empty() {
        return None;
    }
    // Muted on purpose: the icon classifies, the name identifies, and an
    // icon drawn at the name's weight competes with the thing the reader
    // is actually scanning for.
    Some(TextSpan::styled(
        format!("{glyph} "),
        theme::fg(Token::TextDim),
    ))
}

/// How much of the heading row the mode control takes, so the heading
/// itself is drawn shorter rather than under it.
fn modes_width(modes: &[Mode]) -> u16 {
    if modes.is_empty() {
        return 0;
    }
    modes
        .iter()
        .map(|mode| TextSpan::raw(&mode.label).width() as u16 + 2 * MODE_PAD)
        .sum()
}

/// The ways the content can be shown, offered as a segmented control at
/// the end of its heading row.
///
/// A control rather than a hint, and drawn where the thing it changes is:
/// the same choice a key makes has to be one a pointer can make, or the
/// mode belongs to whoever read the keymap.
fn render_modes(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    modes: &[Mode],
    hits: &mut Vec<(Rect, ViewHit)>,
) {
    let mut x = area.right().saturating_sub(modes_width(modes));
    for (index, mode) in modes.iter().enumerate() {
        let width = TextSpan::raw(&mode.label).width() as u16 + 2 * MODE_PAD;
        let rect = Rect::new(x, area.y, width, 1);
        let (fill, ink) = match mode.active {
            true => (Token::SurfaceSelected, Token::TextBright),
            false => (Token::SurfaceBackground, Token::TextMuted),
        };
        let mut style = Style::default()
            .fg(theme::color(ink))
            .bg(theme::color(fill));
        if mode.active {
            style = style.add_modifier(Modifier::BOLD);
        }
        frame.render_widget(
            Paragraph::new(TextSpan::styled(
                format!(
                    "{pad}{}{pad}",
                    mode.label,
                    pad = " ".repeat(MODE_PAD as usize)
                ),
                style,
            )),
            rect,
        );
        hits.push((rect, ViewHit::SelectMode(index)));
        x = x.saturating_add(width);
    }
}

/// Finishes a [`ViewHit::PlaceCaret`] the last frame produced, using where
/// the pointer actually is.
///
/// The render knew which line a row belonged to and where that row began;
/// only the click knows how far along it landed. Splitting it this way is
/// what keeps the hit table one entry per drawn row instead of one per
/// cell — and the arithmetic stays here, with the layout that produced
/// `row`, rather than in the event loop.
pub(crate) fn caret_cell_at(row: Rect, cell: usize, column: u16) -> usize {
    cell + usize::from(column.saturating_sub(row.x.saturating_add(GUTTER_WIDTH)))
}

/// How many cells a line's text has, once the gutter has taken its share.
fn text_width(width: u16, gutter: u16) -> usize {
    usize::from(width.saturating_sub(gutter).max(1))
}

/// How wide the gutter is for these lines: nothing at all when none of
/// them is numbered.
///
/// A rendered document has no line numbers, and reserving the column
/// anyway indents the whole thing by seven cells of blank — which is
/// what the gutter looked like in preview, and what it cost was the
/// left margin of every paragraph.
fn gutter_width(lines: &[ContentLine]) -> u16 {
    let numbered = lines
        .iter()
        .any(|line| !line.number.is_empty() || !line.gutter.trim().is_empty());
    if numbered { GUTTER_WIDTH } else { 0 }
}

/// The caret, drawn by inverting the cell it sits on rather than by
/// drawing a mark into it.
///
/// A glyph rendered at the caret's position *replaces* the character
/// underneath, so the letter being edited is the one letter the person
/// cannot see — the caret eats exactly what the caret is pointing at.
/// Setting the cell's colours leaves the character where it is and makes
/// it the block cursor, which is what a terminal's own cursor does.
///
/// Drawn against the terminal's own cursor rather than with it: the
/// workspace client hides that for the whole session (a pane's PTY draws
/// its own), and turning it back on for one overlay would leave it
/// blinking over a pane the moment the overlay closes.
fn render_caret(
    frame: &mut ratatui::Frame<'_>,
    row: Rect,
    line: &ContentLine,
    column: usize,
    gutter: u16,
) {
    let width = text_width(row.width, gutter);
    let mut before = 0usize;
    let mut remaining = column;
    for span in &line.spans {
        for character in span.text.chars() {
            if remaining == 0 {
                break;
            }
            before += TextSpan::raw(character.to_string()).width().max(1);
            remaining -= 1;
        }
        if remaining == 0 {
            break;
        }
    }
    // A caret past the last character sits one cell beyond it, which is
    // where the next one will be typed.
    before += remaining;
    let x = row.x + gutter + (before % width) as u16;
    let y = row.y + (before / width) as u16;
    if y >= row.bottom() || x >= row.right() {
        return;
    }
    let cell = &mut frame.buffer_mut()[(x, y)];
    cell.set_bg(theme::color(Token::Accent));
    cell.set_fg(theme::color(Token::SurfaceBackground));
}

/// Draws the groove for a surface, and makes the whole of it the drag
/// target.
///
/// The handle alone would be the obvious target and the wrong one:
/// clicking above or below it is how a pointer says "go there", and a
/// drag that wanders off the handle has to keep working.
fn render_scrollbar(
    frame: &mut ratatui::Frame<'_>,
    bar: Option<Scrollbar>,
    first: usize,
    hits: &mut Vec<(Rect, ViewHit)>,
    hit: ViewHit,
) -> Option<Scrollbar> {
    let bar = bar?;
    bar.render(frame, first);
    hits.push((bar.track, hit));
    Some(bar)
}

fn line_height(line: &ContentLine, width: u16, gutter: u16) -> u16 {
    let content_width = text_width(width, gutter);
    let text_width: usize = line.spans.iter().map(|span| styled(span).width()).sum();
    (text_width.max(1).div_ceil(content_width)) as u16
}

/// One line: a gutter mark, one stable number column, then content wrapped
/// to the width that is left.
fn render_line(frame: &mut ratatui::Frame<'_>, area: Rect, line: &ContentLine, gutter: u16) {
    let (marker_style, background) = match line.tone {
        LineTone::Neutral => (theme::fg(Token::TextFaint), None),
        LineTone::Added => (
            theme::fg(Token::StateSuccess),
            Some(theme::color(Token::StateDiffAdded)),
        ),
        LineTone::Removed => (
            theme::fg(Token::StateDanger),
            Some(theme::color(Token::StateDiffRemoved)),
        ),
    };
    let mut content_spans: Vec<TextSpan<'static>> = line.spans.iter().map(styled).collect();
    if let Some(background) = background {
        for span in &mut content_spans {
            span.style = span.style.bg(background);
        }
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(gutter), Constraint::Min(1)])
        .split(area);
    if gutter > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                TextSpan::styled(format!("{} ", line.gutter), marker_style),
                TextSpan::styled(format!("{:>4} ", line.number), theme::fg(Token::TextDim)),
            ]))
            .style(
                Style::default().bg(background.unwrap_or(theme::color(Token::SurfaceBackground))),
            ),
            columns[0],
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(content_spans))
            .wrap(Wrap { trim: false })
            .style(
                Style::default().bg(background.unwrap_or(theme::color(Token::SurfaceBackground))),
            ),
        columns[1],
    );
}

/// A hairline top border plus the hint text directly under it — the same
/// shape `management::render_footer` uses.
fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, commands: &[Command]) {
    // The overlay is what is open, so its own scope is what a key would
    // resolve against — the same stack `Attach::scopes` builds.
    let scopes = [
        uze_keys::Scope::Global,
        uze_keys::Scope::Workspace,
        uze_keys::Scope::Code,
    ];
    let actions: Vec<uze_keys::Action> = commands.iter().copied().map(action_of).collect();
    frame.render_widget(Paragraph::new(crate::ui::hint_for(&scopes, &actions)), area);
}

/// What an extension's command means in the product's own vocabulary. The
/// inverse of the mapping the workspace client makes when it hands a key
/// down — kept here, beside the render that needs it, rather than in the
/// extension, which knows nothing of either.
fn action_of(command: Command) -> uze_keys::Action {
    match command {
        Command::Close => uze_keys::Action::Dismiss,
        Command::FocusNext => uze_keys::Action::FocusNext,
        Command::SelectNext => uze_keys::Action::SelectNext,
        Command::SelectPrevious => uze_keys::Action::SelectPrevious,
        Command::Collapse => uze_keys::Action::Collapse,
        Command::Expand => uze_keys::Action::Expand,
        Command::Activate => uze_keys::Action::Activate,
        Command::ScrollPageUp => uze_keys::Action::ScrollPageUp,
        Command::Edit => uze_keys::Action::EditFile,
        Command::TogglePreview => uze_keys::Action::TogglePreview,
        Command::Save => uze_keys::Action::SaveFile,
        Command::Delete => uze_keys::Action::DeleteFile,
        Command::ConfirmDelete => uze_keys::Action::ConfirmDelete,
        Command::CaretLeft => uze_keys::Action::CaretLeft,
        Command::CaretRight => uze_keys::Action::CaretRight,
        Command::CaretLineStart => uze_keys::Action::CaretLineStart,
        Command::CaretLineEnd => uze_keys::Action::CaretLineEnd,
        Command::Newline => uze_keys::Action::InsertNewline,
        Command::EraseBack => uze_keys::Action::EraseBack,
        Command::EraseForward => uze_keys::Action::EraseForward,
        // Typing has no single key to name, so a footer never lists it.
        Command::Type(_) => uze_keys::Action::EraseBack,
        Command::ScrollPageDown => uze_keys::Action::ScrollPageDown,
    }
}

fn push_right_aligned(spans: &mut Vec<TextSpan<'static>>, value: String, width: u16, color: Color) {
    let used: usize = spans.iter().map(TextSpan::width).sum();
    let value_width = value.chars().count();
    let gap = (width as usize).saturating_sub(used + value_width);
    if gap > 0 {
        spans.push(TextSpan::raw(" ".repeat(gap)));
        spans.push(TextSpan::styled(value, Style::default().fg(color)));
    }
}

/// Draws one extension [`Section`] into the rows it is given, and reports
/// what a click on each row would mean.
///
/// The counterpart to [`render`] for a section rather than a full frame,
/// and the reason it is here rather than in either sidebar: an extension
/// section is an extension surface, and both of them resolve colour,
/// eliding and hit rectangles in this one module. `dragging` is the host's
/// own state — whether the divider under the header is being pulled right
/// now — because the gesture belongs to the host, not to whoever the
/// section came from.
pub(crate) fn render_section(
    frame: &mut ratatui::Frame<'_>,
    section: &Section,
    rows: &mut crate::ui::Rows,
    dragging: bool,
    hits: &mut Vec<(Rect, ViewHit)>,
) {
    let Some(header_rect) = rows.next(1) else {
        return;
    };
    let fold = theme::glyph(if section.collapsed {
        Symbol::ChevronCollapsed
    } else {
        Symbol::ChevronExpanded
    });
    // Bold on a filled row: the one section header in a column of tree
    // rows, so it reads as a heading rather than as one more item.
    let mut spans = vec![
        TextSpan::styled(format!("{fold} "), theme::fg(Token::TextSecondary)),
        TextSpan::styled(
            section.title.clone(),
            Style::default()
                .fg(theme::color(Token::TextSecondary))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    crate::ui::push_trailing(
        &mut spans,
        header_rect.width,
        section.caption.text.clone(),
        color(section.caption.role),
    );
    // Filled only while there is something under it. A band across the
    // column says "this is a heading over content"; on a folded section
    // there is no content, and the band reads as a control of its own —
    // two of them stacked at the foot of the sidebar read as a toolbar.
    if !section.collapsed {
        crate::ui::fill_row_bg(
            &mut spans,
            header_rect.width,
            theme::color(Token::SurfaceRaised),
        );
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), header_rect);
    hits.push((header_rect, ViewHit::ToggleSection));
    if section.collapsed {
        return;
    }
    // The divider between the header and its rows doubles as the drag
    // handle, the way the sidebar's own border does — lit in the accent
    // while it is being dragged, same as that border.
    if section.resizable
        && let Some(handle_rect) = rows.next(1)
    {
        let hue = if dragging {
            theme::color(Token::Accent)
        } else {
            theme::color(Token::BorderFaint)
        };
        frame.render_widget(
            Paragraph::new(TextSpan::styled(
                theme::glyph(Symbol::TreeDivider).repeat(handle_rect.width as usize),
                Style::default().fg(hue),
            )),
            handle_rect,
        );
        hits.push((handle_rect, ViewHit::ResizeSection));
    }
    // Scrolled by whole rows, never past the page that ends on the last
    // one — so the section is always full when its content is.
    let visible = usize::from(rows.remaining());
    let first = section
        .scroll
        .min(section.rows.len().saturating_sub(visible));
    for (index, row) in section.rows.iter().enumerate().skip(first) {
        let Some(rect) = rows.next(1) else {
            break;
        };
        let marker_width = row.marker.text.chars().count() as u16 + 1;
        let trailing_width = row.trailing.text.chars().count() as u16;
        // The name gives way before the trailing value, and one column is
        // reserved for the gap `push_trailing` always leaves between them.
        let name_width = rect
            .width
            .saturating_sub(marker_width + 1 + trailing_width + crate::ui::TRAILING_PAD);
        let mut spans = vec![
            TextSpan::styled(
                format!("{} ", row.marker.text),
                Style::default().fg(color(row.marker.role)),
            ),
            TextSpan::styled(
                crate::ui::elide_tail(&row.name.text, name_width as usize),
                Style::default().fg(color(row.name.role)),
            ),
        ];
        crate::ui::push_trailing(
            &mut spans,
            rect.width,
            row.trailing.text.clone(),
            color(row.trailing.role),
        );
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, ViewHit::SelectItem(index)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use uze_extensions::view::{ContentLine, LineTone, Rgb};

    fn sample() -> View {
        View {
            title: vec![Span::new("demo", Role::Bright)],
            navigator: Some(Navigator {
                heading: "CHANGES".to_owned(),
                badge: "2".to_owned(),
                focused: true,
                anchor: Some(1),
                rows: vec![
                    NavigatorRow::Group {
                        id: 0,
                        name: "src/".to_owned(),
                        depth: 0,
                        collapsed: false,
                        icon: RowIcon::Directory,
                    },
                    NavigatorRow::Item {
                        id: 7,
                        name: "ui.rs".to_owned(),
                        depth: 1,
                        marker: Span::new("M", Role::Warning),
                        selected: true,
                        icon: RowIcon::Code,
                    },
                ],
            }),
            content: Content::Lines {
                caret: None,
                total: 1,
                heading: "DIFF · src/ui.rs".to_owned(),
                scroll: 0,
                lines: vec![ContentLine {
                    gutter: "+".to_owned(),
                    number: "12".to_owned(),
                    tone: LineTone::Added,
                    spans: vec![Span {
                        text: "let x = 1;".to_owned(),
                        role: Role::Default,
                        color: Some(Rgb(1, 2, 3)),
                        bold: false,
                        italic: false,
                    }],
                }],
            },
            footer: vec![Command::Close],
            modes: Vec::new(),
        }
    }

    fn draw(view: &View) -> (Vec<String>, Vec<(Rect, ViewHit)>) {
        let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    view,
                    frame.area(),
                    Some(24),
                    NavigatorScroll::default(),
                    &mut hits,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect();
        (rows, hits)
    }

    /// The whole point of the contract: an extension names a row, the host
    /// decides where it went, so the host is the only side that can answer
    /// a click.
    #[test]
    fn a_click_target_comes_from_what_the_host_drew() {
        let (rows, hits) = draw(&sample());
        // Not just any row mentioning the file: the content heading names
        // it too.
        let item_row = rows
            .iter()
            .position(|row: &String| row.contains("ui.rs") && !row.contains("DIFF"))
            .expect("the item is drawn") as u16;
        let hit = hits
            .iter()
            .find(|(_, hit)| matches!(hit, ViewHit::SelectItem(7)))
            .expect("the item is clickable by the id the extension gave it");
        assert_eq!(
            hit.0.y, item_row,
            "the hit must sit on the row the host actually drew"
        );
        assert!(
            hits.iter()
                .any(|(_, hit)| *hit == ViewHit::GrabNavigatorEdge),
            "the edge is one target for both of its jobs"
        );
        assert!(hits.iter().any(|(_, hit)| *hit == ViewHit::Close));
    }

    /// An extension says what a row *is*; the vocabulary says what that
    /// looks like. So the icon appears only where the active set draws
    /// one, and takes no column where it does not — which is every
    /// built-in set but `nerd`, because plain Unicode has no folder mark
    /// that is not an emoji.
    #[test]
    fn a_rows_icon_comes_from_the_set_and_takes_no_column_when_there_is_none() {
        // The active theme is process-wide, so a test that swaps it takes
        // a turn — otherwise this one's swap is another's flake.
        static SWAPPING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _turn = SWAPPING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let with = |id: &str| {
            let mut layers = vec![uze_theme::default_file()];
            layers.extend(uze_theme::glyph_set_file(id));
            let theme = uze_theme::resolve_stack(
                &uze_theme::Identity::from_file(id, uze_theme::default_file()),
                &layers,
            )
            .expect("resolves")
            .theme;
            uze_theme::set_active(theme);
            let (rows, _) = draw(&sample());
            rows.iter()
                // Not the content heading, which names the file too.
                .find(|row: &&String| row.contains("ui.rs") && !row.contains("DIFF"))
                .cloned()
                .expect("the item is drawn")
        };

        let plain = with("default");
        let nerd = with("nerd");
        uze_theme::set_active(uze_theme::default_theme().clone());

        let column_of = |row: &str, needle: &str| {
            row.find(needle)
                .map(|byte| row[..byte].chars().count())
                .expect("drawn")
        };
        assert_eq!(
            column_of(&nerd, "ui.rs") - column_of(&plain, "ui.rs"),
            2,
            "the nerd set's icon must take a column of its own:\n  {plain:?}\n  {nerd:?}"
        );
        assert!(
            nerd.chars().any(|c| ('\u{ea60}'..='\u{ec84}').contains(&c)),
            "no Codicon on the row: {nerd:?}"
        );
    }

    /// A rendered document is the one content with no gutter, and the
    /// gutter is where every other mode's left margin quietly came from.
    /// Without a margin of its own, a wrapped paragraph runs into both
    /// borders.
    #[test]
    fn unnumbered_content_is_inset_where_numbered_content_leans_on_its_gutter() {
        let prose = |text: &str| ContentLine {
            gutter: String::new(),
            number: String::new(),
            tone: LineTone::Neutral,
            spans: vec![Span {
                text: text.to_owned(),
                role: Role::Default,
                color: None,
                bold: false,
                italic: false,
            }],
        };

        let mut view = sample();
        let Content::Lines { lines, heading, .. } = &mut view.content else {
            unreachable!("the sample is Lines")
        };
        *lines = vec![prose("PROSE")];
        *heading = "HEADING".to_owned();
        let (rows, _) = draw(&view);

        // Measured against the heading rather than the frame, because the
        // heading is drawn at the content area's own left edge — so the
        // difference is the margin and nothing else. In *columns*: the
        // frame's own rules are multi-byte, so a byte offset is not where
        // the terminal put anything.
        let column_of = |needle: &str| -> usize {
            rows.iter()
                .find_map(|row: &String| row.find(needle).map(|byte| row[..byte].chars().count()))
                .unwrap_or_else(|| panic!("`{needle}` is drawn"))
        };
        assert_eq!(
            column_of("PROSE") - column_of("HEADING"),
            usize::from(PROSE_INSET),
            "prose must be inset from the edge its own heading sits on"
        );

        // And the numbered case is untouched — its gutter is the margin.
        let (numbered, _) = draw(&sample());
        let row = numbered
            .iter()
            .find(|row: &&String| row.contains("let x = 1;"))
            .expect("the code line is drawn");
        assert!(
            row.contains("12"),
            "the gutter still carries the number: {row:?}"
        );
    }

    /// A scrollbar is drawn only when there is something to scroll, and
    /// the column it takes comes out of the content rather than sitting
    /// on top of it.
    #[test]
    fn a_scrollbar_appears_only_when_the_content_outgrows_the_frame() {
        let mut view = sample();
        let Content::Lines { lines, total, .. } = &mut view.content else {
            unreachable!("the sample shows lines");
        };
        *total = lines.len();

        let (_rows, hits) = draw(&view);
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| *hit == ViewHit::DragContentScrollbar),
            "one line in a tall frame has nowhere to scroll to"
        );

        let Content::Lines { total, .. } = &mut view.content else {
            unreachable!("the sample shows lines");
        };
        *total = 500;
        let (_rows, hits) = draw(&view);
        let track = hits
            .iter()
            .find(|(_, hit)| *hit == ViewHit::DragContentScrollbar)
            .expect("five hundred lines in a short frame is a scrollbar")
            .0;
        assert_eq!(track.width, crate::ui::scrollbar::Scrollbar::width());
        assert!(
            track.height > 1,
            "the whole groove is the target, not just the handle"
        );
        // The complaint this answers: a groove a column short of the edge
        // and a divider beside it read as two controls arguing.
        let (_navigator, content, _footer) = content_columns(Rect::new(0, 0, 90, 14), Some(24));
        assert_eq!(
            track.x,
            content.right(),
            "the groove hugs the edge rather than leaving a gap beside it"
        );
    }

    /// A mode the keyboard can reach has to be one a pointer can reach,
    /// drawn where the thing it changes is.
    #[test]
    fn the_modes_a_surface_offers_are_a_control_on_its_heading_row() {
        let mut view = sample();
        view.modes = vec![
            Mode {
                label: "Preview".to_owned(),
                active: false,
            },
            Mode {
                label: "Source".to_owned(),
                active: true,
            },
        ];
        let (rows, hits) = draw(&view);

        let segments: Vec<(Rect, usize)> = hits
            .iter()
            .filter_map(|(rect, hit)| match hit {
                ViewHit::SelectMode(index) => Some((*rect, *index)),
                _ => None,
            })
            .collect();
        assert_eq!(
            segments.len(),
            2,
            "both are offered, not just the other one"
        );
        assert_eq!(segments[0].1, 0);
        assert!(
            segments[0].0.x < segments[1].0.x,
            "in the order the extension gave them"
        );

        let heading_row = rows
            .iter()
            .position(|row: &String| row.contains("DIFF"))
            .expect("the heading is drawn") as u16;
        assert_eq!(
            segments[0].0.y, heading_row,
            "beside the heading of what they change, not in the footer"
        );
        assert!(
            rows[heading_row as usize].contains("Preview")
                && rows[heading_row as usize].contains("Source"),
            "and both labels are legible: {}",
            rows[heading_row as usize]
        );
    }

    /// A surface with one way of showing itself offers no choice, and the
    /// heading gets the whole row back.
    #[test]
    fn a_surface_with_one_mode_draws_no_control() {
        let (_rows, hits) = draw(&sample());
        assert!(
            !hits
                .iter()
                .any(|(_, hit)| matches!(hit, ViewHit::SelectMode(_)))
        );
    }

    /// A rendered document has no line numbers, so it gets its left
    /// margin back rather than being indented by a column reserved for
    /// nothing.
    #[test]
    fn unnumbered_lines_are_not_indented_by_an_empty_gutter() {
        let numbered = [ContentLine {
            gutter: "+".to_owned(),
            number: "12".to_owned(),
            tone: LineTone::Added,
            spans: vec![Span::new("code", Role::Default)],
        }];
        let prose = [ContentLine {
            gutter: " ".to_owned(),
            number: String::new(),
            tone: LineTone::Neutral,
            spans: vec![Span::new("a paragraph", Role::Default)],
        }];

        assert_eq!(gutter_width(&numbered), GUTTER_WIDTH);
        assert_eq!(gutter_width(&prose), 0);
    }

    /// The caret marks the character it is on; it never replaces it.
    ///
    /// Drawing a mark into the cell is the obvious implementation and the
    /// wrong one: the letter being edited becomes the one letter the
    /// person cannot see. This is the test that says so.
    #[test]
    fn the_caret_marks_the_character_it_sits_on_without_hiding_it() {
        let mut view = sample();
        let Content::Lines { caret, lines, .. } = &mut view.content else {
            unreachable!("the sample shows lines");
        };
        *caret = Some(Caret { line: 0, column: 4 });
        let text = lines[0].spans[0].text.clone();
        let under_caret = text.chars().nth(4).expect("a character to sit on");

        let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &view,
                    frame.area(),
                    Some(24),
                    NavigatorScroll::default(),
                    &mut hits,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();

        let hit = hits
            .iter()
            .find(|(_, hit)| matches!(hit, ViewHit::PlaceCaret { line: 0, .. }))
            .expect("a drawn content row can be clicked to place the caret");
        let (x, y) = (hit.0.x + GUTTER_WIDTH + 4, hit.0.y);

        assert_eq!(
            buffer[(x, y)].symbol(),
            under_caret.to_string(),
            "the character under the caret is still on screen"
        );
        assert_eq!(
            buffer[(x, y)].bg,
            theme::color(Token::Accent),
            "and it is marked by inverting its cell"
        );
    }

    /// A click resolves to a text position through two halves that each
    /// know only their own side: the host counts cells from the row it
    /// drew, and the extension turns cells into characters.
    #[test]
    fn a_click_resolves_to_the_cell_it_landed_on() {
        let view = sample();
        let (_rows, hits) = draw(&view);
        let (rect, hit) = hits
            .iter()
            .find(|(_, hit)| matches!(hit, ViewHit::PlaceCaret { line: 0, .. }))
            .expect("the first content line is clickable");
        let ViewHit::PlaceCaret { cell, .. } = hit else {
            unreachable!("matched above");
        };

        assert_eq!(
            caret_cell_at(*rect, *cell, rect.x + GUTTER_WIDTH + 6),
            6,
            "six cells past the start of the text is six cells into the line"
        );
        assert_eq!(
            caret_cell_at(*rect, *cell, rect.x),
            0,
            "a click on the gutter belongs to the start of the line, not past it"
        );
    }

    /// Chrome resolves through the palette; content keeps the colour it
    /// brought. An extension that could paint its own chrome is an
    /// extension that drifts from the design system.
    #[test]
    fn chrome_uses_the_hosts_palette_and_content_keeps_its_own() {
        let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &sample(),
                    frame.area(),
                    Some(24),
                    NavigatorScroll::default(),
                    &mut Vec::new(),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        // By cell, never by byte offset: the border glyphs are multi-byte,
        // so a byte index into the joined row is not a column.
        let cell_at = |needle: &str| {
            (0..buffer.area.height).find_map(|row| {
                let cells: Vec<String> = (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol().to_owned())
                    .collect();
                let wanted: Vec<String> = needle
                    .chars()
                    .map(|character| character.to_string())
                    .collect();
                cells
                    .windows(wanted.len())
                    .position(|window| window == wanted.as_slice())
                    .map(|column| buffer[(column as u16, row)].clone())
            })
        };
        assert_eq!(
            cell_at("let x = 1;").unwrap().fg,
            Color::Rgb(1, 2, 3),
            "syntax colour is the extension's own data"
        );
        assert_eq!(
            cell_at("CHANGES").unwrap().fg,
            theme::color(Token::TextSecondary),
            "a heading is chrome, so it resolves through the palette"
        );
        assert_eq!(
            cell_at("M ui.rs").unwrap().bg,
            theme::color(Token::SurfaceSelected),
            "the selected row carries the surface every other list marks its selection with"
        );
        assert_eq!(
            cell_at("M ").unwrap().fg,
            theme::color(Token::StateWarning),
            "Role::Warning"
        );
    }

    /// Wrapping is the host's, so the row a long line occupies is too —
    /// this used to be asserted inside the extension, which could only
    /// guess at the column width.
    #[test]
    fn a_line_too_long_for_the_column_occupies_more_than_one_row() {
        let line = ContentLine {
            gutter: " ".to_owned(),
            number: "1".to_owned(),
            tone: LineTone::Neutral,
            spans: vec![Span::new("abcdefgh", Role::Default)],
        };
        assert_eq!(line_height(&line, GUTTER_WIDTH + 4, GUTTER_WIDTH), 2);
        assert_eq!(line_height(&line, GUTTER_WIDTH + 8, GUTTER_WIDTH), 1);
    }

    /// A group folds from its own row, and says so with the same mark the
    /// sidebar's sections use.
    #[test]
    fn a_group_row_is_a_fold_target() {
        let mut view = sample();
        if let Some(navigator) = view.navigator.as_mut()
            && let NavigatorRow::Group { collapsed, .. } = &mut navigator.rows[0]
        {
            *collapsed = true;
        }
        let (rows, hits) = draw(&view);
        let group_row = rows
            .iter()
            .position(|row: &String| row.contains("src/") && !row.contains("DIFF"))
            .expect("the group is drawn") as u16;
        let hit = hits
            .iter()
            .find(|(_, hit)| matches!(hit, ViewHit::ToggleGroup(0)))
            .expect("the group folds by the id the extension gave it");
        assert_eq!(hit.0.y, group_row);
        assert!(
            rows[group_row as usize].contains(&theme::glyph(Symbol::ChevronCollapsed)),
            "{:?}",
            rows[group_row as usize]
        );
    }

    fn tall_navigator(anchor: Option<usize>) -> View {
        View {
            navigator: Some(Navigator {
                anchor,
                rows: (0..40)
                    .map(|index| NavigatorRow::Item {
                        icon: RowIcon::None,
                        id: index,
                        name: format!("file-{index}.rs"),
                        depth: 0,
                        marker: Span::new("M", Role::Warning),
                        selected: Some(index) == anchor,
                    })
                    .collect(),
                ..sample().navigator.unwrap()
            }),
            ..sample()
        }
    }

    fn drawn_with(view: &View, scroll: NavigatorScroll) -> (Vec<String>, NavigatorScroll) {
        let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
        let mut settled = NavigatorScroll::default();
        terminal
            .draw(|frame| {
                settled = render(frame, view, frame.area(), Some(24), scroll, &mut Vec::new())
                    .navigator_scroll;
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect()
            })
            .collect();
        (rows, settled)
    }

    /// The list scrolls where the wheel put it, and comes back to the
    /// selection only when the selection moves — a wheel looking at rows
    /// far from it is not pulled back every frame.
    #[test]
    fn the_list_follows_the_anchor_only_when_it_changes() {
        let view = tall_navigator(Some(30));

        let (rows, settled) = drawn_with(&view, NavigatorScroll::default());
        assert!(
            rows.iter().any(|row| row.contains("file-30.rs")),
            "a new anchor is brought on screen: {rows:?}"
        );
        assert!(settled.first > 0);
        assert_eq!(settled.revealed, Some(30));

        let scrolled_away = NavigatorScroll {
            first: 0,
            ..settled
        };
        let (rows, settled) = drawn_with(&view, scrolled_away);
        assert!(
            rows.iter().any(|row| row.contains("file-0.rs")),
            "the same anchor does not pull the list back: {rows:?}"
        );
        assert_eq!(settled.first, 0);

        let (_, settled) = drawn_with(
            &view,
            NavigatorScroll {
                first: 500,
                ..settled
            },
        );
        assert!(
            settled.first < 40,
            "held to the rows that exist, so the wheel back is not a long way: {settled:?}"
        );
    }

    #[test]
    fn a_view_without_a_navigator_leaves_the_column_empty() {
        let view = View {
            navigator: None,
            content: Content::Message {
                text: "not a git repository".to_owned(),
                hint: None,
                role: Role::Danger,
            },
            ..sample()
        };
        let (rows, _) = draw(&view);
        assert!(rows.iter().any(|row| row.contains("not a git repository")));
        assert!(
            !rows.iter().any(|row| row.contains("CHANGES")),
            "nothing to navigate means no list, not an empty one: {rows:?}"
        );
    }
}
