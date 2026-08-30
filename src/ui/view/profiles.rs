//! TUI view — Profiles route.
//!
//! Three permanent columns (profile list / preference editor / harness
//! picker) — the one layout shape not used anywhere else in this TUI, which
//! otherwise sticks to a single list plus an optional slide-in drawer. A
//! profile is small, and the three panels different enough in kind, that a
//! permanent split reads clearer here than a drawer would. Scales to
//! however many harnesses are registered: the right panel iterates
//! `doctor.harnesses` (the same read model the Harnesses route uses), never
//! a hardcoded set.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
};

use uze_core::preference::{Autonomy, ModelPreference, PreferenceApplyOutcome, SandboxScope};

use super::super::hit::Hit;
use super::super::model::{ProfilePanel, TuiModel};
use super::super::{
    ACCENT, BLUE, BORDER, DANGER, MUTED, TEXT_BRIGHT, TEXT_SECONDARY, TEXT_TERTIARY, WARNING,
};
use super::super::{content_area, render_screen_header};

pub(crate) fn render_profiles(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(45),
            Constraint::Percentage(33),
        ])
        .split(area);

    render_list(frame, columns[0], model, hits);
    render_editor(frame, columns[1], model, hits);
    render_harnesses(frame, columns[2], model, hits);
}

/// Left/right breathing room inside each panel — text must never sit flush
/// against a column divider or the screen edge (the dividers themselves stay
/// the same dim hairline as everywhere else in this TUI; focus is shown
/// through the header trailer/row styling instead of a loud full-height
/// colored border).
fn panel_padding() -> Padding {
    Padding::new(2, 2, 0, 0)
}

fn bordered_panel() -> Block<'static> {
    Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER))
        .padding(panel_padding())
}

fn unbordered_panel() -> Block<'static> {
    Block::default().padding(panel_padding())
}

fn focus_color(focused: bool) -> Color {
    if focused { ACCENT } else { MUTED }
}

fn render_list(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let focused = model.profile_panel == ProfilePanel::List;
    let block = bordered_panel();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = render_screen_header(
        frame,
        inner,
        "Profiles",
        "select or create",
        Some(Span::styled(
            "+ new",
            Style::default().fg(focus_color(focused)),
        )),
    );

    let bottom = content.y + content.height;
    if model.profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No profiles yet — press n",
                Style::default().fg(MUTED),
            )),
            Rect::new(content.x, content.y, content.width, 1),
        );
        return;
    }
    for (y, (index, profile)) in (content.y..bottom).zip(model.profiles.iter().enumerate()) {
        let cursor = index == model.profiles_selected;
        let border = if cursor {
            Span::styled("│", Style::default().fg(ACCENT))
        } else {
            Span::raw(" ")
        };
        let name_fg = if cursor { TEXT_BRIGHT } else { TEXT_TERTIARY };
        let mut name_style = Style::default().fg(name_fg);
        if cursor {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![
            border,
            Span::raw(" "),
            Span::styled(
                if profile.active { "● " } else { "  " },
                Style::default().fg(ACCENT),
            ),
            Span::styled(profile.id.clone(), name_style),
        ];
        if profile.active {
            spans.push(Span::styled("  (active)", Style::default().fg(ACCENT)));
        }
        let rect = Rect::new(content.x, y, content.width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, Hit::ProfileRow(index)));
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

fn render_editor(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let focused = model.profile_panel == ProfilePanel::Editor;
    let block = bordered_panel();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(profile) = model.selected_profile() else {
        let content = render_screen_header(frame, inner, "Preferences", "", None);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Select or create a profile",
                Style::default().fg(MUTED),
            )),
            Rect::new(content.x, content.y, content.width, 1),
        );
        return;
    };

    let subtitle = profile
        .description
        .as_deref()
        .unwrap_or("select a preference to change it");
    let content = render_screen_header(
        frame,
        inner,
        &format!("Preferences — {}", profile.id),
        subtitle,
        None,
    );

    let rows: [(&str, String, Color); 3] = [
        (
            "autonomy",
            autonomy_label(profile.preferences.autonomy).to_owned(),
            autonomy_color(profile.preferences.autonomy),
        ),
        (
            "sandbox",
            sandbox_label(profile.preferences.sandbox).to_owned(),
            sandbox_color(profile.preferences.sandbox),
        ),
        (
            "model preference",
            model_label(profile.preferences.model).to_owned(),
            BLUE,
        ),
    ];

    let bottom = content.y + content.height;
    for (y, (index, (label, value, color))) in (content.y..bottom).zip(rows.into_iter().enumerate())
    {
        let cursor = focused && index == model.profile_editor_selected;
        let border = if cursor {
            Span::styled("│", Style::default().fg(ACCENT))
        } else {
            Span::raw(" ")
        };
        let mut value_style = Style::default().fg(color);
        if cursor {
            value_style = value_style.add_modifier(Modifier::BOLD);
        }
        let mut spans = vec![
            border,
            Span::raw(" "),
            Span::styled(format!("{label:<18}"), Style::default().fg(TEXT_SECONDARY)),
            Span::styled(value, value_style),
        ];
        if cursor {
            spans.push(Span::styled("  ⇅", Style::default().fg(MUTED)));
        }
        let rect = Rect::new(content.x, y, content.width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, Hit::PreferenceRow(index)));
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
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let block = unbordered_panel();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content = render_screen_header(
        frame,
        inner,
        "Apply to harnesses",
        "",
        Some(Span::styled(
            format!("{} selected", model.profile_harness_selection.len()),
            Style::default().fg(focus_color(focused)),
        )),
    );

    if harnesses.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No harnesses detected",
                Style::default().fg(MUTED),
            )),
            Rect::new(content.x, content.y, content.width, 1),
        );
        return;
    }

    let bottom = content.y + content.height;
    for (y, (index, harness)) in (content.y..bottom).zip(harnesses.iter().enumerate()) {
        let cursor = focused && index == model.profile_harness_selected;
        let checked = model
            .profile_harness_selection
            .contains(&harness.integration);
        let border = if cursor {
            Span::styled("│", Style::default().fg(ACCENT))
        } else {
            Span::raw(" ")
        };
        let checkbox = Span::styled(
            if checked { "[x] " } else { "[ ] " },
            Style::default().fg(if checked { ACCENT } else { MUTED }),
        );
        let name_fg = if cursor { TEXT_BRIGHT } else { TEXT_TERTIARY };
        let mut name_style = Style::default().fg(name_fg);
        if cursor {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        let name = Span::styled(harness.display_name.clone(), name_style);
        let badge = model
            .profile_apply_results
            .iter()
            .find(|result| result.integration == harness.integration)
            .map(|result| outcome_badge(&result.outcome));

        let mut spans = vec![border, Span::raw(" "), checkbox, name];
        if let Some((label, color)) = badge {
            let used: usize = spans.iter().map(|span| span.width()).sum();
            let gap = (content.width as usize).saturating_sub(used + label.len() + 2);
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(label, Style::default().fg(color)));
        }
        let rect = Rect::new(content.x, y, content.width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, Hit::ProfileHarnessRow(index)));
    }
}
