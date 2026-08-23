//! TUI view — Doctor route, and the severity classification it renders.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::application::DoctorReport;

use super::super::model::TuiModel;
use super::super::{ACCENT, DANGER, MUTED, WARNING};
use super::super::{content_area, render_divided_row, render_screen_header};

pub(crate) fn render_doctor(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let area = content_area(area);
    let issues = model.issues();
    let (summary, summary_color) = if issues.iter().any(|i| i.severity == Severity::High) {
        (
            format!(
                "{} high, {} total",
                issues
                    .iter()
                    .filter(|i| i.severity == Severity::High)
                    .count(),
                issues.len()
            ),
            DANGER,
        )
    } else if !issues.is_empty() {
        (format!("{} warning(s)", issues.len()), WARNING)
    } else {
        ("all checks passed".to_owned(), ACCENT)
    };
    let content = render_screen_header(
        frame,
        area,
        "Doctor",
        "diagnostics",
        Some(Span::styled(summary, Style::default().fg(summary_color))),
    );

    let groups = doctor_groups(model.doctor.as_ref());
    let mut y = content.y;
    let bottom = content.y + content.height;
    for (name, checks) in groups {
        if checks.is_empty() || y >= bottom {
            continue;
        }
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Span::styled(
                name,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
        for check in checks {
            if y >= bottom {
                break;
            }
            let (symbol, color) = match check.status {
                CheckStatus::Pass => ("✓", ACCENT),
                CheckStatus::Warn => ("!", WARNING),
                CheckStatus::Fail => ("✕", DANGER),
            };
            let line = Line::from(vec![
                Span::styled(format!("{symbol} "), Style::default().fg(color)),
                Span::styled(format!("{:<40}", check.label), Style::default().fg(color)),
                Span::styled(check.detail.clone(), Style::default().fg(MUTED)),
            ]);
            y = render_divided_row(frame, content, y, line);
        }
        y += 1;
    }
}

// --- Doctor severity classification -----------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug)]
pub(crate) struct Issue {
    pub(crate) severity: Severity,
    #[allow(dead_code)]
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

// --- Doctor screen's own full checklist (pass + fail) -----------------------
//
// `classify_doctor` above only surfaces problems (it backs the titlebar's
// "N issue(s)" count). The Doctor screen itself, like the design, shows a
// full checklist — passing checks included — grouped the same way the
// underlying `DoctorReport` is already organized: store-wide state, one row
// per harness, one row per plugin's attachment health and update state.

enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

struct Check {
    label: String,
    detail: String,
    status: CheckStatus,
}

fn doctor_groups(doctor: Option<&DoctorReport>) -> Vec<(&'static str, Vec<Check>)> {
    let Some(doctor) = doctor else {
        return Vec::new();
    };

    let mut store = Vec::new();
    for (label, error) in [
        ("Attachment ledger", &doctor.ledger_error),
        ("Integration state", &doctor.integration_state_error),
        ("Provisioning state", &doctor.provisioning_state_error),
    ] {
        store.push(match error {
            Some(error) => Check {
                label: format!("{label} unreadable"),
                detail: error.clone(),
                status: CheckStatus::Fail,
            },
            None => Check {
                label: format!("{label} readable"),
                detail: String::new(),
                status: CheckStatus::Pass,
            },
        });
    }

    let mut harnesses = Vec::new();
    for harness in &doctor.harnesses {
        if !harness.detection.present {
            continue;
        }
        let configured = !harness.setup.contains("not configured");
        harnesses.push(Check {
            label: format!("{} detected", harness.display_name),
            detail: harness
                .detection
                .version
                .clone()
                .map(|v| format!("v{v}"))
                .unwrap_or_default(),
            status: if configured {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn
            },
        });
        if !configured {
            harnesses.push(Check {
                label: format!("{} not configured", harness.display_name),
                detail: "Run uze setup".to_owned(),
                status: CheckStatus::Warn,
            });
        }
    }

    let mut plugins = Vec::new();
    for package in &doctor.attachments {
        let state = &package.state;
        if state.conflicts > 0 || state.blocked > 0 {
            plugins.push(Check {
                label: format!("{} has conflicts", package.plugin),
                detail: format!("{} conflict(s), {} blocked", state.conflicts, state.blocked),
                status: CheckStatus::Fail,
            });
        } else if state.drifted > 0 {
            plugins.push(Check {
                label: format!("{} drifted", package.plugin),
                detail: format!("{} attachment(s) drifted", state.drifted),
                status: CheckStatus::Warn,
            });
        } else if state.missing > 0 {
            plugins.push(Check {
                label: format!("{} missing attachments", package.plugin),
                detail: format!("{} attachment(s) missing", state.missing),
                status: CheckStatus::Warn,
            });
        } else {
            plugins.push(Check {
                label: format!("{} attached cleanly", package.plugin),
                detail: String::new(),
                status: CheckStatus::Pass,
            });
        }
    }
    for plugin in &doctor.plugins {
        if plugin.update_available == Some(true) {
            plugins.push(Check {
                label: format!("{} update available", plugin.id),
                detail: "Run uze update".to_owned(),
                status: CheckStatus::Warn,
            });
        }
    }

    vec![
        ("Store", store),
        ("Harnesses", harnesses),
        ("Plugins", plugins),
    ]
}
