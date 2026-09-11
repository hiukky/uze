//! TUI view — Profiles route.
//!
//! Profiles keeps the selected profile's preferences directly beneath its
//! name. The screen therefore reads as one compact tree beside the harness
//! checklist, instead of making people scan three independent columns.
//!
//! The preview takes the tree's place: the same profile, spelled the way
//! each harness's own configuration spells it, against what that file holds
//! now. Preferences are UZE's vocabulary and a harness never reads them —
//! the preview is where a person sees what a harness will.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use uze_application::application::{HarnessPreview, ProfilePreview};
use uze_application::{
    Autonomy, AxisPlan, CompatibilityRoute, KeyPlan, ModelPreference, PlannedValue,
    PreferenceApplyOutcome, SandboxScope,
};

use super::super::hit::Hit;
use super::super::model::{ProfilePanel, ResizablePanel, TuiModel};
use super::super::{content_area, side_panel_area};
use super::{DrawerStatus, drawer_footer_height, render_drawer_footer};
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

/// Columns of a preference row: the cursor, the axis name, then the value
/// between its steppers — wide enough for the longest value
/// (`workspace-write`), so the steppers stand in one column on every row.
const PREFERENCE_CURSOR: usize = 4;
const PREFERENCE_LABEL: usize = 20;
const PREFERENCE_VALUE: usize = 15;

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
    let title = match (model.profile_preview_open, model.selected_profile()) {
        (true, Some(profile)) => format!(
            "Profiles {} {} preview",
            theme::glyph(Symbol::ChevronRight),
            profile.id
        ),
        _ => "Profiles".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            title,
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
    let subtitle = match (model.profile_preview_open, model.selected_profile()) {
        (true, Some(profile)) => preview_summary(model, &profile.id),
        _ => "Configure preferences and apply them across harnesses".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(Span::styled(subtitle, theme::fg(Token::TextMuted))),
        Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
    );

    if model.profile_preview_open && !model.profiles.is_empty() {
        let body = Rect::new(
            inner.x,
            inner.y.saturating_add(3),
            inner.width.saturating_sub(1),
            inner.height.saturating_sub(3),
        );
        let preview = preview_lines(model, body.width);
        // Scrolled only as far as keeping the cursor's harness, and a few
        // of its settings, on screen needs.
        let cursor_line = preview
            .harness_rows
            .get(model.profile_preview_cursor)
            .copied()
            .unwrap_or(0);
        let scroll = cursor_line.saturating_sub(usize::from(body.height).saturating_sub(6));
        for (index, line) in preview.harness_rows.iter().enumerate() {
            if let Some(row) = line
                .checked_sub(scroll)
                .and_then(|row| u16::try_from(row).ok())
                && row < body.height
            {
                hits.push((
                    Rect::new(body.x, body.y + row, body.width, 1),
                    Hit::PreviewHarness(index),
                ));
            }
        }
        frame.render_widget(
            Paragraph::new(preview.lines).scroll((u16::try_from(scroll).unwrap_or(0), 0)),
            body,
        );
        return;
    }

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
                format!(" ({} active)", theme::glyph(Symbol::MarkOk)),
                theme::fg_bold(Token::Accent),
            ));
        }
        // The row names the profile and nothing else: what can be done to
        // it is the drawer's buttons, as on every other screen.
        let profile_rect = Rect::new(inner.x, y, inner.width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), profile_rect);
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
                // The steppers say the value is one of several before anyone
                // tries a key, and they are what the pointer changes it with.
                let stepper = theme::on(
                    if editing {
                        Token::Accent
                    } else {
                        Token::TextMuted
                    },
                    Token::SurfaceRaised,
                );
                let rect = Rect::new(inner.x, y, inner.width, 1);
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            if editing {
                                format!("  {} ", theme::glyph(Symbol::ChevronRight))
                            } else {
                                " ".repeat(PREFERENCE_CURSOR)
                            },
                            theme::on(Token::Accent, Token::SurfaceRaised),
                        ),
                        Span::styled(
                            format!("{label:<PREFERENCE_LABEL$}"),
                            theme::on(Token::TextSecondary, Token::SurfaceRaised),
                        ),
                        Span::styled(format!("{} ", theme::glyph(Symbol::StepPrevious)), stepper),
                        Span::styled(
                            format!("{value:<PREFERENCE_VALUE$}"),
                            value_style.bg(theme::color(Token::SurfaceRaised)),
                        ),
                        Span::styled(format!(" {}", theme::glyph(Symbol::StepNext)), stepper),
                    ]))
                    .style(theme::bg(Token::SurfaceRaised)),
                    rect,
                );
                let previous_x = rect.x + (PREFERENCE_CURSOR + PREFERENCE_LABEL) as u16;
                let next_x = previous_x + 2 + PREFERENCE_VALUE as u16;
                // Two cells each — the glyph and the gap beside it — so the
                // target is not a single column wide.
                for (x, forward) in [(previous_x, false), (next_x, true)] {
                    if x + 2 <= rect.right() {
                        hits.push((
                            Rect::new(x, y, 2, 1),
                            Hit::StepPreference {
                                index: preference_index,
                                forward,
                            },
                        ));
                    }
                }
                hits.push((rect, Hit::PreferenceRow(preference_index)));
                y += 1;
            }
        }
        y = y.saturating_add(2);
    }
}

