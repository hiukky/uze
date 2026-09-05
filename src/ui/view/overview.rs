//! TUI view — Overview route.
//!
//! The machine dashboard: harnesses detected, plugins installed, marketplace
//! sources, and the global health line. Project-specific context belongs in
//! the dedicated project and harness views, not this machine-scoped overview.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::super::hit::Hit;
use super::super::model::{Focus, Route, TuiModel};
use super::super::{content_area, render_screen_header};
use super::health::Severity;
use crate::ui::theme::{self, Symbol, Token};
use uze_application::{PromptAge, PromptClock};

pub(crate) fn render_overview(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    let content = render_screen_header(frame, area, "Overview", "status & health", None);

    let harness_total = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let harness_detected = model.doctor.as_ref().map_or(0, |d| {
        d.harnesses.iter().filter(|h| h.detection.present).count()
    });
    let alerts = model.alerts();
    let mut y = content.y;

    // Status line: dot + headline + detail, matching the design's single
    // "All systems healthy — N harnesses detected, ..." summary row.
    let (color, headline) = if alerts.is_empty() {
        (theme::color(Token::StateSuccess), "All systems healthy")
    } else {
        (theme::color(Token::StateWarning), "Attention needed")
    };
    let detail = if alerts.is_empty() {
        format!(
            "— {harness_detected} harness{} detected",
            if harness_detected == 1 { "" } else { "es" }
        )
    } else {
        format!("— {} item(s) need attention", alerts.len())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", theme::glyph(Symbol::StatusSelected)),
                Style::default().fg(color),
            ),
            Span::styled(
                headline,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(detail, theme::fg(Token::TextMuted)),
        ])),
        Rect::new(content.x, y, content.width, 1),
    );
    y += 3;

    // 3-column stat grid, each cell divided from its neighbor by a left
    // hairline border — the design's `border-left:1px solid rgba(...)`.
    let stats = [
        (
            "Harnesses detected",
            format!("{harness_detected}/{harness_total}"),
            theme::color(Token::TextBright),
        ),
        (
            "Plugins installed",
            model.plugins.len().to_string(),
            theme::color(Token::TextBright),
        ),
        (
            "Active profile",
            model
                .profiles
                .iter()
                .find(|profile| profile.active)
                .map_or_else(|| "none".to_owned(), |profile| profile.id.clone()),
            theme::color(Token::StateSuccess),
        ),
    ];
    if y + 1 < content.y + content.height {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(Rect::new(content.x, y, content.width, 2));
        for (cell, (label, value, color)) in columns.iter().zip(stats) {
            let block = Block::default()
                .borders(Borders::LEFT)
                .border_style(theme::fg(Token::BorderFaint));
            let inner = block.inner(*cell);
            frame.render_widget(block, *cell);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(inner);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {label}"),
                    theme::fg(Token::TextMuted),
                )),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {value}"),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
                rows[1],
            );
        }
    }
    // Keep the activity stream visually separate from the compact stat cards.
    y += 5;

    // Recent prompts — the workspace-aware mini history. Always rendered
    // after the stat grid so it stays discoverable even when alerts exist;
    // alerts are pushed below it rather than replacing it.
    let bottom = content.y + content.height;
    if y < bottom {
        let reserved_for_alerts = if alerts.is_empty() { 0 } else { 2 };
        y = render_prompt_history(
            frame,
            Rect::new(content.x, y, content.width, bottom - y),
            model,
            hits,
            reserved_for_alerts,
        );
        if !alerts.is_empty() && y < bottom {
            y += 1;
        }
    }

    if alerts.is_empty() {
        return;
    }
    if y < content.y + content.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Needs attention",
                theme::fg_bold(Token::TextMuted),
            )),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
    for alert in alerts
        .iter()
        .take((content.y + content.height).saturating_sub(y) as usize)
    {
        let (symbol, color) = match alert.severity {
            Severity::High => (Symbol::MarkClose, theme::color(Token::StateDanger)),
            Severity::Medium => (Symbol::MarkAttention, theme::color(Token::StateWarning)),
            Severity::Low => (Symbol::MarkDot, theme::color(Token::Accent)),
        };
        let glyph = theme::glyph(symbol);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::styled(alert.label.clone(), theme::fg(Token::TextBright)),
                Span::styled(format!(" — {}", alert.detail), theme::fg(Token::TextMuted)),
            ])),
            Rect::new(content.x, y, content.width, 1),
        );
        y += 1;
    }
}

/// Blank columns between the table's columns.
const COLUMN_GAP: usize = 3;
/// Room for the selection marker in front of every row.
const MARKER_WIDTH: usize = 2;
/// A workspace label longer than this is clipped so the prompt keeps its
/// share of the row.
const MAX_WORKSPACE_WIDTH: usize = 24;

/// One row of the listing once it has been laid out against the budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptRow {
    /// Group heading, preceded by a blank line when it is not the first row.
    Group(PromptAge),
    Entry(usize),
}

