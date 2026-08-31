//! TUI view — Profiles route.
//!
//! Profiles keeps the selected profile's preferences directly beneath its
//! name. The screen therefore reads as one compact tree beside the harness
//! checklist, instead of making people scan three independent columns.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use uze_core::preference::{Autonomy, ModelPreference, PreferenceApplyOutcome, SandboxScope};

use super::super::content_area;
use super::super::hit::Hit;
use super::super::model::{ProfilePanel, TuiModel};
use super::super::{
    ACCENT, BLUE, BORDER, DANGER, MUTED, TEXT_BRIGHT, TEXT_SECONDARY, TEXT_TERTIARY, WARNING,
};

pub(crate) fn render_profiles(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(content_area(area));
    render_profile_tree(frame, columns[0], model, hits);
    render_harnesses(frame, columns[1], model, hits);
}

fn panel(right_border: bool) -> Block<'static> {
    Block::default()
        .borders(if right_border {
            Borders::RIGHT
        } else {
            Borders::NONE
        })
        .border_style(Style::default().fg(BORDER))
}

fn focus_color(focused: bool) -> Color {
    if focused { ACCENT } else { MUTED }
}

fn autonomy_label(value: Autonomy) -> &'static str {
    match value {
        Autonomy::Manual => "manual",
        Autonomy::Balanced => "balanced",
        Autonomy::Auto => "auto",
        Autonomy::Unattended => "unattended",
    }
}

fn autonomy_color(value: Autonomy) -> Color {
    match value {
        Autonomy::Manual | Autonomy::Balanced => ACCENT,
        Autonomy::Auto | Autonomy::Unattended => WARNING,
    }
}

fn sandbox_label(value: SandboxScope) -> &'static str {
    match value {
        SandboxScope::ReadOnly => "read-only",
        SandboxScope::WorkspaceWrite => "workspace-write",
        SandboxScope::FullAccess => "full-access",
    }
}

fn sandbox_color(value: SandboxScope) -> Color {
    match value {
        SandboxScope::ReadOnly | SandboxScope::WorkspaceWrite => ACCENT,
        SandboxScope::FullAccess => WARNING,
    }
}

fn model_label(value: ModelPreference) -> &'static str {
    match value {
        ModelPreference::Default => "default",
        ModelPreference::Fast => "fast",
        ModelPreference::Capable => "capable",
    }
}

fn render_profile_tree(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let block = panel(true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let header = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(5)])
        .split(Rect::new(inner.x, inner.y, inner.width, 1));
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Profiles",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        header[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled("+ new", Style::default().fg(ACCENT)))
            .alignment(Alignment::Right),
        header[1],
    );
    hits.push((header[1], Hit::NewProfile));
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Configure preferences and apply them across harnesses",
            Style::default().fg(MUTED),
        )),
        Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
    );

    if model.profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No profiles yet — press n",
                Style::default().fg(MUTED),
            )),
            Rect::new(inner.x, inner.y.saturating_add(3), inner.width, 1),
        );
        return;
    }

    let mut y = inner.y.saturating_add(3);
    let bottom = inner.y + inner.height.saturating_sub(1);
    for (index, profile) in model.profiles.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let selected = index == model.profiles_selected;
        let mut name_style = Style::default().fg(if profile.active {
            ACCENT
        } else if selected {
            TEXT_BRIGHT
        } else {
            TEXT_TERTIARY
        });
        if selected || profile.active {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![
            Span::styled(
                if selected { "▾" } else { "▸" },
                Style::default().fg(focus_color(
                    selected && model.profile_panel == ProfilePanel::List,
                )),
            ),
            Span::raw(" "),
        ];
        spans.push(Span::styled(profile.id.clone(), name_style));
        if profile.active {
            spans.push(Span::styled(
                " (✓ active)",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
        }
        let row = Rect::new(inner.x, y, inner.width, 1);
        let controls = if selected && model.profiles.len() > 1 && inner.width >= 20 {
            Some(
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(1),
                        Constraint::Length(8),
                        Constraint::Length(3),
                        Constraint::Length(8),
                    ])
                    .split(row),
            )
        } else {
            None
        };
        let profile_rect = controls.as_ref().map_or(row, |parts| parts[0]);
        frame.render_widget(Paragraph::new(Line::from(spans)), profile_rect);
        if let Some(parts) = controls {
            frame.render_widget(
                Paragraph::new(Span::styled("remove", Style::default().fg(DANGER)))
                    .alignment(Alignment::Right),
                parts[1],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(" │ ", Style::default().fg(BORDER)))
                    .alignment(Alignment::Center),
                parts[2],
            );
            hits.push((parts[1], Hit::DeleteSelectedProfile));
            if profile.active {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        "active",
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ))
                    .alignment(Alignment::Left),
                    parts[3],
                );
            } else {
                frame.render_widget(
                    Paragraph::new(Span::styled("apply", Style::default().fg(ACCENT)))
                        .alignment(Alignment::Left),
                    parts[3],
                );
                hits.push((parts[3], Hit::ApplySelectedProfile));
            }
        }
        hits.push((profile_rect, Hit::ProfileRow(index)));
        y += 1;

        if selected {
            let preferences = [
                (
                    "autonomy",
                    autonomy_label(profile.preferences.autonomy),
                    autonomy_color(profile.preferences.autonomy),
                ),
                (
                    "sandbox",
                    sandbox_label(profile.preferences.sandbox),
                    sandbox_color(profile.preferences.sandbox),
                ),
                ("model", model_label(profile.preferences.model), BLUE),
            ];
            for (preference_index, (label, value, color)) in preferences.into_iter().enumerate() {
                if y >= bottom {
                    break;
                }
                let editing = model.profile_panel == ProfilePanel::Editor
                    && preference_index == model.profile_editor_selected;
                let mut value_style = Style::default().fg(color);
                if editing {
                    value_style = value_style.add_modifier(Modifier::BOLD);
                }
                let rect = Rect::new(inner.x, y, inner.width, 1);
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            if editing { "  › " } else { "    " },
                            Style::default().fg(ACCENT),
                        ),
                        Span::styled(format!("{label:<20}"), Style::default().fg(TEXT_SECONDARY)),
                        Span::styled(value, value_style),
                    ])),
                    rect,
                );
                hits.push((rect, Hit::PreferenceRow(preference_index)));
                y += 1;
            }
        }
        y = y.saturating_add(1);
    }
}