fn outcome_badge(outcome: &PreferenceApplyOutcome) -> (&'static str, Color) {
    // How faithfully each axis landed is the preview's table; the badge
    // only says whether the write happened.
    match outcome {
        PreferenceApplyOutcome::Applied { .. }
        | PreferenceApplyOutcome::AppliedWithApproximation { .. } => {
            ("applied", theme::color(Token::Accent))
        }
        PreferenceApplyOutcome::Unsupported { .. } => {
            ("unsupported", theme::color(Token::TextMuted))
        }
        PreferenceApplyOutcome::Failed { .. } => ("failed", theme::color(Token::StateDanger)),
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
    // This panel is the screen's drawer: it sits on the drawers' surface,
    // so the selected profile's actions end it the way they end every
    // other drawer.
    let offers = model
        .selected_profile()
        .map(|profile| profile.offers())
        .unwrap_or_default();
    let footer_height = if model.selected_profile().is_some() {
        drawer_footer_height(&offers)
    } else {
        0
    };
    let inner = Rect::new(
        panel_inner.x.saturating_add(2),
        panel_inner.y.saturating_add(1),
        panel_inner.width.saturating_sub(3),
        panel_inner.height.saturating_sub(2 + footer_height),
    );
    frame.render_widget(block, area);
    let checked: Vec<&str> = harnesses
        .iter()
        .filter(|harness| {
            model
                .profile_harness_selection
                .contains(&harness.integration)
        })
        .map(|harness| harness.integration.as_str())
        .collect();
    if let Some(profile) = model.selected_profile() {
        let (color, headline, subtitle) =
            drawer_status(profile.active, model.profile_preview_answer(), &checked);
        render_drawer_footer(
            frame,
            Rect::new(inner.x, inner.bottom(), inner.width, footer_height),
            DrawerStatus {
                color,
                headline,
                subtitle: &subtitle,
            },
            &offers,
            model.hovered_offer,
            model
                .profile_preview_open
                .then_some(uze_keys::Action::PreviewProfile),
            hits,
        );
    }
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
        let badge = model
            .profile_apply_results
            .iter()
            .find(|result| result.integration == harness.integration)
            .map(|result| {
                let (label, color) = outcome_badge(&result.outcome);
                (label.to_owned(), color)
            })
            .or_else(|| {
                model
                    .current_profile_preview()?
                    .harnesses
                    .iter()
                    .find(|preview| preview.integration == harness.integration)
                    .map(standing_badge)
            });
        if let Some((label, color)) = badge {
            let used: usize = spans.iter().map(|span| span.width()).sum();
            spans.push(Span::raw(" ".repeat(
                (inner.width as usize).saturating_sub(used + label.chars().count()),
            )));
            spans.push(Span::styled(label, Style::default().fg(color)));
        }
        let rect = Rect::new(inner.x, y, inner.width, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        hits.push((rect, Hit::ProfileHarnessRow(index)));
    }
}

/// The drawer's headline for the selected profile. "Active" alone used to
/// claim its preferences were in use, which nothing checked; the preview is
/// what can say so.
fn drawer_status(
    active: bool,
    answer: Option<Result<&ProfilePreview, &str>>,
    checked: &[&str],
) -> (Color, &'static str, String) {
    let pending = match answer {
        Some(Ok(preview)) => Some(pending_on(preview, checked)),
        _ => None,
    };
    let (color, headline) = match (active, pending) {
        (true, Some(0)) | (true, None) => (theme::color(Token::Accent), "Active"),
        (true, Some(_)) => (theme::color(Token::StateWarning), "Active, not in effect"),
        (false, _) => (theme::color(Token::TextMuted), "Not active"),
    };
    let subtitle = match (answer, pending) {
        (None, _) => "Reading what the harnesses hold…".to_owned(),
        (Some(Err(_)), _) => "Could not read the harnesses".to_owned(),
        (_, Some(0)) if active => "In effect on every checked harness".to_owned(),
        (_, Some(0)) => "Applying changes nothing on the checked harnesses".to_owned(),
        (_, pending) => {
            let pending = pending.unwrap_or(0);
            format!(
                "{pending} setting{} to write — see the preview, then apply",
                plural(pending)
            )
        }
    };
    (color, headline, subtitle)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Keys applying would change, on the harnesses it would be applied to.
fn pending_on(preview: &ProfilePreview, checked: &[&str]) -> usize {
    preview
        .harnesses
        .iter()
        .filter(|harness| checked.contains(&harness.integration.as_str()))
        .filter_map(|harness| harness.plan.as_ref().ok())
        .map(|plan| plan.pending())
        .sum()
}

/// The preview's one-line answer, where the list screen says what it is.
fn preview_summary(model: &TuiModel, id: &str) -> String {
    let checked: Vec<&str> = model
        .profile_harness_selection
        .iter()
        .map(String::as_str)
        .collect();
    match model.profile_preview_answer() {
        None => format!("Reading what \"{id}\" would change…"),
        Some(Err(_)) => "Could not read the harnesses".to_owned(),
        Some(Ok(preview)) => {
            let pending = pending_on(preview, &checked);
            let harnesses = preview
                .harnesses
                .iter()
                .filter(|harness| checked.contains(&harness.integration.as_str()))
                .filter(|harness| harness.plan.as_ref().is_ok_and(|plan| plan.pending() > 0))
                .count();
            if pending == 0 {
                format!("\"{id}\" is already in effect on every checked harness")
            } else {
                format!(
                    "\"{id}\" would change {pending} setting{} in {harnesses} harness{} — apply \
                     writes them",
                    plural(pending),
                    if harnesses == 1 { "" } else { "es" }
                )
            }
        }
    }
}

/// One harness's standing against the selected profile: its checklist
/// row's badge, and its heading in the preview.
fn standing_badge(preview: &HarnessPreview) -> (String, Color) {
    match &preview.plan {
        Err(_) => ("cannot apply".to_owned(), theme::color(Token::StateDanger)),
        Ok(plan) if plan.pending() == 0 => ("in effect".to_owned(), theme::color(Token::Accent)),
        Ok(plan) => (
            format!("{} change{}", plan.pending(), plural(plan.pending())),
            theme::color(Token::StateWarning),
        ),
    }
}

/// Where a harness's settings start when it is open.
const DETAIL_INDENT: usize = 6;
const VERB_COLUMN: usize = 8;
/// Each axis's column in the preview's table — wide enough for its words.
const AXIS_CELL: usize = 11;

/// What applying does to one key, in the word a person would use for it.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Verb {
    Add,
    Change,
    Remove,
    /// Already what the profile asks for.
    Keep,
    /// The operator's own value, which the profile has no reason to touch.
    Yours,
}

impl Verb {
    fn of(key: &KeyPlan) -> Self {
        match (&key.planned, key.in_effect(), key.current.is_some()) {
            (PlannedValue::Kept, ..) => Self::Yours,
            (_, true, _) => Self::Keep,
            (PlannedValue::Removed, false, _) => Self::Remove,
            (PlannedValue::Set(_), false, true) => Self::Change,
            (PlannedValue::Set(_), false, false) => Self::Add,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Change => "change",
            Self::Remove => "remove",
            Self::Keep => "keep",
            Self::Yours => "yours",
        }
    }

    fn changes(self) -> bool {
        matches!(self, Self::Add | Self::Change | Self::Remove)
    }
}

/// How faithfully a harness can do what one axis asks, in one word — the
/// cell of the preview's table.
fn fidelity(route: CompatibilityRoute) -> (&'static str, Color) {
    match route {
        CompatibilityRoute::Native => ("as asked", theme::color(Token::Accent)),
        CompatibilityRoute::Adaptable => ("adapted", theme::color(Token::StateInfo)),
        CompatibilityRoute::Degraded => ("partial", theme::color(Token::StateWarning)),
        CompatibilityRoute::Unsupported => ("n/a", theme::color(Token::TextMuted)),
    }
}

/// The preview's lines, and which line is each harness's row — so the
/// screen can scroll to the cursor and make each row a click target.
pub(crate) struct PreviewLines {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) harness_rows: Vec<usize>,
}

