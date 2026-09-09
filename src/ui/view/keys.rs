//! TUI view — Keys route.
//!
//! The keyboard, as a thing you can look at. Every action, where it is
//! live, the key that reaches it, and — the part no keymap file can tell
//! you — whether the terminal in front of you can actually send that key.
//!
//! Shaped like Plugins on purpose: a clickable search field, a list
//! grouped by heading, a detail drawer. It is a screen someone visits once
//! and then rarely; making it look like a screen they already know is
//! worth more than any arrangement of its own.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use uze_keys::CaveatKind;

use super::super::hit::Hit;
use super::super::model::{KeyRow, ResizablePanel, TuiModel};
use super::super::{content_area, render_screen_header, side_panel_area};
use crate::ui::theme::{self, Symbol, Token};

pub(crate) fn render_keys(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    let content = render_screen_header(frame, area, "Keys", "what each key does", None);

    let filter_area = Rect::new(content.x, content.y, content.width, 2);
    hits.push((filter_area, Hit::FocusFilter));
    render_filter_box(frame, filter_area, model);

    let drawer_width = model
        .keys_drawer_width
        .unwrap_or(super::DRAWER_DEFAULT_WIDTH);
    let list_width = content.width.saturating_sub(drawer_width);
    let list_area = Rect::new(
        content.x,
        content.y + 3,
        list_width,
        content.height.saturating_sub(3),
    );

    let rows = model.key_rows();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("Nothing matches \"{}\".", model.keys_filter.trim()),
                theme::fg(Token::TextMuted),
            )),
            list_area,
        );
    } else {
        let key_width = rows
            .iter()
            .filter_map(|row| row.chord.map(|chord| chord.to_string().chars().count()))
            .max()
            .unwrap_or(0)
            .max(6);
        // Three columns rather than two words and an empty half-screen:
        // what the key is, what it is called, and the sentence that says
        // what it does. The sentence was only in the drawer, which made
        // the list a set of labels you had to open one at a time to read.
        let label_width = rows
            .iter()
            .map(|row| row.action.label().chars().count())
            .max()
            .unwrap_or(0);
        // The badge's column is reserved for the whole list or for none of
        // it, so the sentences end where each other ends.
        let yours_width = rows.iter().any(KeyRow::custom).then_some(YOURS_COLUMN);
        // The list laid out before any of it is drawn, because both the
        // spacing and the scroll are properties of the whole thing: a
        // group opens with a blank line, and the window has to be able to
        // count entries it is about to skip.
        let mut entries: Vec<Entry> = Vec::new();
        let mut heading = None;
        for (index, row) in rows.iter().enumerate() {
            if heading != Some(row.scope) {
                heading = Some(row.scope);
                // A heading pressed against the previous group's last key
                // belongs to neither of them.
                if !entries.is_empty() {
                    entries.push(Entry::Gap);
                }
                entries.push(Entry::Heading(row.scope));
            }
            entries.push(Entry::Row(index, *row));
        }

        // The window follows the selection rather than being scrolled on
        // its own: this list is long enough that everything past the first
        // screenful used to be invisible and unreachable at the same time
        // — the selection walked off the bottom and nothing followed it.
        // It stays at the top until the selection passes the middle, so
        // reading down from the first row does not move the page.
        // A column for the track, but only when there is more list than
        // screen: a scrollbar on a list that fits says the opposite of
        // what it is for.
        let track_width = theme::width(Symbol::BarThick)
            .max(theme::width(Symbol::BarThin))
            .max(1);
        let height = usize::from(list_area.height);
        let track = (entries.len() > height).then(|| {
            Rect::new(
                list_area.right().saturating_sub(track_width),
                list_area.y,
                track_width,
                list_area.height,
            )
        });
        let list_area = match track {
            Some(_) => Rect::new(
                list_area.x,
                list_area.y,
                list_area.width.saturating_sub(track_width + 1),
                list_area.height,
            ),
            None => list_area,
        };
        let anchor = entries
            .iter()
            .position(
                |entry| matches!(entry, Entry::Row(index, _) if *index == model.keys_selected),
            )
            .unwrap_or(0);
        let first = anchor
            .saturating_sub(height.saturating_sub(1) / 2)
            .min(entries.len().saturating_sub(height));

        for (offset, entry) in entries.iter().skip(first).take(height).enumerate() {
            let y = list_area.y + offset as u16;
            match entry {
                Entry::Gap => {}
                Entry::Heading(scope) => frame.render_widget(
                    Paragraph::new(Span::styled(
                        scope.heading().to_uppercase(),
                        theme::fg_bold(Token::TextMuted),
                    )),
                    Rect::new(list_area.x, y, list_area.width, 1),
                ),
                Entry::Row(index, row) => {
                    let rect = Rect::new(list_area.x, y, list_area.width, 1);
                    frame.render_widget(
                        Paragraph::new(row_line(
                            model,
                            row,
                            *index,
                            Columns {
                                key: key_width,
                                label: label_width,
                                yours: yours_width,
                                width: list_area.width,
                            },
                        )),
                        rect,
                    );
                    hits.push((rect, Hit::KeyRow(*index)));
                }
            }
        }

        if let Some(track) = track {
            render_track(frame, track, first, height, entries.len());
            hits.push((track, Hit::KeysTrack(track)));
        }
    }

    if let Some(row) = model.selected_key_row() {
        render_drawer(frame, area, drawer_width, model, &row, hits);
    }
}

