//! TUI — overlay state transitions and their rendering.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use super::model::{Focus, Overlay, TrustedRetry, TuiModel};
use super::worker::{Intent, TrustGrant};
use crate::ui::hint_aside;
use crate::ui::theme::{self, Symbol, Token};

impl TuiModel {
    pub(crate) fn overlay_key(&mut self, key: KeyEvent) -> Intent {
        let overlay = self.overlay.clone();
        match (&overlay, key.code) {
            (Overlay::Help | Overlay::HarnessHelp, _) => {
                self.close_overlay();
                Intent::None
            }
            (
                Overlay::ConfirmRemove { id, focus },
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right,
            ) => {
                let new_focus = 1 - *focus;
                self.overlay = Overlay::ConfirmRemove {
                    id: id.clone(),
                    focus: new_focus,
                };
                Intent::None
            }
            (Overlay::ConfirmRemove { id, focus }, KeyCode::Enter) => {
                if *focus == 1 {
                    let id = id.clone();
                    self.close_overlay();
                    Intent::Remove(id)
                } else {
                    self.close_overlay();
                    Intent::None
                }
            }
            (Overlay::ConfirmRemove { id, .. }, KeyCode::Char('y' | 'Y')) => {
                let id = id.clone();
                self.close_overlay();
                Intent::Remove(id)
            }
            (Overlay::ConfirmRemove { .. }, KeyCode::Char('n' | 'N') | KeyCode::Esc) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmRemove { .. }, _) => Intent::None,
            (
                Overlay::ConfirmDeleteProfile { id, focus },
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right,
            ) => {
                let new_focus = 1 - *focus;
                self.overlay = Overlay::ConfirmDeleteProfile {
                    id: id.clone(),
                    focus: new_focus,
                };
                Intent::None
            }
            (Overlay::ConfirmDeleteProfile { id, focus }, KeyCode::Enter) => {
                if *focus == 1 {
                    let id = id.clone();
                    self.close_overlay();
                    Intent::DeleteProfile(id)
                } else {
                    self.close_overlay();
                    Intent::None
                }
            }
            (Overlay::ConfirmDeleteProfile { id, .. }, KeyCode::Char('y' | 'Y')) => {
                let id = id.clone();
                self.close_overlay();
                Intent::DeleteProfile(id)
            }
            (Overlay::ConfirmDeleteProfile { .. }, KeyCode::Char('n' | 'N') | KeyCode::Esc) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmDeleteProfile { .. }, _) => Intent::None,
            (Overlay::ThemePicker { themes, selected }, KeyCode::Down | KeyCode::Char('j')) => {
                let last = themes.len().saturating_sub(1);
                self.overlay = Overlay::ThemePicker {
                    themes: themes.clone(),
                    selected: (*selected + 1).min(last),
                };
                Intent::None
            }
            (Overlay::ThemePicker { themes, selected }, KeyCode::Up | KeyCode::Char('k')) => {
                self.overlay = Overlay::ThemePicker {
                    themes: themes.clone(),
                    selected: selected.saturating_sub(1),
                };
                Intent::None
            }
            (Overlay::ThemePicker { themes, selected }, KeyCode::Enter) => {
                let Some((id, _)) = themes.get(*selected).cloned() else {
                    self.close_overlay();
                    return Intent::None;
                };
                self.close_overlay();
                Intent::SelectTheme(id)
            }
            (Overlay::ThemePicker { .. }, KeyCode::Esc | KeyCode::Char('q')) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ThemePicker { .. }, _) => Intent::None,
            (Overlay::ConfirmClearPromptHistory, KeyCode::Char('y' | 'Y') | KeyCode::Enter) => {
                self.close_overlay();
                Intent::ClearPromptHistory
            }
            (Overlay::ConfirmClearPromptHistory, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmUpdate(id), KeyCode::Char('y') | KeyCode::Enter) => {
                let id = id.clone();
                self.close_overlay();
                Intent::Update(id, TrustGrant::Ask)
            }
            (Overlay::ConfirmUpdate(_), _) => {
                self.close_overlay();
                Intent::None
            }
            (
                Overlay::ConfirmInstall { name, marketplace },
                KeyCode::Char('y') | KeyCode::Enter,
            ) => {
                let (name, marketplace) = (name.clone(), marketplace.clone());
                self.close_overlay();
                Intent::Install {
                    name,
                    marketplace,
                    grant: TrustGrant::Ask,
                }
            }
            (Overlay::ConfirmInstall { .. }, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmContextApply, KeyCode::Char('y') | KeyCode::Enter) => {
                self.close_overlay();
                Intent::ContextApply(self.workspace_root())
            }
            (Overlay::ConfirmContextApply, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ProtectedPlugin(_), _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::AddMarketplace(input), KeyCode::Enter) => {
                let source = input.trim().to_owned();
                self.close_overlay();
                if source.is_empty() {
                    Intent::None
                } else {
                    Intent::AddMarketplace(source)
                }
            }
            (Overlay::AddMarketplace(_), KeyCode::Esc) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::AddMarketplace(input), KeyCode::Backspace) => {
                let mut input = input.clone();
                input.pop();
                self.overlay = Overlay::AddMarketplace(input);
                Intent::None
            }
            (Overlay::AddMarketplace(input), KeyCode::Char(c)) => {
                let mut input = input.clone();
                input.push(c);
                self.overlay = Overlay::AddMarketplace(input);
                Intent::None
            }
            (Overlay::AddMarketplace(_), _) => Intent::None,
            (Overlay::NewProfile(input), KeyCode::Enter) => {
                let id = slugify(input);
                self.close_overlay();
                if id.is_empty() {
                    Intent::None
                } else {
                    Intent::CreateProfile(id)
                }
            }
            (Overlay::NewProfile(_), KeyCode::Esc) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::NewProfile(input), KeyCode::Backspace) => {
                let mut input = input.clone();
                input.pop();
                self.overlay = Overlay::NewProfile(input);
                Intent::None
            }
            (Overlay::NewProfile(input), KeyCode::Char(c)) => {
                let mut input = input.clone();
                input.push(c);
                self.overlay = Overlay::NewProfile(input);
                Intent::None
            }
            (Overlay::NewProfile(_), _) => Intent::None,
            (Overlay::TrustRequired { retry, .. }, KeyCode::Char('y') | KeyCode::Enter) => {
                let intent = match retry {
                    TrustedRetry::Install { name, marketplace } => Intent::Install {
                        name: name.clone(),
                        marketplace: marketplace.clone(),
                        grant: TrustGrant::Granted,
                    },
                    TrustedRetry::Update(id) => Intent::Update(id.clone(), TrustGrant::Granted),
                };
                self.close_overlay();
                intent
            }
            (Overlay::TrustRequired { .. }, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::None, _) => Intent::None,
        }
    }

    pub(crate) fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
        self.focus = Focus::Content;
    }
}