/// The preview of the selected profile: a table of every detected harness
/// against the three axes, each cell saying how faithfully that harness
/// can do what the axis asks. A harness opens onto its own file — what
/// applying adds, changes or removes there, and whatever keeps an axis
/// from being honoured — and opens by itself only when applying would
/// write something. What is already in effect and delivered as asked is
/// one quiet row, not a page of keys.
pub(crate) fn preview_lines(model: &TuiModel, width: u16) -> PreviewLines {
    let message = |text: String, token: Token| PreviewLines {
        lines: wrapped(&text, 0, 0, width, theme::color(token)),
        harness_rows: Vec::new(),
    };
    let preview = match model.profile_preview_answer() {
        Some(Ok(preview)) => preview,
        Some(Err(reason)) => {
            return message(
                format!("Could not read the harnesses: {reason}"),
                Token::StateDanger,
            );
        }
        None if model.detected_harness_ids().is_empty() => {
            return message(
                "No harnesses detected — nothing to preview".to_owned(),
                Token::TextMuted,
            );
        }
        None => {
            return message(
                "Reading each harness's configuration…".to_owned(),
                Token::TextMuted,
            );
        }
    };
    let names: Vec<String> = preview
        .harnesses
        .iter()
        .map(|harness| harness_name(model, &harness.integration))
        .collect();
    let name_width = names
        .iter()
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let mut lines = vec![Line::from(vec![
        Span::raw(" ".repeat(4 + name_width)),
        Span::styled(
            ["autonomy", "sandbox", "model"]
                .map(|axis| format!("{axis:<AXIS_CELL$}"))
                .concat(),
            theme::fg(Token::TextMuted),
        ),
    ])];
    let mut harness_rows = Vec::new();
    for (index, (harness, name)) in preview.harnesses.iter().zip(names).enumerate() {
        let open = model.profile_preview_expanded(harness);
        harness_rows.push(lines.len());
        lines.push(harness_row(
            model, harness, &name, name_width, index, open, width,
        ));
        if open {
            lines.extend(harness_detail(harness, width));
            lines.push(Line::default());
        }
    }
    lines.push(Line::default());
    lines.extend(wrapped(
        "adapted: set another way · partial: not fully honoured · n/a: no such setting",
        0,
        0,
        width,
        theme::color(Token::TextMuted),
    ));
    PreviewLines {
        lines,
        harness_rows,
    }
}

