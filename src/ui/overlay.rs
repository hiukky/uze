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
use super::{ACCENT, BASE, BORDER, DANGER, MUTED, TEXT_BRIGHT, WARNING};

impl TuiModel {
    pub(crate) fn overlay_key(&mut self, key: KeyEvent) -> Intent {
        let overlay = self.overlay.clone();
        match (&overlay, key.code) {
            (Overlay::Help, _) | (Overlay::HarnessHelp, _) => {
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
            (Overlay::ConfirmRemove { id, .. }, KeyCode::Char('y') | KeyCode::Char('Y')) => {
                let id = id.clone();
                self.close_overlay();
                Intent::Remove(id)
            }
            (
                Overlay::ConfirmRemove { .. },
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc,
            ) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmRemove { .. }, _) => Intent::None,
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
                Intent::ContextApply(self.context_root.clone())
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
            Line::from("↑↓ / j k     Navigate"),
            Line::from("Tab          Switch focus (sidebar ↔ content)"),
            Line::from("Enter        Open / Inspect"),
            Line::from("Mouse click  Select sidebar route or list row"),
            Line::from("Scroll       Move selection"),
            Line::from("r            Remove plugin (Plugins) · Refresh elsewhere"),
            Line::from("u            Update plugin (Plugins, when available)"),
            Line::from("i            Install plugin (Marketplace)"),
            Line::from("/            Filter marketplace list"),
            Line::from("a            Add marketplace · Analyze (Context)"),
            Line::from("p            Apply (Context)"),
            Line::from("s            Setup harness (Harnesses)"),
            Line::from("g            Refresh"),
            Line::from("q            Quit"),
            Line::from(""),
            Line::from(Span::styled("any key to close", Style::default().fg(MUTED))),
        ],
        ACCENT,
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
    let entry = |glyph: &str, label: &str, color: Color, detail: &str| {
        Line::from(vec![
            Span::styled(
                format!("{glyph} {label:<18}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(detail.to_owned(), Style::default().fg(MUTED)),
        ])
    };
    let heading = |text: &str| {
        Line::from(Span::styled(
            text.to_owned(),
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let lines = vec![
        heading("STATUS"),
        entry(
            "✕",
            "Not installed",
            MUTED,
            "The harness isn't on this machine at all.",
        ),
        entry(
            "●",
            "Installed",
            WARNING,
            "Detected, but UZE hasn't configured it — press s to run setup.",
        ),
        entry(
            "✓",
            "Configured",
            ACCENT,
            "UZE has set it up — ready to receive plugins.",
        ),
        Line::from(""),
        heading("COMPATIBILITY (per capability, in the detail panel)"),
        entry(
            "√",
            "Native",
            ACCENT,
            "Works directly, no adaptation needed.",
        ),
        entry(
            "√",
            "Bridged",
            ACCENT,
            "Routed through UZE's managed AGENTS.md bridge file.",
        ),
        entry(
            "≈",
            "Adapted",
            WARNING,
            "Works, converted from a different format.",
        ),
        entry(
            "≈",
            "Degraded",
            WARNING,
            "Works, but with reduced fidelity.",
        ),
        entry(
            "—",
            "Not supported",
            DANGER,
            "This harness has no route for it.",
        ),
        entry(
            "—",
            "Not implemented",
            MUTED,
            "UZE doesn't route this capability anywhere yet.",
        ),
        Line::from(""),
        Line::from(Span::styled("any key to close", Style::default().fg(MUTED))),
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
            .block(modal_block(" Harness status ", ACCENT))
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
            .bg(TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let remove_style = if focus == 1 {
        Style::default()
            .fg(TEXT_BRIGHT)
            .bg(DANGER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
    };

    let message = Line::from(vec![
        Span::raw("Remove "),
        Span::styled(
            id.to_owned(),
            Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
        ),
        Span::raw("?"),
    ]);
    let hint = Line::from(Span::styled(
        "Only matched artifacts will be detached.",
        Style::default().fg(MUTED),
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
        Style::default().fg(MUTED),
    ));

    let block = modal_block(" Remove plugin? ", DANGER);
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
            Span::styled(
                id.to_owned(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" is an official marketplace plugin"),
        ]),
        Line::from(Span::styled(
            "and cannot be removed from the TUI.",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Use a custom source for removable plugins.",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "esc / enter to dismiss",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(modal_block(" Protected plugin ", WARNING))
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
                Span::styled(
                    id.to_owned(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to the latest marketplace revision?"),
            ]),
            Line::from(Span::styled(
                "enter/y update · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        WARNING,
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
                Span::styled(
                    name.to_owned(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" from "),
                Span::styled(marketplace.to_owned(), Style::default().fg(MUTED)),
                Span::raw("?"),
            ]),
            Line::from(Span::styled(
                "enter/y install · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        ACCENT,
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
    let block = modal_block(" Add marketplace ", ACCENT);
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
            Style::default().fg(MUTED),
        )),
        rows[0],
    );
    let field = Line::from(vec![
        Span::raw("› "),
        Span::styled(
            input.to_owned(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("▏", Style::default().fg(ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(field), rows[1]);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "enter add · esc cancel",
            Style::default().fg(MUTED),
        )),
        rows[3],
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
                Style::default().fg(MUTED),
            )),
        ],
        WARNING,
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
                Span::styled(
                    plugin.to_owned(),
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" declares an executable capability that was not previously trusted:"),
            ]),
            Line::from(Span::styled(detail.to_owned(), Style::default().fg(MUTED))),
            Line::from(""),
            Line::from(Span::styled(
                "enter/y trust and continue · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        WARNING,
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

/// The modal dialog surface: `BASE`-colored (so it reads as "still part of
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
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BASE))
        .padding(Padding::new(1, 1, 1, 0))
}
