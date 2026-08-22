//! TUI — overlay state transitions and their rendering.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph},
};

use super::model::{Focus, Overlay, TrustedRetry, TuiModel};
use super::worker::{Intent, TrustGrant};
use super::{ACCENT, DANGER, MUTED, WARNING, panel_block};

impl TuiModel {
    pub(crate) fn overlay_key(&mut self, key: KeyEvent) -> Intent {
        let overlay = self.overlay.clone();
        match (&overlay, key.code) {
            (Overlay::Help, _) => {
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
            (Overlay::ConfirmInstall(name), KeyCode::Char('y') | KeyCode::Enter) => {
                let name = name.clone();
                self.close_overlay();
                Intent::Install(name, TrustGrant::Ask)
            }
            (Overlay::ConfirmInstall(_), _) => {
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
            (Overlay::TrustRequired { retry, .. }, KeyCode::Char('y') | KeyCode::Enter) => {
                let intent = match retry {
                    TrustedRetry::Install(name) => {
                        Intent::Install(name.clone(), TrustGrant::Granted)
                    }
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
            Line::from("r            Remove plugin (Plugins)"),
            Line::from("u            Update plugin (Plugins, when available)"),
            Line::from("i            Install plugin (Marketplace)"),
            Line::from("a / p        Analyze / Apply (Context)"),
            Line::from("s            Setup harness (Harnesses)"),
            Line::from("g            Refresh"),
            Line::from("q            Quit"),
            Line::from(""),
            Line::from(Span::styled("any key to close", Style::default().fg(MUTED))),
        ],
        ACCENT,
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
            .bg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let remove_style = if focus == 1 {
        Style::default()
            .fg(Color::White)
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

    let block = panel_block(" Remove plugin? ")
        .border_style(Style::default().fg(DANGER))
        .padding(Padding::new(1, 1, 1, 0));
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
            .block(panel_block(" Protected plugin ").border_style(Style::default().fg(WARNING)))
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

pub(crate) fn render_confirm_install(frame: &mut ratatui::Frame<'_>, area: Rect, name: &str) {
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
                Span::raw(" from the official marketplace?"),
            ]),
            Line::from(Span::styled(
                "enter/y install · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        ACCENT,
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
            .block(panel_block(format!(" {title} ")).border_style(Style::default().fg(color)))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        popup,
    );
}
