//! TUI — overlay state transitions and their rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
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
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Danger,
            title: "Remove plugin",
            subject: Some(Line::from(id.to_owned())),
            body: vec![
                "Takes back everything it delivered to each harness. If any of it was changed \
                 by hand, nothing is removed."
                    .to_owned(),
            ],
            confirm: Some("Remove"),
            focus: Some(focus),
        },
        hits,
    );
}

pub(crate) fn render_protected_plugin(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    id: &str,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Caution,
            title: "Protected plugin",
            subject: Some(Line::from(id.to_owned())),
            body: vec![
                "An official marketplace plugin can't be removed from here. Install it from a \
                 custom source to make it removable."
                    .to_owned(),
            ],
            confirm: None,
            focus: None,
        },
        hits,
    );
}

pub(crate) fn render_confirm_update(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    id: &str,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Neutral,
            title: "Update plugin",
            subject: Some(Line::from(id.to_owned())),
            body: vec!["Moves it to the latest revision its marketplace publishes.".to_owned()],
            confirm: Some("Update"),
            focus: None,
        },
        hits,
    );
}

pub(crate) fn render_confirm_clear_prompt_history(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Danger,
            title: "Clear prompt history",
            subject: None,
            body: vec![
                "Deletes every prompt recorded for this workspace. This cannot be undone."
                    .to_owned(),
            ],
            confirm: Some("Clear"),
            focus: None,
        },
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
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Neutral,
            title: "Install plugin",
            subject: Some(Line::from(vec![
                Span::raw(name.to_owned()),
                Span::styled(format!("  from {marketplace}"), theme::fg(Token::TextMuted)),
            ])),
            body: vec!["Delivered to every harness on this machine, from one copy.".to_owned()],
            confirm: Some("Install"),
            focus: None,
        },
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
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Danger,
            title: "Delete profile",
            subject: Some(Line::from(id.to_owned())),
            body: vec![
                "Removes UZE's own record of this profile. No harness configuration is touched."
                    .to_owned(),
            ],
            confirm: Some("Delete"),
            focus: Some(focus),
        },
        hits,
    );
}

pub(crate) fn render_confirm_context_apply(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    hits: &mut Vec<(Rect, Hit)>,
) {
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Caution,
            title: "Apply context changes",
            subject: None,
            body: vec!["Reconciles AGENTS.md and the bridge each harness reads.".to_owned()],
            confirm: Some("Apply"),
            focus: None,
        },
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
    render_dialog(
        frame,
        area,
        &Dialog {
            tone: Tone::Caution,
            title: "Trust required",
            subject: Some(Line::from(plugin.to_owned())),
            body: vec![
                "It declares an executable capability that was not trusted before:".to_owned(),
                detail.to_owned(),
            ],
            confirm: Some("Trust and continue"),
            focus: None,
        },
        hits,
    );
}

/// How much is at stake in a dialog's answer. It colours the thing being
/// acted on and the button that acts — the only two places the answer
/// lands — and nothing else, so the dialog reads calm until the eye
/// reaches what it would do.
#[derive(Clone, Copy)]
enum Tone {
    Neutral,
    Caution,
    Danger,
}

impl Tone {
    fn token(self) -> Token {
        match self {
            Self::Neutral => Token::Accent,
            Self::Caution => Token::StateWarning,
            Self::Danger => Token::StateDanger,
        }
    }
}

/// A question the operator answers, or a notice they dismiss.
///
/// Every dialog is the same four things in the same order — what is being
/// asked, of what, what it means, and the answers — so that someone who
/// has read one knows where to look in the next.
struct Dialog<'a> {
    tone: Tone,
    /// What is being asked, as a heading: "Delete profile".
    title: &'a str,
    /// The thing it would happen to, when there is one: the profile's id.
    subject: Option<Line<'static>>,
    /// What answering yes does, one paragraph per entry.
    body: Vec<String>,
    /// The affirmative, in its own word — "Delete", not "OK". `None` makes
    /// the dialog a notice with one way out.
    confirm: Option<&'a str>,
    /// Which answer the keyboard is on, for the dialogs that carry one.
    focus: Option<usize>,
}

/// The widest a dialog is drawn: a sentence across a whole terminal is
/// read as a strip, not a sentence.
const DIALOG_WIDTH: u16 = 60;
/// The breathing room between the border and everything inside it.
const DIALOG_PAD_X: u16 = 3;

