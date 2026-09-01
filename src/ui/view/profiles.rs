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
use super::super::model::{ProfilePanel, ResizablePanel, TuiModel};
use super::super::{
    ACCENT, BASE, BLUE, BORDER, DANGER, MUTED, SURFACE_OVERLAY, SURFACE_SUBTLE, TEXT_BRIGHT,
    TEXT_SECONDARY, TEXT_TERTIARY, WARNING,
};

pub(crate) fn render_profiles(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let content = content_area(area);
    let right_default = super::DRAWER_DEFAULT_WIDTH;
    let left_width = model
        .profile_columns_width
        .unwrap_or(content.width.saturating_sub(right_default))
        .clamp(24, content.width.saturating_sub(24).max(24))
        .min(content.width);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(left_width), Constraint::Min(1)])
        .split(content);
    let left_area = Rect::new(
        columns[0].x,
        columns[0].y.saturating_sub(1),
        columns[0].width,
        columns[0].height.saturating_add(1),
    );
    let harnesses_area = Rect::new(
        columns[1].x,
        columns[1].y.saturating_sub(1),
        columns[1].width,
        columns[1].height.saturating_add(1),
    );
    render_profile_tree(frame, left_area, model, hits);
    render_harnesses(frame, harnesses_area, model, hits);
    let divider = Rect::new(
        left_area.right().saturating_sub(1),
        left_area.y,
        1,
        left_area.height,
    );
    if model.dragging_panel == Some(ResizablePanel::ProfileColumns) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "│\n".repeat(divider.height.saturating_sub(1) as usize) + "│",
                Style::default().fg(ACCENT),
            )),
            divider,
        );
    }
    hits.insert(
        0,
        (divider, Hit::ResizePanel(ResizablePanel::ProfileColumns)),
    );
}

fn panel(right_border: bool, background: Color) -> Block<'static> {
    Block::default()
        .borders(if right_border {
            Borders::RIGHT
        } else {
            Borders::NONE
        })
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(background))
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
    let block = panel(false, BASE);
    let panel_inner = block.inner(area);
    let inner = Rect::new(
        panel_inner.x.saturating_add(2),
        panel_inner.y.saturating_add(1),
        panel_inner.width.saturating_sub(3),
        panel_inner.height.saturating_sub(2),
    );
    frame.render_widget(block, area);
    let header = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(7)])
        .split(Rect::new(
            inner.x,
            inner.y,
            inner.width.saturating_sub(1),
            1,
        ));
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
            Rect::new(inner.x, inner.y.saturating_add(4), inner.width, 1),
        );
        return;
    }

    let mut y = inner.y.saturating_add(4);
    let bottom = inner.y + inner.height.saturating_sub(1);
    for (index, profile) in model.profiles.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let selected = index == model.profiles_selected;
        if selected {
            let content_h: u16 = 4;
            // 1 top + content + 1 bottom padding inside the overlay
            let y0 = y.saturating_sub(1);
            let available = bottom.saturating_sub(y0) as usize;
            let h = (content_h + 2).min(available as u16);
            if h > 0 {
                let bg_rect = Rect::new(inner.x, y0, inner.width, h);
                frame.render_widget(
                    Block::default().style(Style::default().bg(SURFACE_OVERLAY)),
                    bg_rect,
                );
            }
        }
        let mut name_style = Style::default().fg(if profile.active {
            ACCENT
        } else if selected {
            TEXT_BRIGHT
        } else {
            TEXT_TERTIARY
        });
        if selected {
            name_style = name_style.bg(SURFACE_OVERLAY);
        }
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
                        Constraint::Length(10),
                        Constraint::Length(3),
                        Constraint::Length(10),
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
                Paragraph::new(Span::styled("✕ remove", Style::default().fg(DANGER)))
                    .alignment(Alignment::Right),
                parts[1],
            );
            frame.render_widget(
                Paragraph::new(Span::styled("│", Style::default().fg(BORDER)))
                    .alignment(Alignment::Center),
                parts[2],
            );
            hits.push((parts[1], Hit::DeleteSelectedProfile));
            if profile.active {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        "● active",
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ))
                    .alignment(Alignment::Left),
                    parts[3],
                );
            } else {
                frame.render_widget(
                    Paragraph::new(Span::styled("○ apply", Style::default().fg(ACCENT)))
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
                            Style::default().fg(ACCENT).bg(SURFACE_OVERLAY),
                        ),
                        Span::styled(
                            format!("{label:<20}"),
                            Style::default().fg(TEXT_SECONDARY).bg(SURFACE_OVERLAY),
                        ),
                        Span::styled(value, value_style.bg(SURFACE_OVERLAY)),
                    ]))
                    .style(Style::default().bg(SURFACE_OVERLAY)),
                    rect,
                );
                hits.push((rect, Hit::PreferenceRow(preference_index)));
                y += 1;
            }
        }
        y = y.saturating_add(2);
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
    let block = panel(false, SURFACE_SUBTLE);
    let panel_inner = block.inner(area);
    let inner = Rect::new(
        panel_inner.x.saturating_add(2),
        panel_inner.y.saturating_add(1),
        panel_inner.width.saturating_sub(3),
        panel_inner.height.saturating_sub(2),
    );
    frame.render_widget(block, area);
    let header = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(12)])
        .split(Rect::new(inner.x, inner.y, inner.width, 1));
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Harnesses",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        header[0],
    );
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
        header[1],
    );
    if harnesses.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No harnesses detected",
                Style::default().fg(MUTED),
            )),
            Rect::new(inner.x, inner.y.saturating_add(3), inner.width, 1),
        );
        return;
    }

    let first_harness_row = inner.y.saturating_add(3);
    let bottom = inner.y + inner.height.saturating_sub(1);
    for (y, (index, harness)) in (first_harness_row..bottom).zip(harnesses.iter().enumerate()) {
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
    }
}
