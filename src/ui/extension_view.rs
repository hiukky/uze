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
    Content, ContentLine, LineTone, Navigator, NavigatorRow, Role, ScrollTarget, Section, Size,
    Span, View, ViewHit,
};

use crate::ui::hint_spans;
use crate::ui::theme::{self, Symbol, Token};

/// Narrowest/widest the navigator can be dragged, and the floor left for
/// the content column — the same shape as the host TUI's own
/// `clamp_sidebar_width`, scoped to an extension overlay.
const MIN_NAVIGATOR_WIDTH: u16 = 20;
const MAX_NAVIGATOR_WIDTH: u16 = 50;
const MIN_EXTENSION_CONTENT_WIDTH: u16 = 40;

const GUTTER_WIDTH: u16 = 7;

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
    let inner = Rect::new(
        frame_area.x + 2,
        frame_area.y + 2,
        frame_area.width.saturating_sub(4),
        frame_area.height.saturating_sub(4),
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
pub(crate) fn render(
    frame: &mut ratatui::Frame<'_>,
    view: &View,
    area: Rect,
    navigator_width_override: Option<u16>,
    hits: &mut Vec<(Rect, ViewHit)>,
) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .title(view.title.clone())
            .title_style(theme::fg_bold(Token::Accent))
            .borders(Borders::ALL)
            .border_style(theme::fg(Token::BorderDefault))
            .padding(Padding::new(1, 1, 1, 1))
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
    // The navigator's own right border doubles as the resize handle — the
    // same shape as the sidebar's `ResizeSidebar` push in
    // `orchestrator::render`, whose drag arm lives there too.
    hits.push((
        Rect::new(
            navigator_area.right().saturating_sub(1),
            navigator_area.y,
            1,
            navigator_area.height,
        ),
        ViewHit::ResizeNavigator,
    ));

    if let Some(navigator) = &view.navigator {
        render_navigator(frame, navigator_area, navigator, hits);
    }
    match &view.content {
        Content::Message { text, role } => frame.render_widget(
            Paragraph::new(TextSpan::styled(
                text.clone(),
                Style::default().fg(color(*role)),
            )),
            content_area,
        ),
        Content::Lines {
            heading,
            scroll,
            lines,
        } => render_lines(frame, content_area, heading, *scroll, lines),
    }
    render_footer(frame, footer, &view.footer_hint);
}

fn render_navigator(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    navigator: &Navigator,
    hits: &mut Vec<(Rect, ViewHit)>,
) {
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

    let list = Rect::new(
        inner.x,
        inner.y.saturating_add(1),
        inner.width,
        inner.height.saturating_sub(1),
    );
    let visible = list.height as usize;
    let first = navigator.anchor.saturating_sub(visible.saturating_sub(1));
    for (offset, row) in navigator.rows.iter().skip(first).take(visible).enumerate() {
        let rect = Rect::new(list.x, list.y + offset as u16, list.width, 1);
        match row {
            NavigatorRow::Group { name, depth } => {
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        TextSpan::raw("  ".repeat(*depth)),
                        TextSpan::styled(name.clone(), theme::fg(Token::TextSecondary)),
                    ])),
                    rect,
                );
            }
            NavigatorRow::Item {
                id,
                name,
                depth,
                marker,
                selected,
            } => {
                let label_style = match (*selected, navigator.focused) {
                    (true, true) => Style::default()
                        .fg(theme::color(Token::TextBright))
                        .add_modifier(Modifier::BOLD),
                    (true, false) => theme::fg(Token::TextBright),
                    (false, _) => theme::fg(Token::TextInactive),
                };
                let mut spans = vec![
                    TextSpan::raw("  ".repeat(*depth)),
                    styled(&Span {
                        text: format!("{} ", marker.text),
                        ..marker.clone()
                    }),
                    TextSpan::styled(name.clone(), label_style),
                ];
                if *selected {
                    fill_row_bg(&mut spans, rect.width, theme::color(Token::SurfaceRaised));
                }
                frame.render_widget(Paragraph::new(Line::from(spans)), rect);
                hits.push((rect, ViewHit::SelectItem(*id)));
            }
        }
    }
}