pub(crate) fn render_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    render_modal(
        frame,
        area,
        "Help",
        vec![
            Line::from(format!(
                "{}{} / j k     Navigate",
                theme::glyph(Symbol::ArrowUp),
                theme::glyph(Symbol::ArrowDown)
            )),
            Line::from(format!(
                "Tab          Switch focus (sidebar {} content)",
                theme::glyph(Symbol::ArrowSwap)
            )),
            Line::from("Enter        Open / Inspect"),
            Line::from("Mouse click  Select sidebar route or list row"),
            Line::from("Scroll       Move selection"),
            Line::from(hint_aside(
                "r            Remove plugin (Plugins)",
                "Refresh elsewhere",
            )),
            Line::from("u            Update plugin (Plugins, when available)"),
            Line::from("i            Install plugin (Plugins)"),
            Line::from("/            Filter plugin list"),
            Line::from(hint_aside(
                "a            Add marketplace",
                "Analyze context (Harnesses)",
            )),
            Line::from("p            Apply context plan (Harnesses)"),
            Line::from("s            Setup harness (Harnesses)"),
            Line::from("g            Refresh"),
            Line::from("q            Quit"),
            Line::from(""),
            Line::from(Span::styled(
                "any key to close",
                theme::fg(Token::TextMuted),
            )),
        ],
        theme::color(Token::Accent),
    );
}

