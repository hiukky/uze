//! TUI view — Appearance route.
//!
//! What UZE looks like, as two choices rather than one: the palette, and —
//! chosen apart from it — the set of glyphs every mark is drawn from. A
//! font is installed once and a palette is picked on a whim, so welding
//! them together meant an operator with a patched font had to give up every
//! theme, or copy forty-four glyphs into their overrides.
//!
//! Shaped like Keys — a grouped list with a detail side — for the reason
//! Keys is shaped like Plugins: a screen someone visits rarely is better
//! off looking like one they already know.
//!
//! The one thing here that is its own: **every glyph set is drawn in its own
//! glyphs.** No terminal can be asked which font it is rendering with — no
//! escape sequence answers it, and a cursor-position probe measures width
//! rather than presence, so a missing glyph and a present one both come back
//! as one cell. Detection is therefore not a thing UZE can do honestly, and
//! a question at setup would ask the operator to recall from memory a fact
//! only the screen can settle. So the screen shows the marks, and the
//! operator decides by looking at them: a set this terminal cannot draw is
//! visible as tofu, and a set whose declared widths are wrong is visible as
//! a ragged preview column, both before anything is chosen.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::super::hit::Hit;
use super::super::model::{AppearanceRow, ResizablePanel, TuiModel};
use super::super::{content_area, render_screen_header, side_panel_area, small_caps};
use crate::ui::theme::{self, Symbol, Token};

/// The marks a preview shows. Chosen to be the ones that differ most
/// between sets, and to include a two-cell glyph (`arrow.to` is `->` in
/// ASCII), so a row that does not line up says the widths are wrong.
const PREVIEWED: &[Symbol] = &[
    Symbol::MarkOk,
    Symbol::MarkOfficial,
    Symbol::MarkNative,
    Symbol::MarkAttention,
    Symbol::StatusSelected,
    Symbol::StatusIdle,
    Symbol::ChevronCollapsed,
    Symbol::ArrowTo,
    Symbol::Prompt,
];

pub(crate) fn render_appearance(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let area = content_area(area);
    // What is in force, both halves, on the title's own row: the screen's
    // whole claim is that these are two choices, and a reader should not
    // have to find two ticks in two lists to know what they are on.
    let in_force = |rows: &[String]| -> String {
        rows.first()
            .cloned()
            .unwrap_or_else(|| "default".to_owned())
    };
    let theme = in_force(
        &model
            .appearance_themes
            .iter()
            .filter(|entry| entry.active)
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
    );
    let glyphs = in_force(
        &model
            .appearance_glyph_sets
            .iter()
            .filter(|entry| entry.active)
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
    );
    let content = render_screen_header(
        frame,
        area,
        "Appearance",
        "theme & glyphs",
        Some(Span::styled(
            format!(
                "{theme} {} {glyphs}",
                theme::glyph(Symbol::HintSeparator).trim()
            ),
            theme::fg(Token::TextMuted),
        )),
    );

    let drawer_width = model
        .appearance_drawer_width
        .unwrap_or(super::DRAWER_DEFAULT_WIDTH);
    let list_width = content.width.saturating_sub(drawer_width);
    let list_area = Rect::new(content.x, content.y, list_width, content.height);

    render_list(frame, list_area, model, hits);
    render_drawer(frame, content, drawer_width, model, hits);
}