fn render_lines(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    heading: &str,
    scroll: u16,
    lines: &[ContentLine],
) {
    frame.render_widget(
        Paragraph::new(TextSpan::styled(
            heading.to_owned(),
            theme::fg(Token::TextSecondary),
        )),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let content = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    let mut y = content.y;
    for line in lines.iter().skip(scroll as usize) {
        let height = line_height(line, content.width);
        if y.saturating_add(height) > content.bottom() {
            break;
        }
        render_line(frame, Rect::new(content.x, y, content.width, height), line);
        y = y.saturating_add(height);
    }
}

fn line_height(line: &ContentLine, width: u16) -> u16 {
    let content_width = width.saturating_sub(GUTTER_WIDTH).max(1) as usize;
    let text_width: usize = line.spans.iter().map(|span| styled(span).width()).sum();
    (text_width.max(1).div_ceil(content_width)) as u16
}

/// One line: a gutter mark, one stable number column, then content wrapped
/// to the width that is left.
fn render_line(frame: &mut ratatui::Frame<'_>, area: Rect, line: &ContentLine) {
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
        .constraints([Constraint::Length(GUTTER_WIDTH), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            TextSpan::styled(format!("{} ", line.gutter), marker_style),
            TextSpan::styled(format!("{:>4} ", line.number), theme::fg(Token::TextDim)),
        ]))
        .style(Style::default().bg(background.unwrap_or(theme::color(Token::SurfaceBackground)))),
        columns[0],
    );
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
fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, hint: &str) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme::fg(Token::BorderFaint));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Line::from(hint_spans(hint))), inner);
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

fn fill_row_bg(spans: &mut Vec<TextSpan<'static>>, width: u16, background: Color) {
    for span in spans.iter_mut() {
        span.style = span.style.bg(background);
    }
    let used: usize = spans.iter().map(TextSpan::width).sum();
    spans.push(TextSpan::styled(
        " ".repeat((width as usize).saturating_sub(used)),
        Style::default().bg(background),
    ));
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
    crate::ui::fill_row_bg(
        &mut spans,
        header_rect.width,
        theme::color(Token::SurfaceRaised),
    );
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
            title: " demo ".to_owned(),
            navigator: Some(Navigator {
                heading: "CHANGES".to_owned(),
                badge: "2".to_owned(),
                focused: true,
                anchor: 1,
                rows: vec![
                    NavigatorRow::Group {
                        name: "src/".to_owned(),
                        depth: 0,
                    },
                    NavigatorRow::Item {
                        id: 7,
                        name: "ui.rs".to_owned(),
                        depth: 1,
                        marker: Span::new("M", Role::Warning),
                        selected: true,
                    },
                ],
            }),
            content: Content::Lines {
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
                    }],
                }],
            },
            footer_hint: "esc close".to_owned(),
        }
    }

    fn draw(view: &View) -> (Vec<String>, Vec<(Rect, ViewHit)>) {
        let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, view, frame.area(), Some(24), &mut hits))
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
            hits.iter().any(|(_, hit)| *hit == ViewHit::ResizeNavigator),
            "the divider is draggable"
        );
        assert!(hits.iter().any(|(_, hit)| *hit == ViewHit::Close));
    }

    /// Chrome resolves through the palette; content keeps the colour it
    /// brought. An extension that could paint its own chrome is an
    /// extension that drifts from the design system.
    #[test]
    fn chrome_uses_the_hosts_palette_and_content_keeps_its_own() {
        let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
        terminal
            .draw(|frame| render(frame, &sample(), frame.area(), Some(24), &mut Vec::new()))
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
        assert_eq!(line_height(&line, GUTTER_WIDTH + 4), 2);
        assert_eq!(line_height(&line, GUTTER_WIDTH + 8), 1);
    }

    #[test]
    fn a_view_without_a_navigator_leaves_the_column_empty() {
        let view = View {
            navigator: None,
            content: Content::Message {
                text: "not a git repository".to_owned(),
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