/// The Harnesses screen's glossary — everything that screen's compact
/// glyphs/labels stand for, written out in plain language. Kept separate
/// from the generic `Help` keybinding overlay: this is reference material
/// about what the data *means*, not what a key *does*.
pub(crate) fn render_harness_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    // Width covers the longest label ("Not implemented", 16 chars) plus at
    // least one separating space — `{:<N}` never truncates or forces a gap
    // once content already reaches N, so anything shorter than the longest
    // label here would glue straight into the detail text that follows.
    let entry = |symbol: Symbol, label: &str, color: Color, detail: &str| {
        Line::from(vec![
            Span::styled(
                format!("{} {label:<18}", theme::glyph(symbol)),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(detail.to_owned(), theme::fg(Token::TextMuted)),
        ])
    };
    let heading = |text: &str| {
        Line::from(Span::styled(
            text.to_owned(),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ))
    };
    let lines = vec![
        heading("STATUS"),
        entry(
            Symbol::MarkClose,
            "Not installed",
            theme::color(Token::TextMuted),
            "The harness isn't on this machine at all.",
        ),
        entry(
            Symbol::StatusSelected,
            "Installed",
            theme::color(Token::StateWarning),
            "Detected, but UZE hasn't configured it — press s to run setup.",
        ),
        entry(
            Symbol::MarkOfficial,
            "Configured",
            theme::color(Token::Accent),
            "UZE has set it up — ready to receive plugins.",
        ),
        Line::from(""),
        heading("COMPATIBILITY (per capability, in the detail panel)"),
        entry(
            Symbol::MarkNative,
            "Native",
            theme::color(Token::Accent),
            "Works directly, no adaptation needed.",
        ),
        entry(
            Symbol::MarkNative,
            "Bridged",
            theme::color(Token::Accent),
            "Routed through UZE's managed AGENTS.md bridge file.",
        ),
        entry(
            Symbol::MarkWarning,
            "Missing/Drifted",
            theme::color(Token::StateWarning),
            "AGENTS.md bridge needs reconciliation — a to analyze, p to apply.",
        ),
        entry(
            Symbol::MarkClose,
            "Conflict/Blocked",
            theme::color(Token::StateDanger),
            "AGENTS.md bridge has unresolved content UZE won't overwrite.",
        ),
        entry(
            Symbol::MarkAdapted,
            "Adapted",
            theme::color(Token::StateWarning),
            "Works, converted from a different format.",
        ),
        entry(
            Symbol::MarkAdapted,
            "Degraded",
            theme::color(Token::StateWarning),
            "Works, but with reduced fidelity.",
        ),
        entry(
            Symbol::MarkUnsupported,
            "Not supported",
            theme::color(Token::StateDanger),
            "This harness has no route for it.",
        ),
        entry(
            Symbol::MarkUnsupported,
            "Not implemented",
            theme::color(Token::TextMuted),
            "UZE doesn't route this capability anywhere yet.",
        ),
        Line::from(""),
        Line::from(Span::styled(
            "any key to close",
            theme::fg(Token::TextMuted),
        )),
    ];
    // `render_modal`'s fixed 76-column cap was built for the short one-line
    // confirmations every other overlay uses — this glossary's longest
    // lines need more room than that to avoid wrapping and losing the
    // label/detail alignment, so size the popup off its own content instead
    // (still bounded by the real terminal width on anything narrower).
    let width = (lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4).min(area.width);
    let height = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(modal_block(" Harness status ", theme::color(Token::Accent)))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        popup,
    );
}