fn render_list(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let rows = model.appearance_rows();
    // The id column is reserved for the whole list or for none of it, so
    // every preview starts at the same column and a misaligned glyph is
    // the set's fault rather than the layout's.
    let id_width = rows
        .iter()
        .map(|row| match row {
            AppearanceRow::Heading(_) => 0,
            AppearanceRow::Theme { id, .. } | AppearanceRow::GlyphSet { id, .. } => {
                id.chars().count()
            }
        })
        .max()
        .unwrap_or(0)
        .max(8);

    for (index, row) in rows.iter().enumerate() {
        let y = area.y + index as u16;
        if y >= area.bottom() {
            return;
        }
        let rect = Rect::new(area.x, y, area.width, 1);
        let selected = index == model.appearance_selected;

        match row {
            AppearanceRow::Heading(title) => {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        small_caps(title),
                        theme::fg(Token::TextDim).add_modifier(Modifier::BOLD),
                    )),
                    rect,
                );
            }
            AppearanceRow::Theme { id, active, path } => {
                let source = match path {
                    Some(path) => path.display().to_string(),
                    None => "built in".to_owned(),
                };
                render_choice(frame, rect, id, id_width, *active, selected, &source, None);
                hits.push((rect, Hit::AppearanceRow(index)));
            }
            AppearanceRow::GlyphSet { id, active } => {
                // Only as many marks as the column actually has room for.
                // A preview clipped by the drawer beside it says nothing
                // about the set and everything about the layout.
                let spent = 1 + usize::from(theme::width(Symbol::MarkOk)) + 1 + id_width + 2;
                let room = usize::from(rect.width).saturating_sub(spent);
                render_choice(
                    frame,
                    rect,
                    id,
                    id_width,
                    *active,
                    selected,
                    "",
                    Some(preview_spans(id, room)),
                );
                hits.push((rect, Hit::AppearanceRow(index)));
            }
        }
    }
}

/// One selectable line: the mark saying whether it is in force, the id, and
/// whatever that kind of choice shows beside itself.
#[allow(clippy::too_many_arguments)]
fn render_choice(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    id: &str,
    id_width: usize,
    active: bool,
    selected: bool,
    trailing: &str,
    preview: Option<Vec<Span<'static>>>,
) {
    if selected {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::color(Token::SurfaceSelected))),
            rect,
        );
    }
    let mark = if active {
        Span::styled(theme::glyph(Symbol::MarkOk), theme::fg(Token::Accent))
    } else {
        Span::raw(" ".repeat(usize::from(theme::width(Symbol::MarkOk))))
    };
    let mut spans = vec![
        Span::raw(" "),
        mark,
        Span::raw(" "),
        Span::styled(
            format!("{id:id_width$}  "),
            if active {
                theme::fg(Token::TextBright).add_modifier(Modifier::BOLD)
            } else {
                theme::fg(Token::TextPrimary)
            },
        ),
    ];
    match preview {
        Some(preview) => spans.extend(preview),
        None => spans.push(Span::styled(
            trailing.to_owned(),
            theme::fg(Token::TextMuted),
        )),
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), rect);
}

/// A set's own marks, resolved from that set rather than from the active
/// theme.
///
/// The only place in UZE that deliberately draws a glyph it is not
/// currently drawing with — and it has to, because the question the screen
/// answers is "can this terminal render *that*", which no amount of
/// resolving the active theme can reach.
fn preview_spans(id: &str, room: usize) -> Vec<Span<'static>> {
    let mut layers = vec![uze_theme::default_file()];
    layers.extend(uze_theme::glyph_set_file(id));
    let Ok(resolved) = uze_theme::resolve_stack(
        &uze_theme::Identity::from_file(id, uze_theme::default_file()),
        &layers,
    ) else {
        return Vec::new();
    };
    // Padded to each glyph's *declared* width rather than to its measured
    // one: that is what makes a set whose widths are wrong show up here as
    // a ragged column instead of shearing a row somewhere else later.
    let mut spans = Vec::new();
    let mut spent = 0;
    for symbol in PREVIEWED {
        let definition = resolved.theme.symbol(*symbol);
        let width = usize::from(definition.width()).max(1);
        if spent + width + 1 > room {
            break;
        }
        spent += width + 1;
        spans.push(Span::styled(
            format!("{:<width$} ", definition.glyph()),
            theme::fg(Token::TextPrimary),
        ));
    }
    spans
}

