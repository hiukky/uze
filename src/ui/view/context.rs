//! TUI view — Context route.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::application::HarnessContextDelivery;

use super::super::model::TuiModel;
use super::super::{MUTED, SUCCESS, WARNING, surface_block};
use super::overview::{portability_label, portability_style};

pub(crate) fn render_context(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Project",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  {}", model.context_root.display())),
        Line::from(""),
    ];
    match &model.context_status {
        None => lines.push(Line::from(Span::styled(
            "Press a to analyze this project.",
            Style::default().fg(MUTED),
        ))),
        Some(status) => {
            lines.push(Line::from(vec![
                Span::styled("Portability  ", Style::default().fg(MUTED)),
                Span::styled(
                    portability_label(&status.portability),
                    portability_style(Some(status)),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Canonical    ", Style::default().fg(MUTED)),
                Span::raw("AGENTS.md"),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Harnesses",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )));
            for harness in &status.harnesses {
                let delivery = match &harness.delivery {
                    HarnessContextDelivery::Native => "native".to_owned(),
                    HarnessContextDelivery::Bridge { state, .. } => {
                        format!("{state:?}").to_lowercase()
                    }
                    HarnessContextDelivery::NotDetected => "not detected".to_owned(),
                };
                lines.push(Line::from(format!(
                    "  {:<12} {delivery}",
                    harness.integration
                )));
            }
            if !status.warnings.is_empty() {
                lines.push(Line::from(""));
                for warning in &status.warnings {
                    lines.push(Line::from(Span::styled(
                        format!("! {warning}"),
                        Style::default().fg(WARNING),
                    )));
                }
            }
        }
    }
    if let Some(plan) = &model.context_plan {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if plan.has_changes() {
                "Plan: changes pending"
            } else {
                "Plan: nothing to apply"
            },
            Style::default().fg(if plan.has_changes() { WARNING } else { SUCCESS }),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(surface_block(" Context"))
            .wrap(Wrap { trim: true }),
        area,
    );
}
