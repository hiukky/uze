//! TUI view — Overview route.
//!
//! Three layers, deliberately kept apart:
//!
//! 1. The machine dashboard at the top (harnesses detected, plugins
//!    installed, marketplace sources, context bridges, and the global
//!    health line) — unchanged, global state, driven by `DoctorReport`.
//! 2. The workspace lower section: what kind of UZE workspace the cwd is
//!    inside, and whether it is *ready to work*. It reads vertically —
//!    one PROJECT block, then one MARKETPLACE block, never columns. The
//!    rows are semantic states (`Environment`, `Memory`, `Plugins`, and
//!    the marketplace's `Name`/`Plugins`/`Status`) produced by the
//!    Application's `overview_workspace` read model. This view only
//!    renders them — no lock/blob parsing, no state derivation, no
//!    install decisions.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::application::{
    HarnessContextDelivery, MarketplaceState, MemoryState, OverviewWorkspaceSummary, Portability,
    ProjectContextStatus, ProjectEnvironmentState, WorkspaceKind,
};

use super::super::model::TuiModel;
use super::super::{
    ACCENT, BORDER_FAINT, DANGER, MUTED, SUCCESS, TEXT_BRIGHT, TEXT_DIM, TEXT_PRIMARY, WARNING,
};
use super::super::{content_area, render_screen_header};

/// How wide one workspace block is allowed to be. Rows are short
/// (`Environment   ! install required`); capping the width keeps the
/// stacked block compact on wide terminals instead of stretching an
/// empty 100-column line.
const WORKSPACE_BLOCK_MAX_WIDTH: u16 = 36;

pub(crate) fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let area = content_area(area);
    let content = render_screen_header(frame, area, "Overview", "status & health", None);

    let harness_total = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let harness_detected = model.doctor.as_ref().map_or(0, |d| {
        d.harnesses.iter().filter(|h| h.detection.present).count()
    });
    let issues = model.issues().len();
    let bridges = model
        .context_status
        .as_ref()
        .map(|status| {
            status
                .harnesses
                .iter()
                .filter(|h| !matches!(h.delivery, HarnessContextDelivery::NotDetected))
                .count()
        })
        .unwrap_or(0);

    let mut y = content.y;

    // Status line: dot + headline + detail, matching the design's single
    // "All systems healthy — N harnesses detected, ..." summary row.
    let (color, headline) = if issues == 0 {
        (SUCCESS, "All systems healthy")
    } else {
        (WARNING, "Attention needed")
    };
    let detail = if issues == 0 {
        format!(
            "— {harness_detected} harness{} detected, context bridges verified",
            if harness_detected == 1 { "" } else { "es" }
        )
    } else {
        format!("— {issues} issue(s), see Doctor")
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(
                headline,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(detail, Style::default().fg(MUTED)),
        ])),
        Rect::new(content.x, y, content.width, 1),
    );
    y += 3;

    // 4-column stat grid, each cell divided from its neighbor by a left
    // hairline border — the design's `border-left:1px solid rgba(...)`.
    let stats = [
        (
            "Harnesses detected",
            format!("{harness_detected}/{harness_total}"),
        ),
        ("Plugins installed", model.plugins.len().to_string()),
        ("Marketplace sources", model.marketplace_count.to_string()),
        ("Context bridges", bridges.to_string()),
    ];
    if y + 1 < content.y + content.height {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25); 4])
            .split(Rect::new(content.x, y, content.width, 2));
        for (cell, (label, value)) in columns.iter().zip(stats) {
            let block = Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(BORDER_FAINT));
            let inner = block.inner(*cell);
            frame.render_widget(block, *cell);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1)])
                .split(inner);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {label}"),
                    Style::default().fg(MUTED),
                )),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {value}"),
                    Style::default()
                        .fg(TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )),
                rows[1],
            );
        }
        y += 4;
    }

    // Workspace-aware lower section — semantic states only.
    render_workspace_section(frame, content, y, model);
}