/// The detail side, drawn the way Keys and Plugins draw theirs: a
/// recessed slab behind a left rule, a drag handle on the rule, and the
/// content in labelled blocks. A screen someone visits rarely is better
/// off looking like one they already know.
fn render_drawer(
    frame: &mut ratatui::Frame<'_>,
    content: Rect,
    width: u16,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let drawer = side_panel_area(content, width);
    if drawer.width < 8 {
        return;
    }
    frame.render_widget(Clear, drawer);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(
                if model.dragging_panel == Some(ResizablePanel::AppearanceDrawer) {
                    theme::color(Token::Accent)
                } else {
                    theme::color(Token::SurfaceRecessed)
                },
            ))
            .style(theme::bg(Token::SurfaceRecessed)),
        drawer,
    );
    // First, so the rule answers the pointer before the rows behind it do.
    hits.insert(
        0,
        (
            Rect::new(drawer.x, drawer.y, 1, drawer.height),
            Hit::ResizePanel(ResizablePanel::AppearanceDrawer),
        ),
    );

    let inner = Rect::new(
        drawer.x + 2,
        drawer.y + 1,
        drawer.width.saturating_sub(3),
        drawer.height.saturating_sub(2),
    );
    let block = |label: &str| {
        Line::from(Span::styled(
            small_caps(label),
            theme::fg_bold(Token::TextMuted),
        ))
    };
    let title = |text: String| {
        Line::from(Span::styled(
            text,
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ))
    };
    let prose = |text: &str| {
        Line::from(Span::styled(
            text.to_owned(),
            theme::fg(Token::TextSecondary),
        ))
    };

    let lines = match model.selected_appearance_row() {
        Some(AppearanceRow::Theme { id, active, path }) => vec![
            block("Theme"),
            title(id),
            prose(match &path {
                Some(_) => "Yours.",
                None => "A theme UZE carries.",
            }),
            Line::from(""),
            block("Where"),
            prose(&match path {
                Some(path) => path.display().to_string(),
                None => "built in — no file to edit".to_owned(),
            }),
            Line::from(""),
            block("What it decides"),
            prose(
                "Colours. Whichever glyphs you chose stay chosen, unless \
                 this theme deliberately claims a mark of its own — then it \
                 decides that one.",
            ),
            Line::from(""),
            block("In force"),
            prose(if active {
                "Yes — this is what UZE draws in."
            } else {
                "No. Enter to draw in it."
            }),
        ],
        Some(AppearanceRow::GlyphSet { id, active }) => vec![
            block("Glyphs"),
            title(id.clone()),
            prose(glyph_set_note(&id)),
            Line::from(""),
            block("What it decides"),
            prose("Marks only — no colour moves, and no theme is disturbed."),
            Line::from(""),
            block("Needs"),
            prose(glyph_set_requirement(&id)),
            Line::from(""),
            block("In force"),
            prose(if active {
                "Yes — every mark on screen comes from this set."
            } else {
                "No. Enter to draw with it."
            }),
        ],
        _ => vec![
            block("Appearance"),
            Line::from(""),
            prose(
                "Two choices, and neither changes the other: the palette, and \
             the set every mark is drawn from.",
            ),
        ],
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// What a set asks of the machine, which is the question the preview
/// beside it cannot answer on its own.
fn glyph_set_requirement(id: &str) -> &'static str {
    match id {
        "ascii" => "Nothing at all. Every mark is inside ASCII.",
        "nerd" => {
            "A font patched by Nerd Fonts v3 or newer — the Mono build, \
             whose icons take one cell. If the preview is empty boxes, \
             this terminal has neither."
        }
        _ => "A terminal font with ordinary Unicode coverage. No install.",
    }
}

/// What each set is for, said where someone is deciding between them.
fn glyph_set_note(id: &str) -> &'static str {
    match id {
        "ascii" => {
            "Every mark inside ASCII. For a terminal with no Unicode font — \
             and it costs no palette, because a set carries no colours."
        }
        "nerd" => {
            "Codicons, the icons VS Code draws its own chrome with. Needs a \
             font patched by Nerd Fonts v3, and its widths are declared for \
             the Mono builds. If the preview beside it is empty boxes, this \
             terminal's font does not have them."
        }
        _ => {
            "What UZE ships with: Unicode any modern terminal font draws, \
             and no private-use glyphs."
        }
    }
}