pub(crate) fn render_confirm_remove(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    id: &str,
    focus: usize,
) {
    // Compact, centered confirmation ~52 wide instead of stretching full width.
    let width = 52.min(area.width.saturating_sub(4));
    let height = 8.min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, popup);

    let cancel_style = if focus == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(theme::color(Token::TextBright))
            .add_modifier(Modifier::BOLD)
    } else {
        theme::fg(Token::TextMuted)
    };
    let remove_style = if focus == 1 {
        Style::default()
            .fg(theme::color(Token::TextBright))
            .bg(theme::color(Token::StateDanger))
            .add_modifier(Modifier::BOLD)
    } else {
        theme::fg_bold(Token::StateDanger)
    };

    let message = Line::from(vec![
        Span::raw("Remove "),
        Span::styled(id.to_owned(), theme::fg_bold(Token::StateDanger)),
        Span::raw("?"),
    ]);
    let hint = Line::from(Span::styled(
        "Only matched artifacts will be detached.",
        theme::fg(Token::TextMuted),
    ));
    // Centered button row with clear visual hierarchy; destructive action is
    // red, safe action is muted, focused button gets solid background.
    let buttons = Line::from(vec![
        Span::styled("  Cancel  ", cancel_style),
        Span::raw("  "),
        Span::styled("  Remove  ", remove_style),
    ]);
    let footer = Line::from(Span::styled(
        "tab switch · enter confirm · esc cancel · y/n",
        theme::fg(Token::TextMuted),
    ));

    let block = modal_block(" Remove plugin? ", theme::color(Token::StateDanger));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    // Layout inside popup: message, hint, empty, buttons, footer
    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(message).alignment(Alignment::Center),
        inner_layout[0],
    );
    frame.render_widget(
        Paragraph::new(hint).alignment(Alignment::Center),
        inner_layout[1],
    );
    frame.render_widget(
        Paragraph::new(buttons).alignment(Alignment::Center),
        inner_layout[3],
    );
    frame.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        inner_layout[4],
    );
}

pub(crate) fn render_protected_plugin(frame: &mut ratatui::Frame<'_>, area: Rect, id: &str) {
    let width = 56.min(area.width.saturating_sub(4));
    let height = 7.min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from(vec![
            Span::styled(id.to_owned(), theme::fg_bold(Token::Accent)),
            Span::raw(" is an official marketplace plugin"),
        ]),
        Line::from(Span::styled(
            "and cannot be removed from the TUI.",
            theme::fg(Token::TextMuted),
        )),
        Line::from(Span::styled(
            "Use a custom source for removable plugins.",
            theme::fg(Token::TextMuted),
        )),
        Line::from(Span::styled(
            "esc / enter to dismiss",
            theme::fg(Token::TextMuted),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(modal_block(
                " Protected plugin ",
                theme::color(Token::StateWarning),
            ))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .alignment(Alignment::Center),
        popup,
    );
}

pub(crate) fn render_confirm_update(frame: &mut ratatui::Frame<'_>, area: Rect, id: &str) {
    render_modal(
        frame,
        area,
        "Update plugin?",
        vec![
            Line::from(vec![
                Span::raw("Update "),
                Span::styled(id.to_owned(), theme::fg_bold(Token::Accent)),
                Span::raw(" to the latest marketplace revision?"),
            ]),
            Line::from(Span::styled(
                "enter/y update · esc/n cancel",
                theme::fg(Token::TextMuted),
            )),
        ],
        theme::color(Token::StateWarning),
    );
}

pub(crate) fn render_confirm_clear_prompt_history(frame: &mut ratatui::Frame<'_>, area: Rect) {
    render_modal(
        frame,
        area,
        "Clear prompt history?",
        vec![
            Line::from(Span::raw(
                "Delete every recorded prompt for this workspace. This cannot be undone.",
            )),
            Line::from(Span::styled(
                "enter/y clear · esc/n cancel",
                theme::fg(Token::TextMuted),
            )),
        ],
        theme::color(Token::StateDanger),
    );
}

pub(crate) fn render_confirm_install(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    name: &str,
    marketplace: &str,
) {
    render_modal(
        frame,
        area,
        "Install plugin?",
        vec![
            Line::from(vec![
                Span::raw("Install "),
                Span::styled(name.to_owned(), theme::fg_bold(Token::Accent)),
                Span::raw(" from "),
                Span::styled(marketplace.to_owned(), theme::fg(Token::TextMuted)),
                Span::raw("?"),
            ]),
            Line::from(Span::styled(
                "enter/y install · esc/n cancel",
                theme::fg(Token::TextMuted),
            )),
        ],
        theme::color(Token::Accent),
    );
}

pub(crate) fn render_add_marketplace(frame: &mut ratatui::Frame<'_>, area: Rect, input: &str) {
    let width = 60.min(area.width.saturating_sub(4));
    let height = 7.min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = modal_block(" Add marketplace ", theme::color(Token::Accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Local path or https://... source",
            theme::fg(Token::TextMuted),
        )),
        rows[0],
    );
    let field = Line::from(vec![
        Span::raw("› "),
        Span::styled(input.to_owned(), theme::fg_bold(Token::Accent)),
        Span::styled(theme::glyph(Symbol::BarThin), theme::fg(Token::Accent)),
    ]);
    frame.render_widget(Paragraph::new(field), rows[1]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "enter add · esc cancel",
            theme::fg(Token::TextMuted),
        )),
        rows[3],
    );
}

