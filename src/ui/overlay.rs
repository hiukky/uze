//! TUI — overlay state transitions and their rendering.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};

use uze_keys::Action;

use super::hit::Hit;
use super::model::{Focus, Overlay, TrustedRetry, TuiModel};
use super::worker::{Intent, TrustGrant};
use crate::ui::theme::{self, Symbol, Token};

impl TuiModel {
    /// One action, answered by whichever overlay is open.
    ///
    /// Only the actions an overlay's own surface offers reach it — a
    /// question on screen is answered, dismissed, or left alone, and a
    /// keystroke that means nothing here no longer closes it by accident.
    pub(crate) fn overlay_action(&mut self, action: Action) -> Intent {
        let overlay = self.overlay.clone();
        match overlay {
            Overlay::None | Overlay::HarnessHelp => Intent::None,
            Overlay::ActionIndex {
                scopes,
                filter,
                selected,
            } => {
                let rows = self.action_index_rows(&scopes, &filter);
                match action {
                    Action::SelectNext => {
                        if let Overlay::ActionIndex { selected, .. } = &mut self.overlay {
                            *selected = (*selected + 1).min(rows.len().saturating_sub(1));
                        }
                        Intent::None
                    }
                    Action::SelectPrevious => {
                        if let Overlay::ActionIndex { selected, .. } = &mut self.overlay {
                            *selected = selected.saturating_sub(1);
                        }
                        Intent::None
                    }
                    Action::Activate => {
                        let chosen = rows.get(selected).map(|(action, _)| *action);
                        self.close_overlay();
                        match chosen {
                            // Performing from the index is performing: the
                            // row that did it is also the row that showed
                            // the key, which is how anyone learns one.
                            Some(action) => self.act(action),
                            None => Intent::None,
                        }
                    }
                    Action::Dismiss => {
                        self.close_overlay();
                        Intent::None
                    }
                    Action::EraseBack => self.erase_character(),
                    _ => Intent::None,
                }
            }
            Overlay::ConfirmRemove { id, focus } => match action {
                Action::FocusNext | Action::FocusPrevious => {
                    self.overlay = Overlay::ConfirmRemove {
                        id,
                        focus: 1 - focus,
                    };
                    Intent::None
                }
                Action::Activate if focus == 1 => {
                    self.close_overlay();
                    Intent::Remove(id)
                }
                Action::ConfirmYes => {
                    self.close_overlay();
                    Intent::Remove(id)
                }
                Action::Activate | Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::ConfirmDeleteProfile { id, focus } => match action {
                Action::FocusNext | Action::FocusPrevious => {
                    self.overlay = Overlay::ConfirmDeleteProfile {
                        id,
                        focus: 1 - focus,
                    };
                    Intent::None
                }
                Action::Activate if focus == 1 => {
                    self.close_overlay();
                    Intent::DeleteProfile(id)
                }
                Action::ConfirmYes => {
                    self.close_overlay();
                    Intent::DeleteProfile(id)
                }
                Action::Activate | Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::ThemePicker { themes, selected } => match action {
                Action::SelectNext => {
                    let last = themes.len().saturating_sub(1);
                    self.overlay = Overlay::ThemePicker {
                        themes,
                        selected: (selected + 1).min(last),
                    };
                    Intent::None
                }
                Action::SelectPrevious => {
                    self.overlay = Overlay::ThemePicker {
                        themes,
                        selected: selected.saturating_sub(1),
                    };
                    Intent::None
                }
                Action::Activate => {
                    let chosen = themes.get(selected).cloned();
                    self.close_overlay();
                    match chosen {
                        Some((id, _)) => Intent::SelectTheme(id),
                        None => Intent::None,
                    }
                }
                Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::ConfirmClearPromptHistory => match action {
                Action::Activate | Action::ConfirmYes => {
                    self.close_overlay();
                    Intent::ClearPromptHistory
                }
                Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::ConfirmUpdate(id) => match action {
                Action::Activate | Action::ConfirmYes => {
                    self.close_overlay();
                    Intent::Update(id, TrustGrant::Ask)
                }
                Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::ConfirmInstall { name, marketplace } => match action {
                Action::Activate | Action::ConfirmYes => {
                    self.close_overlay();
                    Intent::Install {
                        name,
                        marketplace,
                        grant: TrustGrant::Ask,
                    }
                }
                Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::ConfirmContextApply => match action {
                Action::Activate | Action::ConfirmYes => {
                    self.close_overlay();
                    Intent::ContextApply(self.workspace_root())
                }
                Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            // Nothing to decide: it explains why an action was refused.
            Overlay::ProtectedPlugin(_) => match action {
                Action::Activate | Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::AddMarketplace(input) => match action {
                Action::Activate => {
                    let source = input.trim().to_owned();
                    self.close_overlay();
                    if source.is_empty() {
                        Intent::None
                    } else {
                        Intent::AddMarketplace(source)
                    }
                }
                Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                Action::EraseBack => {
                    let mut input = input;
                    input.pop();
                    self.overlay = Overlay::AddMarketplace(input);
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::NewProfile(input) => match action {
                Action::Activate => {
                    let id = slugify(&input);
                    self.close_overlay();
                    if id.is_empty() {
                        Intent::None
                    } else {
                        Intent::CreateProfile(id)
                    }
                }
                Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                Action::EraseBack => {
                    let mut input = input;
                    input.pop();
                    self.overlay = Overlay::NewProfile(input);
                    Intent::None
                }
                _ => Intent::None,
            },
            Overlay::TrustRequired { retry, .. } => match action {
                Action::Activate | Action::ConfirmYes => {
                    let intent = match retry {
                        TrustedRetry::Install { name, marketplace } => Intent::Install {
                            name,
                            marketplace,
                            grant: TrustGrant::Granted,
                        },
                        TrustedRetry::Update(id) => Intent::Update(id, TrustGrant::Granted),
                    };
                    self.close_overlay();
                    intent
                }
                Action::ConfirmNo | Action::Dismiss => {
                    self.close_overlay();
                    Intent::None
                }
                _ => Intent::None,
            },
        }
    }

    pub(crate) fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
        self.focus = Focus::Content;
    }
}

/// Everything that can be done here, each with the key that reaches it.
///
/// This is the help and the command palette at once, because they answer
/// the same question and two lists would eventually disagree. Nothing here
/// is written down: every row's words come from the action and every key
/// from the keymap, so a rebound key is right here without anyone editing
/// this function — which is exactly what the hand-typed list it replaced
/// could not promise, and had already broken for nine of its bindings.
pub(crate) fn render_action_index(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    scopes: &[uze_keys::Scope],
    filter: &str,
    selected: usize,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let rows = model.action_index_rows(scopes, filter);
    let key_width = rows
        .iter()
        .filter_map(|(_, chord)| chord.map(|chord| chord.to_string().chars().count()))
        .max()
        .unwrap_or(0)
        .max(4);
    let width = area.width.saturating_sub(8).clamp(30, 72);
    let height = (rows.len() as u16 + 5).min(area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        modal_block(" Everything you can do ", theme::color(Token::Accent)),
        rect,
    );
    let inner = Rect::new(
        rect.x + 2,
        rect.y + 1,
        rect.width.saturating_sub(4),
        rect.height.saturating_sub(2),
    );

    let typed = if filter.is_empty() {
        Line::from(Span::styled("type to narrow", theme::fg(Token::TextMuted)))
    } else {
        Line::from(vec![
            Span::styled(filter.to_owned(), theme::fg(Token::TextPrimary)),
            Span::styled(theme::glyph(Symbol::BarThin), theme::fg(Token::Accent)),
        ])
    };
    frame.render_widget(
        Paragraph::new(typed),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let mut entries: Vec<(Rect, Hit)> = Vec::new();
    for (index, (action, chord)) in rows.iter().enumerate() {
        let y = list.y + index as u16;
        if y >= list.bottom() {
            break;
        }
        let row = Rect::new(list.x, y, list.width, 1);
        let chosen = index == selected;
        let key = match chord {
            Some(chord) => chord.to_string(),
            // An action with no key is a finished design, not a gap — it
            // is reached by pointer and from here.
            None => String::new(),
        };
        let mut label = Style::default().fg(theme::color(if chosen {
            Token::TextBright
        } else if action.destructive() {
            Token::StateDanger
        } else {
            Token::TextPrimary
        }));
        if chosen {
            label = label.add_modifier(Modifier::BOLD);
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{key:<key_width$}  "),
                    theme::fg(if chord.is_some() {
                        Token::Accent
                    } else {
                        Token::TextDim
                    }),
                ),
                Span::styled(action.label(), label),
                Span::styled(
                    format!(
                        "  {} {}",
                        theme::glyph(Symbol::EmDash),
                        action.description()
                    ),
                    theme::fg(Token::TextMuted),
                ),
            ])),
            row,
        );
        entries.push((row, Hit::ActionIndexEntry(index)));
    }
    // Prepended, so the list underneath cannot answer a click meant here.
    hits.splice(0..0, entries);
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
            Symbol::MarkOk,
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
            Symbol::MarkAttention,
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
    hits: &mut Vec<(Rect, Hit)>,
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
    let footer = crate::ui::hint_for(
        &[uze_keys::Scope::Global, uze_keys::Scope::Confirm],
        &[
            uze_keys::Action::FocusNext,
            uze_keys::Action::ConfirmYes,
            uze_keys::Action::ConfirmNo,
        ],
    );

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
    render_modal_buttons(
        frame,
        inner_layout[3],
        Some("Remove"),
        theme::color(Token::StateDanger),
        Some(focus),
        hits,
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

pub(crate) fn render_confirm_update(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    id: &str,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_modal(
        frame,
        area,
        "Update plugin?",
        vec![Line::from(vec![
            Span::raw("Update "),
            Span::styled(id.to_owned(), theme::fg_bold(Token::Accent)),
            Span::raw(" to the latest marketplace revision?"),
        ])],
        theme::color(Token::StateWarning),
        Some("Update"),
        hits,
    );
}

pub(crate) fn render_confirm_clear_prompt_history(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_modal(
        frame,
        area,
        "Clear prompt history?",
        vec![Line::from(Span::raw(
            "Delete every recorded prompt for this workspace. This cannot be undone.",
        ))],
        theme::color(Token::StateDanger),
        Some("Clear"),
        hits,
    );
}

pub(crate) fn render_confirm_install(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    name: &str,
    marketplace: &str,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_modal(
        frame,
        area,
        "Install plugin?",
        vec![Line::from(vec![
            Span::raw("Install "),
            Span::styled(name.to_owned(), theme::fg_bold(Token::Accent)),
            Span::raw(" from "),
            Span::styled(marketplace.to_owned(), theme::fg(Token::TextMuted)),
            Span::raw("?"),
        ])],
        theme::color(Token::Accent),
        Some("Install"),
        hits,
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
    lines.push(crate::ui::hint_for(
        &[uze_keys::Scope::Global, uze_keys::Scope::ThemePicker],
        &[
            uze_keys::Action::SelectNext,
            uze_keys::Action::Activate,
            uze_keys::Action::Dismiss,
        ],
    ));
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
    hits: &mut Vec<(Rect, Hit)>,
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

    let message = Line::from(vec![
        Span::raw("Delete profile "),
        Span::styled(id.to_owned(), theme::fg_bold(Token::StateDanger)),
        Span::raw("?"),
    ]);
    let hint = Line::from(Span::styled(
        "This only removes UZE's own record — no harness config is touched.",
        theme::fg(Token::TextMuted),
    ));
    let footer = crate::ui::hint_for(
        &[uze_keys::Scope::Global, uze_keys::Scope::Confirm],
        &[
            uze_keys::Action::FocusNext,
            uze_keys::Action::ConfirmYes,
            uze_keys::Action::ConfirmNo,
        ],
    );

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
    render_modal_buttons(
        frame,
        inner_layout[3],
        Some("Delete"),
        theme::color(Token::StateDanger),
        Some(focus),
        hits,
    );
    frame.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        inner_layout[4],
    );
}

pub(crate) fn render_confirm_context_apply(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_modal(
        frame,
        area,
        "Apply context changes?",
        vec![Line::from(
            "This reconciles AGENTS.md and its harness bridges.",
        )],
        theme::color(Token::StateWarning),
        Some("Apply"),
        hits,
    );
}

pub(crate) fn render_trust_required(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    plugin: &str,
    detail: &str,
    hits: &mut Vec<(Rect, Hit)>,
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
        ],
        theme::color(Token::StateWarning),
        Some("Trust and continue"),
        hits,
    );
}

/// A modal with its own buttons.
///
/// The buttons are the point: a dialog that could only be answered with a
/// key would be the one place in the product where the keyboard is the way
/// in rather than the accelerator. `yes` is the affirmative's own word —
/// "Install", "Remove" — because "OK" tells a reader nothing about what
/// they are about to agree to. `None` makes it a notice with one way out.
fn render_modal(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    color: Color,
    yes: Option<&str>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let width = area.width.min(76);
    let height = (lines.len() as u16 + 6).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = modal_block(format!(" {title} "), color);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(2),
    );
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: true }),
        body,
    );
    render_modal_buttons(
        frame,
        Rect::new(inner.x, inner.y + body.height, inner.width, 1),
        yes,
        color,
        None,
        hits,
    );
    frame.render_widget(
        Paragraph::new(crate::ui::hint_for(
            &[uze_keys::Scope::Global, uze_keys::Scope::Confirm],
            &[uze_keys::Action::ConfirmYes, uze_keys::Action::ConfirmNo],
        ))
        .alignment(Alignment::Center),
        Rect::new(inner.x, inner.y + body.height + 1, inner.width, 1),
    );
}

/// The affirmative and the way out, as targets.
///
/// `focus` is which one the keyboard is on, for the dialogs that carry a
/// focus; `None` draws the affirmative as the filled one, which is what a
/// yes/no question looks like when nothing has moved yet.
fn render_modal_buttons(
    frame: &mut ratatui::Frame<'_>,
    row: Rect,
    yes: Option<&str>,
    color: Color,
    focus: Option<usize>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let cancel = match yes {
        Some(_) => "  Cancel  ",
        None => "  Close  ",
    };
    let yes_label = yes.map(|label| format!("  {label}  "));
    let gap: u16 = if yes_label.is_some() { 2 } else { 0 };
    let total = cancel.chars().count() as u16
        + yes_label.as_ref().map_or(0, |l| l.chars().count() as u16)
        + gap;
    if row.width < total {
        return;
    }
    let filled = |hue: Color| {
        Style::default()
            .fg(theme::color(Token::TextBright))
            .bg(hue)
            .add_modifier(Modifier::BOLD)
    };
    let mut x = row.x + (row.width - total) / 2;
    let cancel_rect = Rect::new(x, row.y, cancel.chars().count() as u16, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(
            cancel,
            if focus == Some(0) {
                filled(theme::color(Token::TextMuted))
            } else {
                theme::fg(Token::TextMuted)
            },
        )),
        cancel_rect,
    );
    // Prepended, because the dialog is drawn over whatever was behind it
    // and that is still in the hit list underneath.
    let mut buttons = vec![(cancel_rect, Hit::OfferedAction(uze_keys::Action::ConfirmNo))];
    x += cancel_rect.width + gap;
    if let Some(label) = yes_label {
        let rect = Rect::new(x, row.y, label.chars().count() as u16, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                label,
                if focus == Some(0) {
                    theme::fg_bold(Token::StateDanger)
                } else {
                    filled(color)
                },
            )),
            rect,
        );
        buttons.push((rect, Hit::OfferedAction(uze_keys::Action::ConfirmYes)));
    }
    hits.splice(0..0, buttons);
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
