//! Contextual harness support dropdown for an active agent session.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph},
};
use uze_core::{
    capability::CapabilityKind, integration::AttachmentState, router::HarnessCapabilities,
};

use crate::application::{
    HarnessContextDelivery, HarnessHealth, ProfileSummary, ProjectContextStatus,
};

use super::{ACCENT, BASE, BORDER, MUTED, TEXT_BRIGHT, WARNING};

/// The small, immutable slice of the integrations read model needed by a
/// workspace agent tab. It deliberately comes from `HarnessHealth`, the same
/// application read model the Integrations screen renders.
pub(super) struct AgentSupport {
    integration: String,
    display_name: String,
    present: bool,
    capabilities: HarnessCapabilities,
    agents_md: State,
    agents_md_label: &'static str,
    agents_directory: State,
    agents_directory_label: &'static str,
    profile: String,
}

#[derive(Clone, Copy)]
enum State {
    Ready,
    Warning,
    Error,
}

impl AgentSupport {
    pub(super) fn from_health(
        health: HarnessHealth,
        context: Option<&ProjectContextStatus>,
        agents_directory_loaded: bool,
        profile: Option<&ProfileSummary>,
    ) -> Self {
        // `delivery` only observes the *persistent* on-disk bridge file —
        // it says nothing about UZE's experimental runtime PATH shim, which
        // can project `AGENTS.md` into a session without ever writing into
        // the project. `runtime_projection_active` is the real, live answer
        // to "would this harness actually receive it right now", computed
        // by asking the integration the same question the shim itself asks
        // at launch (see `HarnessContextStatus::runtime_projection_active`).
        let harness_context = context.and_then(|status| {
            status
                .harnesses
                .iter()
                .find(|item| item.integration == health.integration)
        });
        let runtime_projection_active =
            harness_context.is_some_and(|item| item.runtime_projection_active);
        let (agents_md, agents_md_label) = if runtime_projection_active {
            (State::Ready, "loaded (shim)")
        } else {
            match harness_context.map(|item| &item.delivery) {
                Some(HarnessContextDelivery::Native) => (State::Ready, "loaded"),
                Some(HarnessContextDelivery::Bridge {
                    state: AttachmentState::Matched,
                    ..
                }) => (State::Ready, "loaded"),
                Some(HarnessContextDelivery::Bridge { .. }) => (State::Warning, "not loaded"),
                Some(HarnessContextDelivery::NotDetected) | None => (State::Error, "unavailable"),
            }
        };
        // A project's `.agents/{skills,agents}/` reaches this harness one
        // of two ways: natively, on its own — Codex and OpenCode walk cwd
        // up to it, Antigravity reads it per-workspace, all confirmed
        // against each vendor's own docs (see `IntegrationPort::
        // discovers_project_agents_directory`) — or, for Claude Code
        // specifically, through the same runtime-shim projection already
        // checked above for AGENTS.md: `claude/runtime.rs` mirrors
        // `.agents/skills`/`.agents/agents` into `.claude/skills`/
        // `.claude/agents` inside the `--add-dir` target it already passes,
        // which Claude Code auto-discovers with no extra flag. Either path
        // needs the directory to actually be there to deliver anything.
        let agents_directory_deliverable =
            health.project_agents_directory_native || runtime_projection_active;
        let (agents_directory, agents_directory_label) = if !agents_directory_deliverable {
            (State::Error, "not supported")
        } else if !agents_directory_loaded {
            (State::Warning, "not found")
        } else if health.project_agents_directory_native {
            (State::Ready, "loaded")
        } else {
            (State::Ready, "loaded (shim)")
        };
        Self {
            integration: health.integration,
            display_name: health.display_name,
            present: health.detection.present,
            capabilities: health.capabilities,
            agents_md,
            agents_md_label,
            agents_directory,
            agents_directory_label,
            profile: profile
                .map(|profile| profile.id.clone())
                .unwrap_or_else(|| "default".to_owned()),
        }
    }

    pub(super) fn integration(&self) -> &str {
        &self.integration
    }
}

pub(super) fn render(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    anchor: Rect,
    support: &AgentSupport,
) {
    // Interior padding beyond the border itself — this used to sit flush
    // against the frame, which read as cramped once the horizontal rules
    // below were removed and had nothing to separate the sections but
    // whitespace.
    const H_PAD: u16 = 2;
    const V_PAD: u16 = 1;

    let width = 40.min(area.width).max(1);
    let inner_width = width.saturating_sub(2 + 2 * H_PAD) as usize;

    let mut lines = vec![title_row(inner_width), Line::default()];

    lines.push(section_header("RUNTIME"));
    lines.push(fact_line(
        harness_state(support),
        "Harness",
        &support.display_name,
        inner_width,
    ));
    lines.push(fact_line(
        support.agents_md,
        "AGENTS.md",
        support.agents_md_label,
        inner_width,
    ));
    lines.push(fact_line(
        support.agents_directory,
        ".agents",
        support.agents_directory_label,
        inner_width,
    ));
    lines.push(fact_line(
        State::Ready,
        "Profile",
        &support.profile,
        inner_width,
    ));

    lines.push(Line::default());
    lines.push(section_header("CAPABILITIES"));

    // Policy is a known `CapabilityKind` but no integration populates it yet
    // (see `crates/uze-integrations`) — showing it here would always read
    // "unavailable" regardless of the harness, which isn't real data.
    for capability in [
        CapabilityKind::AgentSkill,
        CapabilityKind::Mcp,
        CapabilityKind::Hook,
        CapabilityKind::Agent,
    ] {
        let state = capability_state(support, capability);
        lines.push(capability_line(
            state,
            capability_label(capability),
            capability_status_label(state),
            inner_width,
        ));
        if matches!(state, State::Error) {
            lines.push(reason_line(support, capability, inner_width));
        }
    }

    let height = (lines.len() as u16 + 2 + 2 * V_PAD).min(area.height).max(1);
    let popup = Rect::new(
        anchor
            .x
            .saturating_sub(width.saturating_sub(anchor.width))
            .min(area.right().saturating_sub(width)),
        (anchor.y + anchor.height).min(area.bottom().saturating_sub(height)),
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BASE))
        .padding(Padding::new(H_PAD, H_PAD, V_PAD, V_PAD));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    frame.render_widget(Paragraph::new(lines), inner);
}