/// A dialog, laid out from its content: a heading and its subject, the
/// explanation wrapped to the dialog's measure, and the answers on the
/// right, the affirmative last — where the eye ends up after reading. How
/// to answer from the keyboard sits in the bottom border, out of the way
/// of the reading. The height follows the wrapped text, so nothing is cut.
fn render_dialog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    dialog: &Dialog<'_>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let width = DIALOG_WIDTH.min(area.width.saturating_sub(4));
    let measure = usize::from(width.saturating_sub(2 + DIALOG_PAD_X * 2).max(1));
    let hue = dialog.tone.token();

    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(
            dialog.title.to_owned(),
            theme::fg_bold(Token::TextBright),
        )),
    ];
    if let Some(subject) = &dialog.subject {
        let mut subject = subject.clone();
        if let Some(first) = subject.spans.first_mut() {
            first.style = theme::fg_bold(hue);
        }
        lines.push(subject);
    }
    for (index, paragraph) in dialog.body.iter().enumerate() {
        lines.push(Line::default());
        let style = if index == 0 {
            theme::fg(Token::TextSecondary)
        } else {
            theme::fg(Token::TextMuted)
        };
        lines.extend(
            crate::ui::wrap_words(paragraph, measure)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, style))),
        );
    }
    lines.push(Line::default());
    let buttons_row = lines.len() as u16;
    // The row the buttons are drawn over, then the same air below them as
    // above the heading.
    lines.push(Line::default());
    lines.push(Line::default());

    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::fg(Token::BorderDefault))
        .style(theme::bg(Token::SurfaceBackground))
        .title_bottom(dialog_hint(dialog).right_aligned())
        .padding(Padding::horizontal(DIALOG_PAD_X));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
    if buttons_row < inner.height {
        render_dialog_buttons(
            frame,
            Rect::new(inner.x, inner.y + buttons_row, inner.width, 1),
            dialog,
            hits,
        );
    }
}

/// `y delete · esc cancel` — the dialog's own words for its answers, with
/// whichever keys reach them now.
fn dialog_hint(dialog: &Dialog<'_>) -> Line<'static> {
    let keymap = uze_keys::active();
    let scopes = [uze_keys::Scope::Global, uze_keys::Scope::Confirm];
    let answers = match dialog.confirm {
        Some(confirm) => vec![
            (uze_keys::Action::ConfirmYes, confirm.to_lowercase()),
            (uze_keys::Action::Dismiss, "cancel".to_owned()),
        ],
        None => vec![(uze_keys::Action::Dismiss, "close".to_owned())],
    };
    let mut spans = vec![Span::raw(" ")];
    for (index, (action, word)) in answers.into_iter().enumerate() {
        let Some(chord) = keymap.chord_for(action, &scopes) else {
            continue;
        };
        if index > 0 {
            spans.push(Span::styled(
                format!(" {} ", theme::glyph(Symbol::HintSeparator)),
                theme::fg(Token::TextDim),
            ));
        }
        spans.push(Span::styled(
            chord.to_string(),
            theme::fg(Token::TextSecondary),
        ));
        spans.push(Span::styled(
            format!(" {word}"),
            theme::fg(Token::TextMuted),
        ));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// The answers, right-aligned, as targets: the way out first, the
/// affirmative last. The one the keyboard is on is drawn solid, the other
/// soft — the same two weights every button in the product has. With no
/// focus to carry, the affirmative is the solid one, which is what a
/// question looks like before anything has moved.
fn render_dialog_buttons(
    frame: &mut ratatui::Frame<'_>,
    row: Rect,
    dialog: &Dialog<'_>,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let cancel = if dialog.confirm.is_some() {
        "  Cancel  "
    } else {
        "  Close  "
    };
    let confirm = dialog.confirm.map(|label| format!("  {label}  "));
    let on_cancel = dialog.focus == Some(0) || dialog.confirm.is_none();
    let mut buttons: Vec<(String, Style, uze_keys::Action)> = vec![(
        cancel.to_owned(),
        crate::ui::view::button_style(Token::TextSecondary, on_cancel, Token::SurfaceBackground),
        uze_keys::Action::ConfirmNo,
    )];
    if let Some(label) = confirm {
        buttons.push((
            label,
            crate::ui::view::button_style(
                dialog.tone.token(),
                !on_cancel,
                Token::SurfaceBackground,
            ),
            uze_keys::Action::ConfirmYes,
        ));
    }
    let gap = 2;
    let total: u16 = buttons
        .iter()
        .map(|(label, ..)| label.chars().count() as u16)
        .sum::<u16>()
        + gap * (buttons.len() as u16 - 1);
    if row.width < total {
        return;
    }
    let mut x = row.right() - total;
    // Prepended, because the dialog is drawn over whatever was behind it
    // and that is still in the hit list underneath.
    let mut targets = Vec::new();
    for (label, style, action) in buttons {
        let rect = Rect::new(x, row.y, label.chars().count() as u16, 1);
        frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
        targets.push((rect, Hit::OfferedAction(action)));
        x += rect.width + gap;
    }
    hits.splice(0..0, targets);
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
