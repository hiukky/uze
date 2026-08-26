//! TUI view — Doctor route, and the severity classification it renders.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::application::DoctorReport;
use crate::integration::AttachmentState;

use super::super::model::TuiModel;
use super::super::{ACCENT, BORDER_FAINT, DANGER, MUTED, TEXT_SECONDARY, WARNING};
use super::super::{content_area, render_screen_header};

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
            Paragraph::new(Span::styled(
                name,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
        for check in &checks {
            if y >= bottom {
                break;
            }
            y = render_check(frame, content, y, check);
        }
        y += 1;
    }
}

/// One checklist entry: the problem line (symbol, label, evidence), an
/// indented `→` fix line for anything that isn't passing, then a hairline
/// divider under whichever line came last. Returns the y after the divider.
fn render_check(frame: &mut ratatui::Frame<'_>, content: Rect, y: u16, check: &Check) -> u16 {
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
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(content.x, y, content.width, 1),
    );
    let mut y = y + 1;
    if let Some(solution) = check.solution
        && y < content.y + content.height
    {
        let solution_line = Line::from(vec![
            Span::raw("    "),
            Span::styled("→", Style::default().fg(ACCENT)),
            Span::raw(" "),
            Span::styled(solution.to_owned(), Style::default().fg(TEXT_SECONDARY)),
        ]);
        frame.render_widget(
            Paragraph::new(solution_line),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
    if y < content.y + content.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(content.width as usize),
                Style::default().fg(BORDER_FAINT),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
    y
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
        for hook in &package.hooks {
            // A hook whose route cannot preserve its declared semantics is
            // an honest, actionable finding (ADR-033 doctor spec) — never
            // hidden behind a healthy-native row.
            if let Some(loss) = &hook.weakened {
                issues.push(Issue {
                    severity: Severity::Medium,
                    message: format!(
                        "{}: hook `{}` on {} is {:?} — {loss}",
                        package.plugin, hook.hook, hook.harness, hook.route
                    ),
                });
            }
            match hook.state {
                Some(
                    AttachmentState::Drifted | AttachmentState::Conflict | AttachmentState::Blocked,
                ) => {
                    issues.push(Issue {
                        severity: Severity::High,
                        message: format!(
                            "{}: hook `{}` on {} attachment is {:?}",
                            package.plugin, hook.hook, hook.harness, hook.state
                        ),
                    });
                }
                Some(AttachmentState::Missing) => {
                    issues.push(Issue {
                        severity: Severity::Low,
                        message: format!(
                            "{}: hook `{}` on {} attachment missing",
                            package.plugin, hook.hook, hook.harness
                        ),
                    });
                }
                _ => {}
            }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

struct Check {
    label: String,
    /// The evidence — the concrete numbers/state behind the verdict.
    detail: String,
    status: CheckStatus,
    /// The fix this screen can actually point at: a key to press or a CLI
    /// command. `None` for a passing check.
    solution: Option<&'static str>,
}

fn doctor_groups(doctor: Option<&DoctorReport>) -> Vec<(&'static str, Vec<Check>)> {
    let Some(doctor) = doctor else {
        return Vec::new();
    };

    let damaged_store_solution =
        Some("Repair or remove the damaged state file under ~/.uze, then press r to recheck");
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
                solution: damaged_store_solution,
            },
            None => Check {
                label: format!("{label} readable"),
                detail: String::new(),
                status: CheckStatus::Pass,
                solution: None,
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
            solution: None,
        });
        if !configured {
            harnesses.push(Check {
                label: format!("{} not configured", harness.display_name),
                detail: "UZE hasn't set the harness up yet".to_owned(),
                status: CheckStatus::Warn,
                solution: Some("Press s on the Harnesses screen to run setup"),
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
                solution: Some("Update (u) or remove (r) the plugin in Plugins, then refresh"),
            });
        } else if state.drifted > 0 {
            plugins.push(Check {
                label: format!("{} drifted", package.plugin),
                detail: format!("{} attachment(s) drifted", state.drifted),
                status: CheckStatus::Warn,
                solution: Some("Press u in Plugins — updating re-applies the expected attachments"),
            });
        } else if state.missing > 0 {
            plugins.push(Check {
                label: format!("{} missing attachments", package.plugin),
                detail: format!("{} attachment(s) missing", state.missing),
                status: CheckStatus::Warn,
                solution: Some("Run uze setup (s on Harnesses) to re-attach them"),
            });
        } else {
            plugins.push(Check {
                label: format!("{} attached cleanly", package.plugin),
                detail: String::new(),
                status: CheckStatus::Pass,
                solution: None,
            });
        }
        // One row per hook attachment finding (ADR-033 doctor spec): the
        // plugin may be healthy overall while a specific hook on a specific
        // harness cannot preserve its declared semantics.
        for hook in &package.hooks {
            if hook.state == Some(AttachmentState::Missing) {
                plugins.push(Check {
                    label: format!(
                        "{} hook `{}` missing on {}",
                        package.plugin, hook.hook, hook.harness
                    ),
                    detail: format!("[{}] receipt not found in managed config", hook.event),
                    status: CheckStatus::Warn,
                    solution: Some("Press u in Plugins — updating re-applies the managed entry"),
                });
            }
            if let Some(loss) = &hook.weakened {
                plugins.push(Check {
                    label: format!(
                        "{} hook `{}` — {:?} on {}",
                        package.plugin, hook.hook, hook.route, hook.harness
                    ),
                    detail: format!("[{}] {loss}", hook.event),
                    status: CheckStatus::Warn,
                    solution: Some("Remove the plugin (r) or accept the stated limitation"),
                });
            }
        }
    }
    for plugin in &doctor.plugins {
        if plugin.update_available == Some(true) {
            plugins.push(Check {
                label: format!("{} update available", plugin.id),
                detail: "A newer marketplace revision is ready".to_owned(),
                status: CheckStatus::Warn,
                solution: Some("Press u in Plugins to update this plugin"),
            });
        }
    }

    vec![
        ("Store", store),
        ("Harnesses", harnesses),
        ("Plugins", plugins),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::application::{DoctorReport, ManagedStateSummary, PackageManagedState, StoreHealth};

    use super::{CheckStatus, doctor_groups};

    fn doctor_with_conflict() -> DoctorReport {
        DoctorReport {
            uze_home: PathBuf::from("/home"),
            store: StoreHealth::Ready,
            plugins: Vec::new(),
            harnesses: Vec::new(),
            attachments: vec![PackageManagedState {
                hooks: Vec::new(),
                plugin: "acme".to_owned(),
                state: ManagedStateSummary {
                    matched: 0,
                    missing: 1,
                    drifted: 0,
                    conflicts: 1,
                    blocked: 0,
                    ledger_error: None,
                },
            }],
            ledger_error: None,
            integration_state_error: None,
            provisioning_state_error: None,
        }
    }

    #[test]
    fn failing_checks_carry_solutions_and_passes_do_not() {
        let groups = doctor_groups(Some(&doctor_with_conflict()));
        let failing: Vec<&super::Check> = groups
            .iter()
            .flat_map(|(_, checks)| checks.iter())
            .filter(|check| check.status != CheckStatus::Pass)
            .collect();
        assert!(
            !failing.is_empty(),
            "the conflict doctor has failing checks"
        );
        assert!(
            failing.iter().all(|check| check.solution.is_some()),
            "every non-passing check needs an actionable solution"
        );
        let passing: Vec<&super::Check> = groups
            .iter()
            .flat_map(|(_, checks)| checks.iter())
            .filter(|check| check.status == CheckStatus::Pass)
            .collect();
        assert!(passing.iter().all(|check| check.solution.is_none()));
    }
}