/// Normalizes free-text input into a profile-id slug: lowercase, runs of
/// whitespace/underscores collapsed to one `-`, everything else outside
/// `[a-z0-9-]` dropped. Trims leading/trailing `-`. Mirrors
/// `profile_state::validate_id`'s accepted charset (plus `_`, folded into
/// `-` here rather than rejected, since typing a space is the most likely
/// way a user would separate words).
fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in input.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch.is_whitespace() || ch == '_' {
            pending_dash = true;
        }
    }
    slug
}

/// The theme picker: what UZE can be drawn in, and which it is drawn in now.
///
/// Deliberately a plain list with no preview. A preview would have to draw a
/// second palette inside a frame already painted in the first one, which is
/// the one thing a terminal cannot do convincingly — and the real preview is
/// free: pressing enter repaints everything.
pub(crate) fn render_theme_picker(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    themes: &[(String, bool)],
    selected: usize,
) {
    let width = 46.min(area.width.saturating_sub(4));
    let height = (themes.len() as u16 + 4).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = modal_block(" Theme ", theme::color(Token::Accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines: Vec<Line<'static>> = themes
        .iter()
        .enumerate()
        .map(|(index, (id, in_force))| {
            let cursor = if index == selected {
                theme::glyph(Symbol::ChevronCollapsed)
            } else {
                " ".to_owned()
            };
            Line::from(vec![
                Span::styled(format!("{cursor} "), theme::fg(Token::Accent)),
                Span::styled(
                    id.clone(),
                    if index == selected {
                        theme::fg_bold(Token::TextBright)
                    } else {
                        theme::fg(Token::TextPrimary)
                    },
                ),
                Span::styled(
                    if *in_force {
                        format!("  {} in use", theme::glyph(Symbol::StatusSelected))
                    } else {
                        String::new()
                    },
                    theme::fg(Token::TextDim),
                ),
            ])
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(crate::ui::hint_spans(
        "↑↓ select · enter apply · esc close",
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(crate) fn render_new_profile(frame: &mut ratatui::Frame<'_>, area: Rect, input: &str) {
    let width = 60.min(area.width.saturating_sub(4));
    let height = 7.min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = modal_block(" New profile ", theme::color(Token::Accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Span::styled("Profile name", theme::fg(Token::TextMuted))),
        rows[0],
    );
    let field = Line::from(vec![
        Span::raw("› "),
        Span::styled(input.to_owned(), theme::fg_bold(Token::Accent)),
        Span::styled(theme::glyph(Symbol::BarThin), theme::fg(Token::Accent)),
    ]);
    frame.render_widget(Paragraph::new(field), rows[1]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "enter create · esc cancel",
            theme::fg(Token::TextMuted),
        )),
        rows[3],
    );
}

pub(crate) fn render_confirm_delete_profile(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    id: &str,
    focus: usize,
) {
    let width = 52.min(area.width.saturating_sub(4));
    let height = 8.min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);

    let cancel_style = if focus == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(theme::color(Token::TextBright))
            .add_modifier(Modifier::BOLD)
    } else {
        theme::fg(Token::TextMuted)
    };
    let delete_style = if focus == 1 {
        Style::default()
            .fg(theme::color(Token::TextBright))
            .bg(theme::color(Token::StateDanger))
            .add_modifier(Modifier::BOLD)
    } else {
        theme::fg_bold(Token::StateDanger)
    };

    let message = Line::from(vec![
        Span::raw("Delete profile "),
        Span::styled(id.to_owned(), theme::fg_bold(Token::StateDanger)),
        Span::raw("?"),
    ]);
    let hint = Line::from(Span::styled(
        "This only removes UZE's own record — no harness config is touched.",
        theme::fg(Token::TextMuted),
    ));
    let buttons = Line::from(vec![
        Span::styled("  Cancel  ", cancel_style),
        Span::raw("  "),
        Span::styled("  Delete  ", delete_style),
    ]);
    let footer = Line::from(Span::styled(
        "tab switch · enter confirm · esc cancel · y/n",
        theme::fg(Token::TextMuted),
    ));

    let block = modal_block(" Delete profile? ", theme::color(Token::StateDanger));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(message).alignment(Alignment::Center),
        inner_layout[0],
    );
    frame.render_widget(
        Paragraph::new(hint).alignment(Alignment::Center),
        inner_layout[1],
    );
    frame.render_widget(
        Paragraph::new(buttons).alignment(Alignment::Center),
        inner_layout[3],
    );
    frame.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        inner_layout[4],
    );
}

pub(crate) fn render_confirm_context_apply(frame: &mut ratatui::Frame<'_>, area: Rect) {
    render_modal(
        frame,
        area,
        "Apply context changes?",
        vec![
            Line::from("This reconciles AGENTS.md and its harness bridges."),
            Line::from(Span::styled(
                "enter/y apply · esc/n cancel",
                theme::fg(Token::TextMuted),
            )),
        ],
        theme::color(Token::StateWarning),
    );
}

pub(crate) fn render_trust_required(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    plugin: &str,
    detail: &str,
) {
    render_modal(
        frame,
        area,
        "Trust required",
        vec![
            Line::from(vec![
                Span::styled(plugin.to_owned(), theme::fg_bold(Token::StateWarning)),
                Span::raw(" declares an executable capability that was not previously trusted:"),
            ]),
            Line::from(Span::styled(detail.to_owned(), theme::fg(Token::TextMuted))),
            Line::from(""),
            Line::from(Span::styled(
                "enter/y trust and continue · esc/n cancel",
                theme::fg(Token::TextMuted),
            )),
        ],
        theme::color(Token::StateWarning),
    );
}

fn render_modal(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    color: Color,
) {
    let width = area.width.min(76);
    let height = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(modal_block(format!(" {title} "), color))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        popup,
    );
}

/// The modal dialog surface: `theme::color(Token::SurfaceBackground)`-colored (so it reads as "still part of
/// this app", not a different layer) with a thin hairline border — the
/// only place in the whole UI a content box gets a full border, since a
/// dialog genuinely needs to visually separate from whatever is behind it.
/// Callers must render `Clear` over `popup` first so leftover content
/// underneath can't bleed through.
fn modal_block(title: impl Into<Line<'static>>, color: Color) -> Block<'static> {
    Block::default()
        .title(title)
        .title_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(theme::fg(Token::BorderDefault))
        .style(theme::bg(Token::SurfaceBackground))
        .padding(Padding::new(1, 1, 1, 0))
}