fn harness_name(model: &TuiModel, integration: &str) -> String {
    model
        .doctor
        .as_ref()
        .and_then(|doctor| {
            doctor
                .harnesses
                .iter()
                .find(|health| health.integration == integration)
        })
        .map_or_else(
            || integration.to_owned(),
            |health| health.display_name.clone(),
        )
}

fn harness_row(
    model: &TuiModel,
    harness: &HarnessPreview,
    name: &str,
    name_width: usize,
    index: usize,
    open: bool,
    width: u16,
) -> Line<'static> {
    let checked = model
        .profile_harness_selection
        .contains(&harness.integration);
    let cursor = index == model.profile_preview_cursor;
    let mut spans = vec![
        Span::styled(
            if cursor {
                format!("{} ", theme::glyph(Symbol::ChevronRight))
            } else {
                "  ".to_owned()
            },
            theme::fg(Token::Accent),
        ),
        Span::styled(
            format!(
                "{} ",
                theme::glyph(if open {
                    Symbol::ChevronExpanded
                } else {
                    Symbol::ChevronCollapsed
                })
            ),
            theme::fg(Token::TextMuted),
        ),
        Span::styled(
            format!("{name:<name_width$}"),
            if checked {
                theme::fg_bold(Token::TextBright)
            } else {
                theme::fg(Token::TextTertiary)
            },
        ),
    ];
    match &harness.plan {
        Ok(plan) => {
            for axis in &plan.axes {
                let (word, color) = fidelity(axis.route);
                spans.push(Span::styled(
                    format!("{word:<AXIS_CELL$}"),
                    Style::default().fg(color),
                ));
            }
        }
        Err(_) => spans.push(Span::raw(" ".repeat(AXIS_CELL * 3))),
    }
    let (standing, color) = if checked {
        standing_badge(harness)
    } else {
        ("not checked".to_owned(), theme::color(Token::TextMuted))
    };
    let used: usize = spans.iter().map(Span::width).sum();
    spans.push(Span::raw(
        " ".repeat(
            (width as usize)
                .saturating_sub(used + standing.chars().count())
                .max(1),
        ),
    ));
    spans.push(Span::styled(standing, Style::default().fg(color)));
    let line = Line::from(spans);
    if cursor {
        line.style(theme::bg(Token::SurfaceRaised))
    } else {
        line
    }
}