/// A line of the list, before it is a line on screen: the blank that opens
/// a group, the group's own name, or one key under it. Laying the list out
/// as entries first is what lets the window skip whole groups without
/// having to redraw them to find out how tall they were.
enum Entry {
    Gap,
    Heading(uze_keys::Scope),
    Row(usize, KeyRow),
}

/// One line: the key, what it does, and whether it is the operator's own
/// choice. An unbound action reads as unbound rather than as blank — the
/// difference between "no key" and "I have not scrolled to it" matters on
/// a screen whose whole subject is keys.
fn row_line(model: &TuiModel, row: &KeyRow, index: usize, columns: Columns) -> Line<'static> {
    let key_width = columns.key;
    let selected = index == model.keys_selected;
    let capturing = selected && model.keys_capture;
    let key = if capturing {
        format!("{:<key_width$}", "press…")
    } else {
        match row.chord {
            Some(chord) => format!("{:<key_width$}", chord.to_string()),
            None => format!("{:<key_width$}", "—"),
        }
    };
    let key_colour = if capturing {
        Token::StateWarning
    } else if row.chord.is_none() {
        Token::TextDim
    } else {
        Token::Accent
    };
    let mut label = Style::default().fg(theme::color(if selected {
        Token::TextBright
    } else if row.action.destructive() {
        Token::StateDanger
    } else {
        Token::TextPrimary
    }));
    if selected {
        label = label.add_modifier(Modifier::BOLD);
    }
    let label_text = row.action.label();
    // Indented under the heading, which is what makes a heading read as
    // one rather than as another row in a different colour. The marker's
    // column is reserved on every row and carried on one — the shape the
    // Overview's own list uses, where a bullet against every entry was
    // texture the eye had to look past to find the one that mattered.
    let marker = if selected {
        Span::styled(
            format!("{} ", theme::glyph(Symbol::Prompt)),
            theme::fg(Token::Accent),
        )
    } else {
        Span::raw(" ".repeat(marker_width()))
    };
    let mut spans = vec![
        Span::raw(INDENT),
        marker,
        Span::styled(key, theme::fg_bold(key_colour)),
        Span::styled(GUTTER, theme::fg(Token::TextDim)),
        Span::styled(
            format!("{label_text:<width$}", width = columns.label),
            label,
        ),
    ];
    let used = INDENT.len() + marker_width() + key_width + GUTTER.len() + columns.label;
    let room = usize::from(columns.width)
        .saturating_sub(used + GUTTER.len() + columns.yours.unwrap_or(0) + INDENT.len());
    // A sentence only when there is room for one worth reading — half of
    // one, clipped at some arbitrary column, says less than the label
    // already did.
    if room >= 24 {
        let mut sentence = Line::from(Span::styled(
            row.action.description(),
            theme::fg(Token::TextMuted),
        ));
        crate::ui::management::clip_line(&mut sentence, room);
        spans.push(Span::styled(GUTTER, theme::fg(Token::TextDim)));
        spans.extend(sentence.spans);
    }
    if let Some(reserved) = columns.yours {
        // Pinned to its own column rather than trailing whatever the row
        // happened to end with: a column that starts wherever the last
        // word ended is not one.
        let drawn: usize = spans.iter().map(|span| span.width()).sum();
        let pad = usize::from(columns.width)
            .saturating_sub(drawn + reserved + INDENT.len())
            .max(1);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(
            if row.custom() { "yours" } else { "     " },
            theme::fg(Token::StateInfo),
        ));
    }
    // The selected row is a filled band the width of the list, not a
    // brighter word inside it — the same treatment every other list in
    // this mode gives its selection, and the reason one is findable
    // without reading it.
    if selected {
        for span in &mut spans {
            span.style = span.style.bg(theme::color(Token::SurfaceSelected));
        }
        let drawn: usize = spans.iter().map(|span| span.width()).sum();
        spans.push(Span::styled(
            " ".repeat(usize::from(columns.width).saturating_sub(drawn)),
            theme::bg(Token::SurfaceSelected),
        ));
    }
    Line::from(spans)
}

