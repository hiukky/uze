//! Actionable machine-health signals for the Overview.

use uze_application::AttachmentState;
use uze_application::application::DoctorReport;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Alert {
    pub(crate) severity: Severity,
    pub(crate) label: String,
    pub(crate) detail: String,
}

/// Projects the current health snapshot into only the problems an operator
/// can act on. Passing checks stay out of the Overview: their detailed state
/// already belongs to the Plugins and Integrations screens.
pub(crate) fn actionable_alerts(doctor: Option<&DoctorReport>) -> Vec<Alert> {
    let Some(doctor) = doctor else {
        return Vec::new();
    };
    let mut alerts = Vec::new();
    for (label, error) in [
        ("Attachment ledger unreadable", &doctor.ledger_error),
        (
            "Integration state unreadable",
            &doctor.integration_state_error,
        ),
        (
            "Provisioning state unreadable",
            &doctor.provisioning_state_error,
        ),
    ] {
        if let Some(detail) = error {
            alerts.push(Alert {
                severity: Severity::High,
                label: label.to_owned(),
                detail: detail.clone(),
            });
        }
    }
    for package in &doctor.attachments {
        let state = &package.state;
        if state.conflicts > 0 || state.blocked > 0 {
            alerts.push(Alert {
                severity: Severity::High,
                label: format!("{} needs attention", package.plugin),
                detail: format!("{} conflict(s), {} blocked", state.conflicts, state.blocked),
            });
        } else if state.drifted > 0 {
            alerts.push(Alert {
                severity: Severity::Medium,
                label: format!("{} attachments drifted", package.plugin),
                detail: format!("{} attachment(s) differ from UZE's record", state.drifted),
            });
        } else if state.missing > 0 {
            alerts.push(Alert {
                severity: Severity::Low,
                label: format!("{} attachments missing", package.plugin),
                detail: format!("{} attachment(s) need reattaching", state.missing),
            });
        }
        for hook in &package.hooks {
            if let Some(loss) = &hook.weakened {
                alerts.push(Alert {
                    severity: Severity::Medium,
                    label: format!("{} hook {} is approximated", package.plugin, hook.hook),
                    detail: loss.clone(),
                });
            }
            match hook.state {
                Some(
                    AttachmentState::Drifted | AttachmentState::Conflict | AttachmentState::Blocked,
                ) => {
                    alerts.push(Alert {
                        severity: Severity::High,
                        label: format!("{} hook {} needs attention", package.plugin, hook.hook),
                        detail: hook.event.clone(),
                    });
                }
                Some(AttachmentState::Missing) => alerts.push(Alert {
                    severity: Severity::Low,
                    label: format!("{} hook {} is missing", package.plugin, hook.hook),
                    detail: hook.event.clone(),
                }),
                _ => {}
            }
        }
    }
    for harness in &doctor.harnesses {
        if harness.detection.present && harness.setup.contains("not configured") {
            alerts.push(Alert {
                severity: Severity::Medium,
                label: format!("{} is not configured", harness.display_name),
                detail: "Open Integrations and run setup".to_owned(),
            });
        }
    }
    for plugin in &doctor.plugins {
        // Startup already applies every update it can settle on its own,
        // so one still standing here is one that needs a person: an
        // out-of-band source, new executable capability to confirm, or
        // managed state the update refused to disturb.
        if plugin.update_available == Some(true) {
            alerts.push(Alert {
                severity: Severity::Low,
                label: format!("{} update available", plugin.id),
                detail: "Open Plugins and press u to confirm it".to_owned(),
            });
        }
    }
    alerts.sort_by_key(|alert| alert.severity);
    alerts
}