/// An open harness: its file, what keeps an axis from being honoured, and
/// its settings — what applying changes first.
fn harness_detail(harness: &HarnessPreview, width: u16) -> Vec<Line<'static>> {
    let plan = match &harness.plan {
        Ok(plan) => plan,
        Err(reason) => {
            return wrapped(
                reason,
                DETAIL_INDENT,
                0,
                width,
                theme::color(Token::StateDanger),
            );
        }
    };
    let mut lines = vec![Line::from(vec![
        Span::raw(" ".repeat(DETAIL_INDENT)),
        Span::styled(
            crate::ui::display_project_path(&plan.config_path),
            theme::fg(Token::TextMuted),
        ),
    ])];
    lines.extend(caveats(&plan.axes, width));
    let key_width = plan
        .axes
        .iter()
        .flat_map(|axis| &axis.keys)
        .map(|key| key.key.len())
        .max()
        .unwrap_or(0)
        // A key longer than half the room pushes only its own line along,
        // rather than every line's value off the edge.
        .min((width as usize).saturating_sub(DETAIL_INDENT + VERB_COLUMN) / 2);
    let mut keys: Vec<&KeyPlan> = plan.axes.iter().flat_map(|axis| &axis.keys).collect();
    keys.sort_by_key(|key| !Verb::of(key).changes());
    lines.extend(keys.into_iter().map(|key| key_line(key, key_width)));
    lines
}