fn harness_state(support: &AgentSupport) -> State {
    if support.present {
        State::Ready
    } else {
        State::Error
    }
}

fn title_row(width: usize) -> Line<'static> {
    let left = "support";
    let right = "esc";
    let gap = width.saturating_sub(left.len() + right.len()).max(1);
    Line::from(vec![
        Span::styled(
            left,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, Style::default().fg(MUTED)),
    ])
}

fn section_header(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
    ))
}

/// Lays out one `<icon> <label> ... <value>` row, right-aligning `value`
/// within `width` — the shape every row in this popup shares. `fact_line`
/// and `capability_line` only differ in which styles they hand in for
/// `label`/`value`; the icon, clipping, and gap math live here once.
fn styled_row(
    state: State,
    label: &str,
    label_style: Style,
    value: &str,
    value_style: Style,
    width: usize,
) -> Line<'static> {
    let (icon, icon_color) = icon_for(state);
    let value = clip(value, width.saturating_sub(3));
    let gap = width
        .saturating_sub(2 + label.chars().count() + value.chars().count())
        .max(1);
    Line::from(vec![
        Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
        Span::styled(label.to_owned(), label_style),
        Span::raw(" ".repeat(gap)),
        Span::styled(value, value_style),
    ])
}

/// A runtime fact row: label and value both read as plain information, only
/// the leading icon carries state color — used for things like the active
/// profile, never anything the user needs to act on.
fn fact_line(state: State, label: &str, value: &str, width: usize) -> Line<'static> {
    let plain = Style::default().fg(TEXT_BRIGHT);
    styled_row(state, label, plain, value, plain, width)
}

/// A capability status row: the value color itself carries the severity —
/// muted for the unremarkable "supported"/"limited" states, a loud danger
/// color for "unavailable" — and an unavailable capability's own label is
/// struck through to read as switched off.
fn capability_line(state: State, label: &str, value: &str, width: usize) -> Line<'static> {
    let label_style = match state {
        State::Error => Style::default()
            .fg(MUTED)
            .add_modifier(Modifier::CROSSED_OUT),
        _ => Style::default().fg(TEXT_BRIGHT),
    };
    let value_style = match state {
        State::Ready | State::Warning => Style::default().fg(MUTED),
        State::Error => Style::default()
            .fg(super::DANGER)
            .add_modifier(Modifier::BOLD),
    };
    styled_row(state, label, label_style, value, value_style, width)
}

fn reason_line(support: &AgentSupport, capability: CapabilityKind, width: usize) -> Line<'static> {
    let text = format!(
        "{} does not expose {}",
        support.display_name,
        capability_label(capability).to_lowercase()
    );
    Line::from(Span::styled(
        format!("  {}", clip(&text, width.saturating_sub(2))),
        Style::default().fg(MUTED),
    ))
}

fn icon_for(state: State) -> (&'static str, ratatui::style::Color) {
    match state {
        State::Ready => ("✓", ACCENT),
        State::Warning => ("!", WARNING),
        State::Error => ("✕", super::DANGER),
    }
}

fn capability_state(support: &AgentSupport, kind: CapabilityKind) -> State {
    let capabilities = &support.capabilities;
    if capabilities.direct_standard.contains(&kind) || capabilities.native.contains(&kind) {
        State::Ready
    } else if capabilities.adaptable.contains(&kind) || capabilities.degraded.contains(&kind) {
        State::Warning
    } else {
        State::Error
    }
}

fn capability_status_label(state: State) -> &'static str {
    match state {
        State::Ready => "supported",
        State::Warning => "limited",
        State::Error => "unavailable",
    }
}

fn clip(value: &str, max: usize) -> String {
    let mut clipped = value.chars().take(max).collect::<String>();
    if value.chars().count() > max {
        clipped.pop();
        clipped.push('…');
    }
    clipped
}

fn capability_label(kind: CapabilityKind) -> &'static str {
    match kind {
        CapabilityKind::Instruction => "Instructions",
        CapabilityKind::AgentSkill => "Skills",
        CapabilityKind::Mcp => "MCP",
        CapabilityKind::Agent => "Agents",
        CapabilityKind::Hook => "Hooks",
        CapabilityKind::Policy => "Policies",
    }
}
