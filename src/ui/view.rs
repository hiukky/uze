//! TUI view — extracted without semantic change.

use std::collections::BTreeMap;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::application::PluginCapability;
use uze_core::capability::CapabilityKind;

use super::{ACCENT, MUTED, icon};

pub mod context;
pub mod doctor;
pub mod harnesses;
pub mod marketplace;
pub mod overview;
pub mod plugins;

/// A short, human-facing label for a resource kind — used wherever a
/// plugin's capabilities are grouped for display (never for exposure
/// naming, which is `IntegrationPort::exposure_name_candidates`'s own
/// decision).
pub(crate) fn capability_kind_label(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Instruction => "Instructions",
        CapabilityKind::AgentSkill => "Skills",
        CapabilityKind::Mcp => "MCP Servers",
        CapabilityKind::Agent => "Agents",
        CapabilityKind::Action => "Actions",
        CapabilityKind::Hook => "Hooks",
        CapabilityKind::Policy => "Policies",
    }
}

fn capability_kind_icon(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::AgentSkill => icon::SKILLS,
        CapabilityKind::Agent => icon::AGENTS,
        CapabilityKind::Mcp => icon::MCP,
        _ => "•",
    }
}

/// The three resource kinds every plugin could plausibly declare, always
/// shown in this fixed order (even empty, as a `–` placeholder) so the
/// Resources card reads as a stable table rather than a list that
/// reshuffles/disappears entirely for a plugin with nothing to show yet.
const CORE_KINDS: [CapabilityKind; 3] = [
    CapabilityKind::AgentSkill,
    CapabilityKind::Agent,
    CapabilityKind::Mcp,
];

/// Appends a grouped-by-kind, indented listing of `capabilities` to `lines`,
/// as a bordered card: the three core kinds (Skills/Agents/MCP Servers)
/// always appear, in that order, each as its own icon-labeled group with a
/// `–` placeholder when empty; any other kind actually present
/// (Instructions/Actions/Hooks/Policies) is appended after, only when
/// non-empty. A thin divider separates each group. Only the resource's own
/// logical/file name is shown — an MCP server groups as one row here, since
/// the individual tools it exposes are runtime-discovered by the harness
/// that connects to it, not declared anywhere UZE reads.
pub(crate) fn push_capability_table(lines: &mut Vec<Line<'_>>, capabilities: &[PluginCapability]) {
    let mut grouped: BTreeMap<CapabilityKind, Vec<&PluginCapability>> = BTreeMap::new();
    for capability in capabilities {
        grouped.entry(capability.kind).or_default().push(capability);
    }

    let mut ordered_kinds: Vec<CapabilityKind> = CORE_KINDS.to_vec();
    for kind in grouped.keys() {
        if !ordered_kinds.contains(kind) {
            ordered_kinds.push(*kind);
        }
    }

    let mut first = true;
    for kind in ordered_kinds {
        let items = grouped.get(&kind);
        if items.is_none() && !CORE_KINDS.contains(&kind) {
            continue;
        }
        if !first {
            lines.push(Line::from(Span::styled(
                "─".repeat(20),
                Style::default().fg(MUTED),
            )));
        }
        first = false;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", capability_kind_icon(kind)),
                Style::default().fg(ACCENT),
            ),
            Span::styled(
                capability_kind_label(kind),
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ),
        ]));
        match items {
            Some(items) => {
                for item in items {
                    lines.push(Line::from(format!("  › {}", item.name)));
                }
            }
            None => lines.push(Line::from(Span::styled("  –", Style::default().fg(MUTED)))),
        }
    }
}

/// The bordered, icon-headlined status card pinned under a detail panel's
/// scrollable content — same shape for the Marketplace and Plugins routes,
/// each computing its own `(color, headline, subtitle)` from its own
/// domain state (install/update for a catalog entry; health/update for an
/// installed one).
pub(crate) fn render_status_card(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    color: Color,
    headline: &str,
    subtitle: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", icon::CHECK),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
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
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
