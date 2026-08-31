//! TUI view — Extensions route.
//!
//! The tool side of the product: official uze extensions that extend the
//! TUI/CLI itself (as opposed to plugins, which are agentic packages
//! delivered *to* harnesses — see `view::plugins`). Rows come straight
//! from `uze_extensions::registry::ExtensionRegistry::builtin`, the one
//! composition root that knows the extension set, so nothing here is
//! hand-maintained. Today every entry is bundled with the binary (there is
//! no loading/enablement surface yet); a flat two-line-per-row list
//! (name + surface on the first line, description on the second), and the
//! detail drawer opens on selection the same way Plugins/Harnesses do —
//! its content is static catalog metadata, so there is nothing to fetch.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::super::hit::Hit;
use super::super::model::{ResizablePanel, TuiModel};
use super::super::{
    ACCENT, BASE, BORDER, MUTED, SELECTED_BG, TEXT_BRIGHT, TEXT_PRIMARY, TEXT_SECONDARY,
};
use super::super::{content_area, render_screen_header};
use super::render_status_line;

pub(crate) fn render_extensions(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let content = content_area(area);
    let content = render_screen_header(
        frame,
        content,
        "Extensions",
        "official tool extensions",
        Some(Span::styled(
            format!("{} bundled", model.extensions.len()),
            Style::default().fg(MUTED),
        )),
    );

    if model.extensions.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "No extensions available.",
                Style::default().fg(MUTED),
            )),
            content,
        );
    } else {
        let mut y = content.y;
        for (index, extension) in model.extensions.iter().enumerate() {
            if y + 1 >= content.y + content.height {
                break;
            }
            let selected = index == model.extensions_selected;
            let rect = Rect::new(content.x, y, content.width, 2);
            render_extension_row(frame, rect, extension, selected, hits, index);
            // A blank row between blocks — otherwise one extension's
            // description sits directly against the next extension's name
            // with no breathing room at all.
            y += 3;
        }
    }

    if model.extension_drawer_open
        && let Some(extension) = model.selected_extension()
    {
        let drawer_area = area_for_drawer(area);
        let drawer_width = model
            .extension_drawer_width
            .unwrap_or(52)
            .clamp(24, drawer_area.width.saturating_sub(24).max(24));
        render_extension_drawer(frame, drawer_area, drawer_width, model, extension, hits);
    }
}

/// The drawer overlays from the *original* (unshrunk) route area, matching
/// how Plugins/Harnesses anchor theirs — `content_area` already insets
/// once for the list; re-deriving here keeps the drawer's own inset
/// independent of how much of that area the header consumed.
fn area_for_drawer(area: Rect) -> Rect {
    content_area(area)
}

fn render_extension_row(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    extension: &uze_extensions::registry::BuiltinExtension,
    selected: bool,
    hits: &mut Vec<(Rect, Hit)>,
    index: usize,
) {
    let name_fg = if selected {
        TEXT_BRIGHT
    } else {
        TEXT_SECONDARY
    };
    let left = vec![Span::styled(extension.name, Style::default().fg(name_fg))];
    let right = vec![Span::styled(
        format!("{} · bundled", extension.surface),
        Style::default().fg(ACCENT),
    )];
    let used: usize = left.iter().chain(right.iter()).map(Span::width).sum();
    let gap = (rect.width as usize).saturating_sub(used);
    let mut name_spans = left;
    name_spans.push(Span::raw(" ".repeat(gap.max(2))));
    name_spans.extend(right);

    let mut desc_spans = vec![Span::styled(
        extension.description,
        Style::default().fg(MUTED),
    )];

    if selected {
        for span in name_spans.iter_mut().chain(desc_spans.iter_mut()) {
            span.style = span.style.bg(SELECTED_BG);
        }
        for spans in [&mut name_spans, &mut desc_spans] {
            let used: usize = spans.iter().map(Span::width).sum();
            let gap = (rect.width as usize).saturating_sub(used);
            spans.push(Span::styled(
                " ".repeat(gap),
                Style::default().bg(SELECTED_BG),
            ));
        }
    }

    let top = Rect::new(rect.x, rect.y, rect.width, 1);
    let bottom = Rect::new(rect.x, rect.y + 1, rect.width, 1);
    frame.render_widget(Paragraph::new(Line::from(name_spans)), top);
    frame.render_widget(Paragraph::new(Line::from(desc_spans)), bottom);
    hits.push((rect, Hit::ExtensionRow(index)));
}

fn render_extension_drawer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    width: u16,
    model: &TuiModel,
    extension: &uze_extensions::registry::BuiltinExtension,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let width = width.min(area.width);
    let drawer = Rect::new(
        area.x + area.width - width,
        area.y - 1,
        width,
        area.height + 1,
    );
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::ExtensionDrawer) {
                    ACCENT
                } else {
                    BORDER
                },
            ))
            .style(Style::default().bg(BASE)),
        drawer,
    );
    hits.insert(
        0,
        (
            Rect::new(drawer.x, drawer.y, 1, drawer.height),
            Hit::ResizePanel(ResizablePanel::ExtensionDrawer),
        ),
    );
    // Same sectioning as the Plugins drawer: a body that scrolls/clips
    // naturally and a fixed 3-row status block beneath it, so the two can
    // never overlap regardless of terminal height.
    let sections_x = drawer.x + 2;
    let sections_width = drawer.width.saturating_sub(3);
    let status_height = 3;
    let body = Rect::new(
        sections_x,
        drawer.y + 1,
        sections_width,
        drawer.height.saturating_sub(2 + status_height),
    );
    let status = Rect::new(
        sections_x,
        body.y + body.height,
        sections_width,
        status_height,
    );

    let lines = vec![
        Line::from(Span::styled(
            "EXTENSION",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            extension.name,
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            extension.description,
            Style::default().fg(TEXT_SECONDARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "SURFACE",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            extension.surface,
            Style::default().fg(TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "HOW TO OPEN",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            extension.usage,
            Style::default().fg(TEXT_SECONDARY),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);

    render_status_line(
        frame,
        status,
        ACCENT,
        "Bundled",
        "Ships with uze — always available",
    );
}
