//! TUI view — Overview route.
//!
//! The machine dashboard: harnesses detected, plugins installed, marketplace
//! sources, and the global health line. Project-specific context belongs in
//! the dedicated project and harness views, not this machine-scoped overview.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::super::hit::Hit;
use super::super::model::{Focus, Route, TuiModel};
use super::super::{
    ACCENT, BORDER_FAINT, DANGER, MUTED, SELECTED_BG, SUCCESS, SURFACE_OVERLAY, TEXT_BRIGHT,
    TEXT_DIM, WARNING,
};
use super::super::{content_area, render_screen_header};
use super::health::Severity;

pub(crate) fn render_overview(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    let content = render_screen_header(frame, area, "Overview", "status & health", None);

    let harness_total = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let harness_detected = model.doctor.as_ref().map_or(0, |d| {
        d.harnesses.iter().filter(|h| h.detection.present).count()
    });
    let alerts = model.alerts();
    let mut y = content.y;

    // Status line: dot + headline + detail, matching the design's single
    // "All systems healthy — N harnesses detected, ..." summary row.
    let (color, headline) = if alerts.is_empty() {
        (SUCCESS, "All systems healthy")
    } else {
        (WARNING, "Attention needed")
    };
    let detail = if alerts.is_empty() {
        format!(
            "— {harness_detected} harness{} detected",
            if harness_detected == 1 { "" } else { "es" }
        )
    } else {
        format!("— {} item(s) need attention", alerts.len())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(
                headline,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(detail, Style::default().fg(MUTED)),
        ])),
        Rect::new(content.x, y, content.width, 1),
    );
    y += 3;

    // 3-column stat grid, each cell divided from its neighbor by a left
    // hairline border — the design's `border-left:1px solid rgba(...)`.
    let stats = [
        (
            "Harnesses detected",
            format!("{harness_detected}/{harness_total}"),
            TEXT_BRIGHT,
        ),
        (
            "Plugins installed",
            model.plugins.len().to_string(),
            TEXT_BRIGHT,
        ),
        (
            "Active profile",
            model
                .profiles
                .iter()
                .find(|profile| profile.active)
                .map_or_else(|| "none".to_owned(), |profile| profile.id.clone()),
            SUCCESS,
        ),
    ];
    if y + 1 < content.y + content.height {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(Rect::new(content.x, y, content.width, 2));
        for (cell, (label, value, color)) in columns.iter().zip(stats) {
            let block = Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(BORDER_FAINT));
            let inner = block.inner(*cell);
            frame.render_widget(block, *cell);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(inner);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {label}"),
                    Style::default().fg(MUTED),
                )),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {value}"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
                rows[1],
            );
        }
    }
    // Keep the activity stream visually separate from the compact stat cards.
    y += 5;

    // Recent prompts — the workspace-aware mini history. Always rendered
    // after the stat grid so it stays discoverable even when alerts exist;
    // alerts are pushed below it rather than replacing it.
    let bottom = content.y + content.height;
    if y < bottom {
        let header = format!(
            "Recent prompts — {}",
            if model.prompt_history.is_empty() {
                "no history yet".to_owned()
            } else {
                format!("{} recorded", model.prompt_history.len())
            }
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                header,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        // Breathing room between title and listing, matching the gap the
        // stat grid leaves above.
        y += 2;

        if model.prompt_history.is_empty() {
            if y < bottom {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        "Prompts submitted to an agent tab appear here. Click one to jump back to its tab.",
                        Style::default().fg(TEXT_DIM),
                    )),
                    Rect::new(content.x, y, content.width, 1),
                );
                y += 1;
            }
        } else {
            // Each entry is two rows plus a hairline divider between
            // neighbours, so n entries occupy 3n-1 rows. Two rows are held
            // back for the alerts section when there is one.
            let available = bottom.saturating_sub(y) as usize;
            let budget = if alerts.is_empty() {
                available
            } else {
                available.saturating_sub(2)
            };
            let entry_budget = ((budget + 1) / 3).max(1);

            for (index, entry) in model.prompt_history.iter().take(entry_budget).enumerate() {
                let rect = Rect::new(content.x, y, content.width, 2);
                if rect.y + rect.height > bottom {
                    break;
                }
                let selected = index == model.overview_prompt_selected
                    && model.focus == Focus::Content
                    && model.route == Route::Overview;
                let background = if selected {
                    Some(SELECTED_BG)
                } else if model.overview_prompt_hovered == Some(index) {
                    Some(SURFACE_OVERLAY)
                } else {
                    None
                };
                // One paint covers both rows edge to edge; the paragraphs
                // below carry no background of their own and compose over
                // it, so the row never has to be padded to the right edge.
                if let Some(background) = background {
                    frame.render_widget(
                        Block::default().style(Style::default().bg(background)),
                        rect,
                    );
                }

                let mut meta = Line::from(vec![
                    Span::styled("● ", Style::default().fg(ACCENT)),
                    Span::styled(entry.agent_binary.clone(), Style::default().fg(TEXT_BRIGHT)),
                    Span::styled(
                        format!(" · {}", entry.relative_time()),
                        Style::default().fg(TEXT_DIM),
                    ),
                    Span::styled(
                        format!(" · {}/{}", entry.space_label, entry.tab_label),
                        Style::default().fg(TEXT_DIM),
                    ),
                ]);
                let mut prompt = Line::from(Span::styled(
                    format!("  {}", entry.preview),
                    Style::default().fg(MUTED),
                ));
                let max = rect.width as usize;
                if meta.width() > max {
                    super::super::management::clip_line(&mut meta, max);
                }
                if prompt.width() > max {
                    super::super::management::clip_line(&mut prompt, max);
                }
                frame.render_widget(
                    Paragraph::new(meta),
                    Rect::new(rect.x, rect.y, rect.width, 1),
                );
                frame.render_widget(
                    Paragraph::new(prompt),
                    Rect::new(rect.x, rect.y + 1, rect.width, 1),
                );
                hits.push((rect, Hit::PromptHistory(index)));
                y += 2;

                // Hairline divider between entries — the same weight the
                // sidebar uses, separating without drawing a box.
                let last = index + 1 >= entry_budget.min(model.prompt_history.len());
                if !last && y < bottom {
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            "─".repeat(content.width as usize),
                            Style::default().fg(BORDER_FAINT),
                        )),
                        Rect::new(content.x, y, content.width, 1),
                    );
                    y += 1;
                }
            }
            if !alerts.is_empty() && y < bottom {
                y += 1;
            }
        }
    }

    if alerts.is_empty() {
        return;
    }
    if y < content.y + content.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Needs attention",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
    for alert in alerts
        .iter()
        .take((content.y + content.height).saturating_sub(y) as usize)
    {
        let (glyph, color) = match alert.severity {
            Severity::High => ("✕", DANGER),
            Severity::Medium => ("!", WARNING),
            Severity::Low => ("•", ACCENT),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::styled(alert.label.clone(), Style::default().fg(TEXT_BRIGHT)),
                Span::styled(format!(" — {}", alert.detail), Style::default().fg(MUTED)),
            ])),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
}
