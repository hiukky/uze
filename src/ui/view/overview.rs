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

use super::super::model::TuiModel;
use super::super::{ACCENT, BORDER_FAINT, DANGER, MUTED, SUCCESS, TEXT_BRIGHT, WARNING};
use super::super::{content_area, render_screen_header};
use super::health::Severity;

pub(crate) fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
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