/// How far a key sits in from its group's name, and how far each column
/// sits from the one before it. Both were one space narrower, which read
/// as one block of text rather than as columns.
const INDENT: &str = "  ";
const GUTTER: &str = "   ";
/// What the "yours" badge occupies when any row in the list carries one.
const YOURS_COLUMN: usize = 5;

/// Where each of a row's columns ends. Measured once for the whole list —
/// a column measured per row is not a column.
#[derive(Clone, Copy)]
struct Columns {
    key: usize,
    label: usize,
    yours: Option<usize>,
    width: u16,
}

/// Where in the list the window sits, as a bar down the right edge.
///
/// It answers the question a long list cannot answer by itself — whether
/// there is more, and how much — and it takes the answer back: a click
/// jumps there and a drag keeps jumping, which is the gesture anyone who
/// has seen a scrollbar tries first. Drawing something that looks like a
/// control and does nothing is worse than not drawing it.
///
/// It moves the selection rather than a scroll offset of its own, because
/// the window is derived from the selection — so there is no second notion
/// of where the page is that could disagree with the first.
fn render_track(
    frame: &mut ratatui::Frame<'_>,
    track: Rect,
    first: usize,
    shown: usize,
    total: usize,
) {
    let height = usize::from(track.height);
    if height == 0 || total == 0 {
        return;
    }
    // The thumb is the visible fraction, floored at one row so it never
    // vanishes on a list long enough to make the fraction round to
    // nothing — which is exactly the list that most needs it.
    let thumb = (shown * height / total).clamp(1, height);
    let travel = height - thumb;
    let span = total.saturating_sub(shown);
    let top = (first * travel).checked_div(span).unwrap_or(0);
    for row in 0..height {
        let here = row >= top && row < top + thumb;
        frame.render_widget(
            Paragraph::new(Span::styled(
                theme::glyph(if here {
                    Symbol::BarThick
                } else {
                    Symbol::BarThin
                }),
                theme::fg(if here {
                    Token::BorderDefault
                } else {
                    Token::BorderFaint
                }),
            )),
            Rect::new(track.x, track.y + row as u16, track.width, 1),
        );
    }
}

/// The marker column: the glyph plus the space after it. Reserved on every
/// row, so a selection changes what is in the column and never where the
/// keys after it start.
fn marker_width() -> usize {
    usize::from(theme::width(Symbol::Prompt)) + 1
}

