//! TUI view — Doctor route, and the severity classification it renders.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::application::DoctorReport;

use super::super::model::TuiModel;
use super::super::{DANGER, MUTED, SUCCESS, WARNING, panel_block};

pub(crate) fn render_doctor(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let mut lines = Vec::new();
    let issues = model.issues();
    if issues.is_empty() {
        lines.push(Line::from(Span::styled(
            "Healthy",
            Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("{} issue(s)", issues.len()),
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for severity in [Severity::High, Severity::Medium, Severity::Low] {
            let matching: Vec<&Issue> = issues.iter().filter(|i| i.severity == severity).collect();
            if matching.is_empty() {
                continue;
            }
            lines.push(Line::from(Span::styled(
                severity.label(),
                severity.style().add_modifier(Modifier::BOLD),
            )));
            for issue in matching {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(issue.message.clone()),
                ]));
            }
        }
    }
    if let Some(doctor) = &model.doctor {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "{} plugins  ·  {} harnesses  ·  {:?} store",
                doctor.plugins.len(),
                doctor.harnesses.len(),
                doctor.store
            ),
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Doctor "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

// --- Doctor severity classification -----------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) enum Severity {
    High,
    Medium,
    Low,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
        }
    }

    fn style(self) -> Style {
        match self {
            Severity::High => Style::default().fg(DANGER),
            Severity::Medium => Style::default().fg(WARNING),
            Severity::Low => Style::default().fg(MUTED),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Issue {
    pub(crate) severity: Severity,
    message: String,
}

/// Classifies `DoctorReport` findings into actionable severities. Deliberately
/// conservative about what counts as "verified": a matched receipt means
/// configuration agrees with what UZE expects, not that the harness has been
/// observed to actually work — see `docs/architecture/invariants.md`.
pub(crate) fn classify_doctor(doctor: Option<&DoctorReport>) -> Vec<Issue> {
    let Some(doctor) = doctor else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    if let Some(error) = &doctor.ledger_error {
        issues.push(Issue {
            severity: Severity::High,
            message: format!("Attachment ledger is unreadable: {error}"),
        });
    }
    if let Some(error) = &doctor.integration_state_error {
        issues.push(Issue {
            severity: Severity::High,
            message: format!("Integration state is unreadable: {error}"),
        });
    }
    if let Some(error) = &doctor.provisioning_state_error {
        issues.push(Issue {
            severity: Severity::High,
            message: format!("Provisioning state is unreadable: {error}"),
        });
    }
    for package in &doctor.attachments {
        let state = &package.state;
        if state.conflicts > 0 || state.blocked > 0 {
            issues.push(Issue {
                severity: Severity::High,
                message: format!(
                    "{}: {} conflict(s), {} blocked — needs manual resolution",
                    package.plugin, state.conflicts, state.blocked
                ),
            });
        }
        if state.drifted > 0 {
            issues.push(Issue {
                severity: Severity::Medium,
                message: format!(
                    "{}: {} attachment(s) drifted from what UZE expects",
                    package.plugin, state.drifted
                ),
            });
        }
        if state.missing > 0 {
            issues.push(Issue {
                severity: Severity::Low,
                message: format!(
                    "{}: {} attachment(s) missing",
                    package.plugin, state.missing
                ),
            });
        }
    }
    for harness in &doctor.harnesses {
        if harness.detection.present && harness.setup.contains("not configured") {
            issues.push(Issue {
                severity: Severity::Medium,
                message: format!(
                    "{} is installed but not configured — run setup",
                    harness.integration
                ),
            });
        }
    }
    for plugin in &doctor.plugins {
        if plugin.update_available == Some(true) {
            issues.push(Issue {
                severity: Severity::Low,
                message: format!("{}: update available", plugin.id),
            });
        }
    }
    issues.sort_by_key(|issue| issue.severity);
    issues
}
