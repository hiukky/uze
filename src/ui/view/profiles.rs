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
use uze_application::{Autonomy, ModelPreference, PreferenceApplyOutcome, SandboxScope};

use super::super::hit::Hit;
use super::super::model::{ProfilePanel, ResizablePanel, TuiModel};
use super::super::{content_area, side_panel_area};
use crate::ui::theme::{self, Symbol, Token};

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
    let harnesses_area = side_panel_area(content, columns[1].width);
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
                {
                    let rule = theme::glyph(Symbol::TreeColumnDivider);
                    format!("{rule}\n").repeat(divider.height.saturating_sub(1) as usize) + &rule
                },
                theme::fg(Token::Accent),
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
        .border_style(theme::fg(Token::BorderDefault))
        .style(Style::default().bg(background))
}

fn focus_color(focused: bool) -> Color {
    if focused {
        theme::color(Token::Accent)
    } else {
        theme::color(Token::TextMuted)
    }
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
        Autonomy::Manual | Autonomy::Balanced => theme::color(Token::Accent),
        Autonomy::Auto | Autonomy::Unattended => theme::color(Token::StateWarning),
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
        SandboxScope::ReadOnly | SandboxScope::WorkspaceWrite => theme::color(Token::Accent),
        SandboxScope::FullAccess => theme::color(Token::StateWarning),
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
    let block = panel(false, theme::color(Token::SurfaceBackground));
    let panel_inner = block.inner(area);
    // The panel reaches one row above the content inset so its edge meets
    // the frame; the text inside starts where every other screen's
    // header does — the content inset itself, no padding of its own.
    let inner = Rect::new(
        panel_inner.x,
        panel_inner.y.saturating_add(1),
        panel_inner.width.saturating_sub(1),
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
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        )),
        header[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled("+ new", theme::fg(Token::Accent))).alignment(Alignment::Right),
        header[1],
    );
    hits.push((header[1], Hit::NewProfile));
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Configure preferences and apply them across harnesses",
            theme::fg(Token::TextMuted),
        )),
        Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
    );

    if model.profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No profiles yet — press n",
                theme::fg(Token::TextMuted),
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
                    Block::default().style(theme::bg(Token::SurfaceRaised)),
                    bg_rect,
                );
            }
        }
        let mut name_style = Style::default().fg(if profile.active {
            theme::color(Token::Accent)
        } else if selected {
            theme::color(Token::TextBright)
        } else {
            theme::color(Token::TextTertiary)
        });
        if selected {
            name_style = name_style.bg(theme::color(Token::SurfaceRaised));
        }
        if selected || profile.active {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![
            Span::styled(
                theme::glyph(if selected {
                    Symbol::ChevronExpanded
                } else {
                    Symbol::ChevronCollapsed
                }),
                Style::default().fg(focus_color(
                    selected && model.profile_panel == ProfilePanel::List,
                )),
            ),
            Span::raw(" "),
        ];
        spans.push(Span::styled(profile.id.clone(), name_style));
        if profile.active {
            spans.push(Span::styled(
                format!(" ({} active)", theme::glyph(Symbol::MarkOfficial)),
                theme::fg_bold(Token::Accent),
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
                Paragraph::new(Span::styled(
                    format!("{} remove", theme::glyph(Symbol::MarkClose)),
                    theme::fg(Token::StateDanger),
                ))
                .alignment(Alignment::Right),
                parts[1],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    theme::glyph(Symbol::TreeColumnDivider),
                    theme::fg(Token::BorderDefault),
                ))
                .alignment(Alignment::Center),
                parts[2],
            );
            hits.push((parts[1], Hit::DeleteSelectedProfile));
            if profile.active {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        format!("{} active", theme::glyph(Symbol::StatusSelected)),
                        theme::fg_bold(Token::Accent),
                    ))
                    .alignment(Alignment::Left),
                    parts[3],
                );
            } else {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        format!("{} apply", theme::glyph(Symbol::StatusIdle)),
                        theme::fg(Token::Accent),
                    ))
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
                (
                    "model",
                    model_label(profile.preferences.model),
                    theme::color(Token::StateInfo),
                ),
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
                            theme::on(Token::Accent, Token::SurfaceRaised),
                        ),
                        Span::styled(
                            format!("{label:<20}"),
                            theme::on(Token::TextSecondary, Token::SurfaceRaised),
                        ),
                        Span::styled(value, value_style.bg(theme::color(Token::SurfaceRaised))),
                    ]))
                    .style(theme::bg(Token::SurfaceRaised)),
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
        PreferenceApplyOutcome::Applied { .. } => ("Applied", theme::color(Token::Accent)),
        PreferenceApplyOutcome::AppliedWithApproximation { .. } => {
            ("Applied~", theme::color(Token::StateWarning))
        }
        PreferenceApplyOutcome::Unsupported { .. } => {
            ("Unsupported", theme::color(Token::TextMuted))
        }
        PreferenceApplyOutcome::Failed { .. } => ("Failed", theme::color(Token::StateDanger)),
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
    let block = panel(false, theme::color(Token::SurfaceRecessed));
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
                .fg(theme::color(Token::TextBright))
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
                theme::fg(Token::TextMuted),
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
        let mut name_style = Style::default().fg(if cursor {
            theme::color(Token::TextBright)
        } else {
            theme::color(Token::TextTertiary)
        });
        if cursor {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![
            Span::styled(if cursor { "› " } else { "  " }, theme::fg(Token::Accent)),
            Span::styled(
                if checked { "[x] " } else { "[ ] " },
                Style::default().fg(if checked {
                    theme::color(Token::Accent)
                } else {
                    theme::color(Token::TextMuted)
                }),
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
