//! TUI view — shared helpers used by every route's render function.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::application::PluginCapability;

use super::{BORDER, MUTED};

pub mod extensions;
pub mod harnesses;
pub mod health;
pub mod overview;
pub mod plugins;
pub mod profiles;

/// The design's `selectedPackage.resources` field is a single flat string
/// ("README, CHANGELOG") — this mirrors that exactly: every capability's
/// own logical/file name, comma-joined, in the order the manifest declared
/// them. Not grouped by kind — the design doesn't, and a plugin rarely
/// declares enough resources for that grouping to earn its own visual
/// weight the way it would in a package-manager UI.
pub(crate) fn resource_summary(capabilities: &[PluginCapability]) -> String {
    if capabilities.is_empty() {
        return "—".to_owned();
    }
    capabilities
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The drawer's bottom status block: a `BORDER`-colored top divider, then a
/// colored dot + bold status text, then a muted note beneath — exactly the
/// design's `border-top` + dot + text status footer, no card, no box.
pub(crate) fn render_status_line(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    color: Color,
    headline: &str,
    subtitle: &str,
) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(BORDER));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(vec![
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(
                headline.to_owned(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            subtitle.to_owned(),
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}
