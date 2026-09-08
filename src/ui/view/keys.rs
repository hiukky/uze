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
        let mut y = list_area.y;
        let mut heading = None;
        for (index, row) in rows.iter().enumerate() {
            if y >= list_area.bottom() {
                break;
            }
            if heading != Some(row.scope) {
                heading = Some(row.scope);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        row.scope.heading().to_uppercase(),
                        theme::fg_bold(Token::TextMuted),
                    )),
                    Rect::new(list_area.x, y, list_area.width, 1),
                );
                y += 1;
                if y >= list_area.bottom() {
                    break;
                }
            }
            let rect = Rect::new(list_area.x, y, list_area.width, 1);
            frame.render_widget(Paragraph::new(row_line(model, row, index, key_width)), rect);
            hits.push((rect, Hit::KeyRow(index)));
            y += 1;
        }
    }

    if let Some(row) = model.selected_key_row() {
        render_drawer(frame, area, drawer_width, model, &row, hits);
    }
}

/// One line: the key, what it does, and whether it is the operator's own
/// choice. An unbound action reads as unbound rather than as blank — the
/// difference between "no key" and "I have not scrolled to it" matters on
/// a screen whose whole subject is keys.
fn row_line(model: &TuiModel, row: &KeyRow, index: usize, key_width: usize) -> Line<'static> {
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
    let mut spans = vec![
        Span::styled(
            format!("{} ", theme::glyph(mark(selected))),
            theme::fg(if selected {
                Token::Accent
            } else {
                Token::TextDim
            }),
        ),
        Span::styled(key, theme::fg_bold(key_colour)),
        Span::styled("  ", theme::fg(Token::TextDim)),
        Span::styled(row.action.label(), label),
    ];
    if row.custom() {
        spans.push(Span::styled("  yours", theme::fg(Token::StateInfo)));
    }
    Line::from(spans)
}

fn mark(selected: bool) -> Symbol {
    if selected {
        Symbol::StatusSelected
    } else {
        Symbol::MarkDot
    }
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