/// `change  permissions.defaultMode  "auto" → "acceptEdits"`.
fn key_line(key: &KeyPlan, key_width: usize) -> Line<'static> {
    let verb = Verb::of(key);
    let (verb_style, key_style) = if verb.changes() {
        (
            theme::fg_bold(Token::StateWarning),
            theme::fg(Token::TextBright),
        )
    } else {
        (theme::fg(Token::TextMuted), theme::fg(Token::TextTertiary))
    };
    let current = key.current.clone().unwrap_or_else(|| "unset".to_owned());
    let arrow = || {
        Span::styled(
            format!(" {} ", theme::glyph(Symbol::ArrowTo)),
            theme::fg(Token::TextMuted),
        )
    };
    let planned = |value: String| Span::styled(value, theme::fg_bold(Token::StateWarning));
    let mut spans = vec![
        Span::raw(" ".repeat(DETAIL_INDENT)),
        Span::styled(format!("{:<VERB_COLUMN$}", verb.word()), verb_style),
        Span::styled(format!("{:<key_width$}  ", key.key), key_style),
    ];
    match (verb, &key.planned) {
        (Verb::Add, PlannedValue::Set(value)) => spans.push(planned(value.clone())),
        (Verb::Change, PlannedValue::Set(value)) => {
            spans.push(Span::styled(current, theme::fg(Token::TextMuted)));
            spans.push(arrow());
            spans.push(planned(value.clone()));
        }
        (Verb::Remove, _) => {
            spans.push(Span::styled(current, theme::fg(Token::TextMuted)));
            spans.push(arrow());
            spans.push(planned("unset".to_owned()));
        }
        (Verb::Yours, _) => {
            spans.push(Span::styled(current, theme::fg(Token::TextSecondary)));
            spans.push(Span::styled(
                "  left as you set it",
                theme::fg(Token::TextMuted),
            ));
        }
        _ => spans.push(Span::styled(current, theme::fg(Token::TextMuted))),
    }
    Line::from(spans)
}

/// What keeps an axis from being honoured on this harness — a `partial`
/// or `n/a` cell's reason. How an `adapted` axis is spelled is not one:
/// the settings below already show it.
fn caveats(axes: &[AxisPlan], width: u16) -> Vec<Line<'static>> {
    let mut caveats: Vec<&AxisPlan> = axes
        .iter()
        .filter(|axis| {
            matches!(
                axis.route,
                CompatibilityRoute::Degraded | CompatibilityRoute::Unsupported
            )
        })
        .collect();
    caveats.sort_by_key(|axis| axis.route != CompatibilityRoute::Degraded);
    caveats
        .into_iter()
        .flat_map(|axis| {
            let (mark, color) = if axis.route == CompatibilityRoute::Degraded {
                (Symbol::MarkAttention, theme::color(Token::StateWarning))
            } else {
                (Symbol::MarkUnsupported, theme::color(Token::TextMuted))
            };
            let reason = axis.note.as_ref().unwrap_or(&axis.summary);
            wrapped(
                &format!("{} {}: {reason}", theme::glyph(mark), axis.axis.label()),
                DETAIL_INDENT,
                2,
                width,
                color,
            )
        })
        .collect()
}

/// Word-wraps `text` into lines indented by `indent`, within `width`.
/// Continuation lines hang `hang` columns further in, so a note after its
/// mark reads as one block.
fn wrapped(text: &str, indent: usize, hang: usize, width: u16, color: Color) -> Vec<Line<'static>> {
    let room = (width as usize).saturating_sub(indent + hang).max(20);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > room {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let lead = if index == 0 { indent } else { indent + hang };
            Line::from(vec![
                Span::raw(" ".repeat(lead)),
                Span::styled(line, Style::default().fg(color)),
            ])
        })
        .collect()
}
