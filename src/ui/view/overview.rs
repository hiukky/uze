//! TUI view — Overview route.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::application::{Portability, ProjectContextStatus};

use super::super::model::TuiModel;
use super::super::{ACCENT, MUTED, SUCCESS, WARNING, panel_block};

pub(crate) fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let harness_total = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let harness_detected = model.doctor.as_ref().map_or(0, |d| {
        d.harnesses.iter().filter(|h| h.detection.present).count()
    });
    let portability = model
        .context_status
        .as_ref()
        .map(|status| portability_label(&status.portability))
        .unwrap_or("checking…");
    let issues = model.issues().len();

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{harness_detected}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("/{harness_total} harnesses detected")),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}", model.plugins.len()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(if model.plugins.len() == 1 {
                " plugin installed"
            } else {
                " plugins installed"
            }),
        ]),
        Line::from(vec![Span::raw(if model.marketplace_name.is_empty() {
            "Marketplace loading…".to_owned()
        } else {
            format!(
                "{} marketplace ready ({} plugins)",
                model.marketplace_name,
                model.marketplace_plugins.len()
            )
        })]),
        Line::from(vec![
            Span::raw("Current project: "),
            Span::styled(
                portability,
                portability_style(model.context_status.as_ref()),
            ),
        ]),
        Line::from(vec![if issues == 0 {
            Span::styled("No issues", Style::default().fg(SUCCESS))
        } else {
            Span::styled(
                format!("{issues} issue(s) — see Doctor"),
                Style::default().fg(WARNING),
            )
        }]),
        Line::from(""),
        Line::from(Span::styled(
            "Suggested",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from("  Tab → Marketplace   browse and install plugins"),
        Line::from("  Tab → Harnesses     manage detected harnesses"),
        Line::from("  Tab → Context       analyze this project"),
    ];
    if model.plugins.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No plugins installed yet — open Marketplace to install one.",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Overview "))
            .wrap(Wrap { trim: true }),
        area,
    );
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