fn render_filter_box(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(if model.filtering {
            theme::color(Token::Accent)
        } else {
            theme::color(Token::BorderDefault)
        }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = if model.keys_filter.is_empty() {
        Line::from(Span::styled(
            "Filter keys and actions…",
            theme::fg(Token::TextMuted),
        ))
    } else {
        let mut spans = vec![Span::styled(
            model.keys_filter.clone(),
            theme::fg(Token::TextPrimary),
        )];
        if model.filtering {
            spans.push(Span::styled(
                theme::glyph(Symbol::BarThin),
                theme::fg(Token::Accent),
            ));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(text), inner);
}

/// The detail half: what this action does, what its key costs, and what
/// this particular terminal will do with it.
fn render_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    width: u16,
    model: &TuiModel,
    row: &KeyRow,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let drawer = side_panel_area(area, width);
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::KeysDrawer) {
                    theme::color(Token::Accent)
                } else {
                    theme::color(Token::SurfaceRecessed)
                },
            ))
            .style(theme::bg(Token::SurfaceRecessed)),
        drawer,
    );
    hits.insert(
        0,
        (
            Rect::new(drawer.x, drawer.y, 1, drawer.height),
            Hit::ResizePanel(ResizablePanel::KeysDrawer),
        ),
    );

    let inner = Rect::new(
        drawer.x + 2,
        drawer.y + 1,
        drawer.width.saturating_sub(3),
        drawer.height.saturating_sub(2),
    );
    let mut lines = vec![
        Line::from(Span::styled("ACTION", theme::fg_bold(Token::TextMuted))),
        Line::from(Span::styled(
            row.action.label(),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            row.action.description(),
            theme::fg(Token::TextSecondary),
        )),
        Line::from(""),
        Line::from(Span::styled("WHERE", theme::fg_bold(Token::TextMuted))),
        Line::from(Span::styled(
            row.scope.heading().to_owned(),
            theme::fg(Token::TextSecondary),
        )),
        Line::from(""),
        Line::from(Span::styled("KEY", theme::fg_bold(Token::TextMuted))),
    ];
    match row.chord {
        Some(chord) => {
            lines.push(Line::from(vec![
                Span::styled(chord.to_string(), theme::fg_bold(Token::Accent)),
                Span::styled(
                    format!(
                        "  {} {}",
                        theme::glyph(Symbol::EmDash),
                        chord.tier().label()
                    ),
                    theme::fg(Token::TextMuted),
                ),
            ]));
            if !model.keyboard.can_deliver(chord.tier()) {
                lines.push(Line::from(Span::styled(
                    "this terminal cannot send it — it will never arrive",
                    theme::fg(Token::StateDanger),
                )));
            }
            for caveat in chord.caveats() {
                let colour = match caveat.kind {
                    CaveatKind::HostClaims => Token::StateWarning,
                    CaveatKind::PaneLoses => Token::TextMuted,
                };
                lines.push(Line::from(Span::styled(
                    caveat.note.to_owned(),
                    theme::fg(colour),
                )));
            }
        }
        None => lines.push(Line::from(Span::styled(
            "no key — it is offered where it acts, and in the index",
            theme::fg(Token::TextDim),
        ))),
    }
    if row.custom() {
        lines.push(Line::from(Span::styled(
            match row.default_chord {
                Some(chord) => format!("uze ships with {chord}"),
                None => "uze ships with no key for this".to_owned(),
            },
            theme::fg(Token::TextDim),
        )));
    }
    if let Some(problem) = &model.keys_problem {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            problem.clone(),
            theme::fg(Token::StateDanger),
        )));
    }
    if let Some(probe) = &model.keys_probe {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "THIS TERMINAL",
            theme::fg_bold(Token::TextMuted),
        )));
        lines.push(Line::from(Span::styled(
            probe.clone(),
            theme::fg(Token::TextSecondary),
        )));
    }
    let body_height = inner.height.saturating_sub(3);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y, inner.width, body_height),
    );

    // The two things this screen is for, as targets rather than as keys —
    // a screen about rebinding that could only be driven by the bindings
    // it is rebinding would be a joke on itself.
    let actions = Rect::new(inner.x, inner.y + body_height, inner.width, 3);
    let rebind = Rect::new(actions.x, actions.y, actions.width, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(
            if model.keys_capture {
                "press the key you want, or esc"
            } else {
                "change this key"
            },
            theme::fg_bold(if model.keys_capture {
                Token::StateWarning
            } else {
                Token::Accent
            }),
        )),
        rebind,
    );
    hits.push((rebind, Hit::CaptureKey));
    if row.custom() {
        let reset = Rect::new(actions.x, actions.y + 1, actions.width, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "put back what uze ships with",
                theme::fg(Token::TextMuted),
            )),
            reset,
        );
        hits.push((reset, Hit::ResetKey));
    }
}