fn outcome_badge(outcome: &PreferenceApplyOutcome) -> (&'static str, Color) {
    match outcome {
        PreferenceApplyOutcome::Applied { .. } => ("Applied", ACCENT),
        PreferenceApplyOutcome::AppliedWithApproximation { .. } => ("Applied~", WARNING),
        PreferenceApplyOutcome::Unsupported { .. } => ("Unsupported", MUTED),
        PreferenceApplyOutcome::Failed { .. } => ("Failed", DANGER),
    }
}

fn render_harnesses(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let focused = model.profile_panel == ProfilePanel::Harnesses;
    let harnesses: Vec<_> = model
        .doctor
        .as_ref()
        .map(|doctor| {
            doctor
                .harnesses
                .iter()
                .filter(|harness| harness.detection.present)
                .collect()
        })
        .unwrap_or_default();
    let block = panel(false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Harnesses",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if harnesses.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No harnesses detected",
                Style::default().fg(MUTED),
            )),
            Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1),
        );
        return;
    }

    let mut y = inner.y.saturating_add(2);
    let bottom = inner.y + inner.height.saturating_sub(1);
    for (index, harness) in harnesses.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let cursor = focused && index == model.profile_harness_selected;
        let checked = model
            .profile_harness_selection
            .contains(&harness.integration);
        let mut name_style = Style::default().fg(if cursor { TEXT_BRIGHT } else { TEXT_TERTIARY });
        if cursor {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![
            Span::styled(
                if cursor { "› " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                if checked { "[x] " } else { "[ ] " },
                Style::default().fg(if checked { ACCENT } else { MUTED }),
            ),
            Span::styled(harness.display_name.clone(), name_style),
        ];
        if let Some((label, color)) = model
            .profile_apply_results
            .iter()
            .find(|result| result.integration == harness.integration)
            .map(|result| outcome_badge(&result.outcome))
        {
            let used: usize = spans.iter().map(|span| span.width()).sum();
            spans.push(Span::raw(
                " ".repeat((inner.width as usize).saturating_sub(used + label.len())),
            ));
            spans.push(Span::styled(label, Style::default().fg(color)));
        }
        let rect = Rect::new(inner.x, y, inner.width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, Hit::ProfileHarnessRow(index)));
        y += 1;
    }

    let selected = harnesses
        .iter()
        .filter(|harness| {
            model
                .profile_harness_selection
                .contains(&harness.integration)
        })
        .count();
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("{selected}/{} selected", harnesses.len()),
            Style::default().fg(focus_color(focused)),
        ))
        .alignment(Alignment::Right),
        Rect::new(
            inner.x,
            inner.y + inner.height.saturating_sub(1),
            inner.width,
            1,
        ),
    );
}