/// Draws the prompt table into `area` and returns the first row below it.
/// `reserved` rows at the bottom are left for whatever follows the table.
fn render_prompt_history(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
    reserved: u16,
) -> u16 {
    let bottom = area.y + area.height;
    let entries = &model.prompt_history;
    let mut y = area.y;

    let mut title = Line::from(vec![
        Span::styled(
            "Recent prompts",
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if entries.is_empty() {
                " — no history yet".to_owned()
            } else {
                format!(" — {} recorded", entries.len())
            },
            theme::fg(Token::TextMuted),
        ),
    ]);
    let tally = harness_tally(entries);
    if title.width() + COLUMN_GAP + tally.width() <= area.width as usize {
        frame.render_widget(
            Paragraph::new(tally).alignment(Alignment::Right),
            Rect::new(area.x, y, area.width, 1),
        );
    } else {
        super::super::management::clip_line(&mut title, area.width as usize);
    }
    frame.render_widget(Paragraph::new(title), Rect::new(area.x, y, area.width, 1));
    y += 2;

    if entries.is_empty() {
        if y < bottom {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "Prompts submitted to an agent tab appear here. Click one to jump back to its tab.",
                    theme::fg(Token::TextDim),
                )),
                Rect::new(area.x, y, area.width, 1),
            );
            y += 1;
        }
        return y;
    }

    let clock = PromptClock::now();
    let ages: Vec<PromptAge> = entries.iter().map(|entry| entry.age(&clock)).collect();
    let whens: Vec<String> = entries
        .iter()
        .map(|entry| entry.compact_age(&clock))
        .collect();
    let workspaces: Vec<String> = entries
        .iter()
        .map(|entry| format!("{}/{}", entry.space_label, entry.tab_label))
        .collect();
    let columns = PromptColumns::fit(entries, &whens, &workspaces);

    if y < bottom {
        let mut header = columns.line(
            Span::raw(" ".repeat(MARKER_WIDTH)),
            "HARNESS",
            "WHEN",
            "WORKSPACE",
            "PROMPT",
            theme::fg(Token::TextMuted),
        );
        super::super::management::clip_line(&mut header, area.width as usize);
        frame.render_widget(Paragraph::new(header), Rect::new(area.x, y, area.width, 1));
    }
    y += 2;

    let budget = bottom.saturating_sub(y).saturating_sub(reserved) as usize;
    let selected_index = model.overview_prompt_selected;
    let rows = rows_keeping_selection_visible(&ages, selected_index, budget);

    for (position, row) in rows.iter().enumerate() {
        match *row {
            PromptRow::Group(age) => {
                if position > 0 {
                    y += 1;
                }
                let label = age.label().unwrap_or_default().to_uppercase();
                let lead = format!("{} ", theme::glyph(Symbol::TreeDivider).repeat(2));
                let rule = (area.width as usize)
                    .saturating_sub(lead.chars().count() + label.chars().count() + 1);
                let mut line = Line::from(vec![
                    Span::styled(lead, theme::fg(Token::TextFaint)),
                    Span::styled(label, theme::fg(Token::TextMuted)),
                    Span::styled(
                        format!(" {}", theme::glyph(Symbol::TreeDivider).repeat(rule)),
                        theme::fg(Token::TextFaint),
                    ),
                ]);
                super::super::management::clip_line(&mut line, area.width as usize);
                frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
                y += 1;
            }
            PromptRow::Entry(index) => {
                let entry = &entries[index];
                let rect = Rect::new(area.x, y, area.width, 1);
                let selected = index == selected_index
                    && model.focus == Focus::Content
                    && model.route == Route::Overview;
                let background = if selected {
                    Some(theme::color(Token::SurfaceSelected))
                } else if model.overview_prompt_hovered == Some(index) {
                    Some(theme::color(Token::SurfaceRaised))
                } else {
                    None
                };
                if let Some(background) = background {
                    frame.render_widget(
                        Block::default().style(Style::default().bg(background)),
                        rect,
                    );
                }
                let marker = if selected {
                    Span::styled(
                        format!("{} ", theme::glyph(Symbol::Prompt)),
                        theme::fg(Token::Accent),
                    )
                } else {
                    Span::raw(" ".repeat(MARKER_WIDTH))
                };
                let mut line = columns.line(
                    marker,
                    &entry.agent_binary,
                    &whens[index],
                    &workspaces[index],
                    &entry.preview,
                    Style::default(),
                );
                let (harness_style, when_style, workspace_style, prompt_style) = if selected {
                    let bold = Modifier::BOLD;
                    (
                        theme::fg(Token::Accent).add_modifier(bold),
                        theme::fg(Token::TextSecondary),
                        theme::fg(Token::StateInfo),
                        theme::fg(Token::TextBright).add_modifier(bold),
                    )
                } else {
                    (
                        theme::fg(Token::TextSecondary),
                        theme::fg(Token::TextDim),
                        theme::fg(Token::StateInfo),
                        theme::fg(Token::TextPrimary),
                    )
                };
                for (span, style) in line.spans.iter_mut().skip(1).zip([
                    harness_style,
                    when_style,
                    workspace_style,
                    prompt_style,
                ]) {
                    span.style = style;
                }
                super::super::management::clip_line(&mut line, area.width as usize);
                frame.render_widget(Paragraph::new(line), rect);
                hits.push((rect, Hit::PromptHistory(index)));
                y += 1;
            }
        }
    }
    y
}