/// Draws the workspace columns and returns the next free `y`.
fn render_workspace_section(
    frame: &mut ratatui::Frame<'_>,
    content: Rect,
    y: u16,
    model: &TuiModel,
) -> u16 {
    let Some(workspace) = &model.workspace else {
        if y < content.y + content.height {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "Loading workspace…",
                    Style::default().fg(MUTED),
                )),
                Rect::new(content.x, y, content.width, 1),
            );
        }
        return y + 1;
    };

    // Which blocks exist: MARKETPLACE only for a pure marketplace
    // workspace; PROJECT everywhere else (a plain directory is a real
    // "not configured" state, never a blank screen).
    let show_project = workspace.kind != WorkspaceKind::Marketplace;
    let show_marketplace = workspace.marketplace.is_some();

    let (project_block, marketplace_block) = if show_project && show_marketplace {
        (
            Some(project_lines(model, workspace)),
            Some(marketplace_lines(workspace)),
        )
    } else if show_project {
        (Some(project_lines(model, workspace)), None)
    } else {
        (None, Some(marketplace_lines(workspace)))
    };

    // The workspace section reads top-to-bottom: PROJECT first, then
    // MARKETPLACE — two stacked rows, never columns. Rows keep the
    // capped width so the block stays compact on wide terminals.
    let block_width = WORKSPACE_BLOCK_MAX_WIDTH.min(content.width);
    let area = Rect::new(content.x, y, block_width, content.height);
    let mut cursor = y;
    if let Some(lines) = &project_block {
        cursor = render_block(frame, area, cursor, "PROJECT", lines);
        if cursor < content.y + content.height {
            cursor += 1; // one blank row between the two blocks
        }
    }
    if let Some(lines) = &marketplace_block {
        cursor = render_block(frame, area, cursor, "MARKETPLACE", lines);
    }
    cursor.saturating_add(1)
}

/// Renders `title` + `lines` starting at `y`; stops at the block bottom.
fn render_block(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    y: u16,
    title: &str,
    lines: &[Line<'static>],
) -> u16 {
    let mut cursor = y;
    if cursor < area.y + area.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                title,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Rect::new(area.x, cursor, area.width, 1),
        );
        cursor += 1;
    }
    for line in lines {
        if cursor >= area.y + area.height {
            break;
        }
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(area.x, cursor, area.width, 1),
        );
        cursor += 1;
    }
    cursor.saturating_add(1)
}

// --- Row construction ------------------------------------------------------

/// A key/value row: muted padded label + a styled value carrying the
/// semantic weight (`✓ ready`, `! 1/2 installed`, `— none`, `2 installed`).
fn value_row(label: &str, label_width: usize, value: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<label_width$}"),
            Style::default().fg(MUTED),
        ),
        Span::styled(format!(" {value}"), style),
    ])
}

fn blank() -> Line<'static> {
    Line::from("")
}

/// One full-width line (no label column) — status/action lines.
fn plain_line(indent: usize, text: &str, style: Style) -> Line<'static> {
    Line::from(Span::styled(format!("{}{text}", " ".repeat(indent)), style))
}

const OK: Style = Style::new().fg(SUCCESS);
const ATTENTION: Style = Style::new().fg(WARNING);
const ERROR: Style = Style::new().fg(DANGER);
const DIM: Style = Style::new().fg(TEXT_DIM);

const LABEL_PROJECT: usize = 12;
const LABEL_MARKET: usize = 12;

// --- PROJECT column --------------------------------------------------------

