//! TUI view — Overview route.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::application::{HarnessContextDelivery, Portability, ProjectContextStatus};

use super::super::model::TuiModel;
use super::super::{BORDER_FAINT, MUTED, SUCCESS, TEXT_BRIGHT, WARNING};
use super::super::{content_area, render_screen_header};

pub(crate) fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let area = content_area(area);
    let content = render_screen_header(frame, area, "Overview", "status & health", None);

    let harness_total = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let harness_detected = model.doctor.as_ref().map_or(0, |d| {
        d.harnesses.iter().filter(|h| h.detection.present).count()
    });
    let issues = model.issues().len();
    let bridges = model
        .context_status
        .as_ref()
        .map(|status| {
            status
                .harnesses
                .iter()
                .filter(|h| !matches!(h.delivery, HarnessContextDelivery::NotDetected))
                .count()
        })
        .unwrap_or(0);

    let mut y = content.y;

    // Status line: dot + headline + detail, matching the design's single
    // "All systems healthy — N harnesses detected, ..." summary row.
    let (color, headline) = if issues == 0 {
        (SUCCESS, "All systems healthy")
    } else {
        (WARNING, "Attention needed")
    };
    let detail = if issues == 0 {
        format!(
            "— {harness_detected} harness{} detected, context bridges verified",
            if harness_detected == 1 { "" } else { "es" }
        )
    } else {
        format!("— {issues} issue(s), see Doctor")
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

    // 4-column stat grid, each cell divided from its neighbor by a left
    // hairline border — the design's `border-left:1px solid rgba(...)`.
    let stats = [
        (
            "Harnesses detected",
            format!("{harness_detected}/{harness_total}"),
        ),
        ("Plugins installed", model.plugins.len().to_string()),
        ("Marketplace sources", model.marketplace_count.to_string()),
        ("Context bridges", bridges.to_string()),
    ];
    if y + 1 < content.y + content.height {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25); 4])
            .split(Rect::new(content.x, y, content.width, 2));
        for (cell, (label, value)) in columns.iter().zip(stats) {
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
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )),
                rows[1],
            );
        }
        y += 4;
    }

    // Recent activity — the design shows a fabricated event log; the real
    // app tracks no persisted activity history (only the last mutation's
    // status message, which doesn't survive a refresh), so this stays
    // honest about having nothing to show rather than inventing entries.
    if y < content.y + content.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "RECENT ACTIVITY",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 3;
    }
    if y < content.y + content.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No activity tracked in this session yet.",
                Style::default().fg(MUTED),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
    }
}

pub(crate) fn portability_label(portability: &Portability) -> &'static str {
    match portability {
        Portability::NoContext => "no context",
        Portability::Portable => "portable",
        Portability::PartiallyPortable { .. } => "partially portable",
        Portability::VendorLocked { .. } => "vendor-locked",
    }
}

pub(crate) fn portability_style(status: Option<&ProjectContextStatus>) -> Style {
    match status.map(|s| &s.portability) {
        Some(Portability::Portable) => Style::default().fg(SUCCESS),
        Some(Portability::NoContext) => Style::default().fg(MUTED),
        Some(_) => Style::default().fg(WARNING),
        None => Style::default().fg(MUTED),
    }
}