/// Per-harness counts for the title's right edge, busiest first and, on a
/// tie, the one heard from most recently first.
fn harness_tally(entries: &[uze_application::PromptEntry]) -> Line<'static> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for entry in entries {
        match counts
            .iter_mut()
            .find(|(binary, _)| *binary == entry.agent_binary)
        {
            Some((_, count)) => *count += 1,
            None => counts.push((entry.agent_binary.as_str(), 1)),
        }
    }
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let mut spans = Vec::new();
    for (position, (binary, count)) in counts.into_iter().enumerate() {
        if position > 0 {
            spans.push(Span::styled(" · ", theme::fg(Token::TextFaint)));
        }
        spans.push(Span::styled(
            format!("{binary} {count}"),
            theme::fg(Token::TextMuted),
        ));
    }
    Line::from(spans)
}

/// Column widths measured over every entry, so a row never shifts when the
/// listing scrolls.
struct PromptColumns {
    harness: usize,
    when: usize,
    workspace: usize,
}

impl PromptColumns {
    fn fit(
        entries: &[uze_application::PromptEntry],
        whens: &[String],
        workspaces: &[String],
    ) -> Self {
        let widest = |values: &mut dyn Iterator<Item = usize>| values.max().unwrap_or(0);
        Self {
            harness: widest(
                &mut entries
                    .iter()
                    .map(|entry| entry.agent_binary.chars().count()),
            )
            .max("HARNESS".len()),
            when: widest(&mut whens.iter().map(|when| when.chars().count())).max("WHEN".len()),
            workspace: widest(&mut workspaces.iter().map(|workspace| workspace.chars().count()))
                .clamp("WORKSPACE".len(), MAX_WORKSPACE_WIDTH),
        }
    }

    /// One row as five spans — marker, then the four columns — so a caller
    /// can restyle each column without re-deriving the padding.
    fn line(
        &self,
        marker: Span<'static>,
        harness: &str,
        when: &str,
        workspace: &str,
        prompt: &str,
        style: Style,
    ) -> Line<'static> {
        let gap = " ".repeat(COLUMN_GAP);
        let workspace = clip_chars(workspace, self.workspace);
        Line::from(vec![
            marker,
            Span::styled(
                format!("{harness:<width$}{gap}", width = self.harness),
                style,
            ),
            Span::styled(format!("{when:>width$}{gap}", width = self.when), style),
            Span::styled(
                format!("{workspace:<width$}{gap}", width = self.workspace),
                style,
            ),
            Span::styled(prompt.to_owned(), style),
        ])
    }
}

fn clip_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut clipped: String = value.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

/// Lays the listing out newest-first within `budget` lines. When the
/// selected entry would fall below the fold, rows are dropped from the top
/// until it fits, so keyboard navigation never selects something unseen.
fn rows_keeping_selection_visible(
    ages: &[PromptAge],
    selected: usize,
    budget: usize,
) -> Vec<PromptRow> {
    let mut first = 0;
    loop {
        let rows = rows_from(ages, first, budget);
        let shows_selection = rows.contains(&PromptRow::Entry(selected));
        if shows_selection || first >= selected || first + 1 >= ages.len() {
            return rows;
        }
        first += 1;
    }
}

fn rows_from(ages: &[PromptAge], first: usize, budget: usize) -> Vec<PromptRow> {
    let mut rows = Vec::new();
    let mut used = 0;
    let mut current: Option<PromptAge> = None;
    for (index, age) in ages.iter().enumerate().skip(first) {
        let mut cost = 1;
        let heading = if current != Some(*age) && age.label().is_some() {
            // A heading costs its own line plus the blank that separates it
            // from the rows above, and is only worth drawing with a row.
            cost += if rows.is_empty() { 1 } else { 2 };
            Some(PromptRow::Group(*age))
        } else {
            None
        };
        if used + cost > budget {
            // A listing too short for a heading still shows its first row
            // rather than nothing at all.
            if rows.is_empty() && heading.is_some() && budget >= 1 {
                rows.push(PromptRow::Entry(index));
            }
            break;
        }
        rows.extend(heading);
        rows.push(PromptRow::Entry(index));
        used += cost;
        current = Some(*age);
    }
    rows
}