fn project_lines(model: &TuiModel, workspace: &OverviewWorkspaceSummary) -> Vec<Line<'static>> {
    let project = &workspace.project;
    let mut lines = Vec::new();

    // Environment: the Application's verdict on the declared environment.
    match project.environment {
        ProjectEnvironmentState::NotConfigured => {
            lines.push(value_row(
                "Environment",
                LABEL_PROJECT,
                "— not configured",
                DIM,
            ));
        }
        ProjectEnvironmentState::Invalid => {
            lines.push(value_row("Environment", LABEL_PROJECT, "× invalid", ERROR));
        }
        ProjectEnvironmentState::InstallRequired => {
            lines.push(value_row(
                "Environment",
                LABEL_PROJECT,
                "! install required",
                ATTENTION,
            ));
        }
        ProjectEnvironmentState::Ready => {
            lines.push(value_row("Environment", LABEL_PROJECT, "✓ ready", OK));
        }
    }

    // Memory: what the project's instructions baseline is.
    match project.memory {
        MemoryState::None => lines.push(value_row("Memory", LABEL_PROJECT, "— none", DIM)),
        MemoryState::Ready => {
            lines.push(value_row("Memory", LABEL_PROJECT, "✓ AGENTS.md", OK));
        }
        MemoryState::Issue => {
            lines.push(value_row("Memory", LABEL_PROJECT, "! issue", ATTENTION));
        }
    }

    // Plugins: a quantity, not a verdict — colored only when it diverges.
    match project.environment {
        ProjectEnvironmentState::NotConfigured => {
            lines.push(value_row("Plugins", LABEL_PROJECT, "— none", DIM));
        }
        ProjectEnvironmentState::Invalid => {
            lines.push(value_row("Plugins", LABEL_PROJECT, "— unknown", DIM));
        }
        ProjectEnvironmentState::InstallRequired => {
            lines.push(value_row(
                "Plugins",
                LABEL_PROJECT,
                &format!(
                    "! {}/{} installed",
                    project.installed_plugins, project.declared_plugins
                ),
                ATTENTION,
            ));
        }
        ProjectEnvironmentState::Ready => {
            if project.declared_plugins == 0 {
                lines.push(value_row("Plugins", LABEL_PROJECT, "— none", DIM));
            } else {
                lines.push(value_row(
                    "Plugins",
                    LABEL_PROJECT,
                    &format!("{} installed", project.installed_plugins),
                    TEXT_PRIMARY_STYLE,
                ));
            }
        }
    }

    // The one action this screen is allowed to offer.
    if model.overview_install_path().is_some() {
        lines.push(blank());
        lines.push(plain_line(
            2,
            "i install",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
    }
    lines
}

// --- MARKETPLACE column ----------------------------------------------------

fn marketplace_lines(workspace: &OverviewWorkspaceSummary) -> Vec<Line<'static>> {
    let market = workspace.marketplace.as_ref().expect("marketplace kind");
    let mut lines = Vec::new();

    match &market.name {
        Some(name) => lines.push(value_row("Name", LABEL_MARKET, name, TEXT_PRIMARY_STYLE)),
        None => lines.push(value_row("Name", LABEL_MARKET, "— unknown", DIM)),
    }

    match market.state {
        MarketplaceState::Valid => {
            let count = market.package_count;
            lines.push(value_row(
                "Plugins",
                LABEL_MARKET,
                &format!("{count} package{}", if count == 1 { "" } else { "s" }),
                TEXT_PRIMARY_STYLE,
            ));
        }
        MarketplaceState::InvalidManifest => {
            lines.push(value_row("Plugins", LABEL_MARKET, "— unknown", DIM));
        }
    }

    match market.state {
        MarketplaceState::Valid if market.invalid_packages == 0 => {
            lines.push(value_row("Status", LABEL_MARKET, "✓ valid", OK));
        }
        MarketplaceState::Valid => {
            let invalid = market.invalid_packages;
            lines.push(value_row(
                "Status",
                LABEL_MARKET,
                &format!("! {invalid} invalid"),
                ATTENTION,
            ));
        }
        MarketplaceState::InvalidManifest => {
            lines.push(value_row(
                "Status",
                LABEL_MARKET,
                "× invalid manifest",
                ERROR,
            ));
        }
    }
    lines
}

const TEXT_PRIMARY_STYLE: Style = Style::new().fg(TEXT_PRIMARY);

pub(crate) fn portability_label(portability: &Portability) -> &'static str {
    match portability {
        Portability::NoContext => "no context",
        Portability::Portable => "portable",
        Portability::PartiallyPortable { .. } => "partially portable",
        Portability::VendorLocked { .. } => "vendor-locked",
    }
}

pub(crate) fn portability_style(status: Option<&ProjectContextStatus>) -> Style {
    match status.map(|s| &s.portability) {
        Some(Portability::Portable) => Style::default().fg(SUCCESS),
        None | Some(Portability::NoContext) => Style::default().fg(MUTED),
        Some(_) => Style::default().fg(WARNING),
    }
}
