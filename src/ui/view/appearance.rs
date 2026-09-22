//! TUI view — Appearance route.
//!
//! What UZE looks like, as two choices rather than one: the palette, and —
//! chosen apart from it — the set of glyphs every mark is drawn from. A
//! font is installed once and a palette is picked on a whim, so welding
//! them together meant an operator with a patched font had to give up every
//! theme, or copy forty-four glyphs into their overrides.
//!
//! Shaped like Keys — grouped choices with a detail side — for the reason
//! Keys is shaped like Plugins: a screen someone visits rarely is better
//! off looking like one they already know. The choices themselves are cards
//! rather than rows, because what a theme or a set *is* cannot be said in
//! words: it has to be shown, and a row has one line to show it in.
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
    widgets::{Paragraph, Wrap},
};

use super::super::hit::Hit;
use super::super::model::{AppearanceRow, ResizablePanel, Route, TuiModel};
use super::super::{content_area, render_screen_header};
use crate::ui::theme::{self, Symbol, Token};
use crate::ui::widget::{self, Scrollbar, Surface};

/// The marks a preview shows. Chosen to be the ones that differ most
/// between sets, and to include a two-cell glyph (`arrow.to` is `->` in
/// ASCII), so a row that does not line up says the widths are wrong.
const PREVIEWED: &[Symbol] = &[
    Symbol::MarkOk,
    Symbol::MarkOfficial,
    Symbol::MarkAdapted,
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
    let content = render_screen_header(frame, area, Route::Appearance, None);

    let drawer_width = super::drawer_width(ResizablePanel::AppearanceDrawer, model, area);
    let list_width = content.width.saturating_sub(drawer_width);
    let list_area = Rect::new(content.x, content.y, list_width, content.height);

    render_catalog(frame, list_area, model, hits);
    // The whole content area, not what the header left: a drawer runs the
    // frame's full height on every other screen, and one that starts below
    // the title reads as a panel that failed to open.
    render_drawer(frame, area, model, hits);
}

/// A card in the catalogue. Wide enough for a glyph set's whole preview
/// row, short enough that the palette and every set fit on an 80x24
/// terminal at once — which is the point of a catalogue over a list.
const CARD_WIDTH: u16 = 30;
const CARD_HEIGHT: u16 = 4;
const CARD_GAP: u16 = 1;

/// The choices, as a grid of cards under their group headings.
///
/// Selection stays linear — reading order, left to right and down — so
/// `up`/`down` mean what they did when this was a list, and a heading is
/// crossed rather than landed on.
///
/// Laid out onto a canvas first and windowed onto the screen after, the
/// way the keys screen windows its list. Drawing straight down the area
/// and stopping at its foot was enough while the cards fit: narrow the
/// column and they wrap onto more lines than there are rows, and
/// everything past the last one was invisible and unreachable at once —
/// the selection walked off the bottom and nothing followed it.
///
/// The window moves in whole bands — a heading, a blank, or a line of
/// cards — rather than in rows. A card is four rows tall, and a window
/// free to stop between them would either clip one or drop it and leave
/// the gap where it stood.
fn render_catalog(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let bands = bands_of(model, area.width);
    let total: u16 = bands.iter().map(Band::height).sum();
    // A column for the track, and only when there is more catalogue than
    // screen: a scrollbar on a page that fits says the opposite of what
    // it is for. Taken before the cards are placed, so the width they are
    // laid out against is the width they are drawn in.
    let track_width = Scrollbar::width().max(1);
    let track = (total > area.height).then(|| {
        Rect::new(
            area.right().saturating_sub(track_width),
            area.y,
            track_width,
            area.height,
        )
    });
    let area = match track {
        Some(_) => Rect::new(
            area.x,
            area.y,
            area.width.saturating_sub(track_width + 1),
            area.height,
        ),
        None => area,
    };
    let bands = bands_of(model, area.width);
    let total: u16 = bands.iter().map(Band::height).sum();

    // The window stays at the top until the selection passes the middle,
    // so reading down from the first card does not move the page.
    let first = first_band(&bands, model.appearance_selected, area.height);
    let mut y = area.y;
    let skipped: u16 = bands.iter().take(first).map(Band::height).sum();
    for band in bands.iter().skip(first) {
        if y + band.height() > area.bottom() {
            break;
        }
        match band {
            Band::Gap => {}
            Band::Heading(title) => frame.render_widget(
                Paragraph::new(Span::styled(
                    title.to_uppercase(),
                    theme::fg(Token::TextDim).add_modifier(Modifier::BOLD),
                )),
                Rect::new(area.x, y, area.width, 1),
            ),
            Band::Cards(cards) => {
                for (column, index) in cards.iter().enumerate() {
                    let x = area.x + column as u16 * (CARD_WIDTH + CARD_GAP);
                    let width = CARD_WIDTH.min(area.right().saturating_sub(x));
                    let rect = Rect::new(x, y, width, CARD_HEIGHT);
                    render_row(frame, rect, model, *index);
                    hits.push((rect, Hit::AppearanceRow(*index)));
                }
            }
        }
        y += band.height();
    }
    if let Some(track) = track
        && let Some(bar) = Scrollbar::measure(track, usize::from(area.height), usize::from(total))
    {
        bar.render(frame, usize::from(skipped));
    }
}

/// One stretch of the catalogue that moves as a unit: a group heading,
/// the blank row between two groups, or one line of cards.
enum Band<'a> {
    Heading(&'a str),
    Gap,
    Cards(Vec<usize>),
}

impl Band<'_> {
    fn height(&self) -> u16 {
        match self {
            Self::Heading(_) => 1,
            Self::Gap => 1,
            Self::Cards(_) => CARD_HEIGHT,
        }
    }
}

/// The catalogue laid out for a column `width` wide, in reading order.
fn bands_of(model: &TuiModel, width: u16) -> Vec<Band<'_>> {
    let columns = usize::from(((width + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).max(1));
    let mut bands = Vec::new();
    let mut line: Vec<usize> = Vec::new();
    for (index, row) in model.appearance_rows().iter().enumerate() {
        if let AppearanceRow::Heading(title) = row {
            if !line.is_empty() {
                bands.push(Band::Cards(std::mem::take(&mut line)));
            }
            if !bands.is_empty() {
                bands.push(Band::Gap);
            }
            bands.push(Band::Heading(title));
            continue;
        }
        line.push(index);
        if line.len() == columns {
            bands.push(Band::Cards(std::mem::take(&mut line)));
        }
    }
    if !line.is_empty() {
        bands.push(Band::Cards(line));
    }
    bands
}

/// The first band to draw so the `selected` card is on screen, held so the
/// last page ends on the last band rather than on empty rows.
fn first_band(bands: &[Band<'_>], selected: usize, height: u16) -> usize {
    let anchor = bands
        .iter()
        .position(|band| matches!(band, Band::Cards(cards) if cards.contains(&selected)))
        .unwrap_or(0);
    // Back off from the anchor until half the screen is filled behind it,
    // then forward again while the tail still fits: two walks over a list
    // of bands, because their heights differ and no arithmetic on indices
    // answers where a variable-height window starts.
    let fits_from = |first: usize| -> bool {
        let mut used = 0;
        for band in bands.iter().skip(first) {
            used += band.height();
        }
        used <= height
    };
    let mut first = anchor;
    let mut behind = 0;
    while first > 0 {
        let candidate = first - 1;
        behind += bands[candidate].height();
        if behind > height / 2 {
            break;
        }
        first = candidate;
    }
    while first > 0 && fits_from(first - 1) {
        first -= 1;
    }
    first
}

/// One card of the catalogue, by its index into `appearance_rows`: which
/// kind it is decides what is drawn under the name and what the preview
/// line shows.
fn render_row(frame: &mut ratatui::Frame<'_>, rect: Rect, model: &TuiModel, index: usize) {
    let rows = model.appearance_rows();
    let Some(row) = rows.get(index) else {
        return;
    };
    let selected = index == model.appearance_selected;
    match row {
        AppearanceRow::Theme { id, active, path } => {
            let source = match path {
                // The file's own name, not its path: a card has room for
                // one, and the drawer beside it carries the other.
                Some(path) => path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ),
                None => "built in".to_owned(),
            };
            render_card(
                frame,
                rect,
                id,
                *active,
                selected,
                &source,
                swatch_spans(model, id),
            );
        }
        AppearanceRow::GlyphSet { id, active } => {
            let room = usize::from(rect.width).saturating_sub(4);
            render_card(
                frame,
                rect,
                id,
                *active,
                selected,
                glyph_set_tagline(id),
                preview_spans(id, room),
            );
        }
        // Headings are bands of their own and never reach a card rect.
        AppearanceRow::Heading(_) => {}
    }
}

/// One choice: its name in the frame, what it is beneath, and — the whole
/// One choice: its name in the frame, what it is beneath, and — the whole
/// reason this is a card — what it actually looks like on the last line.
///
/// The two states are drawn by two different means on purpose. Which card
/// the keyboard is on is the *frame*; which one is in force is the *fill*
/// inside it, so the answer to "what am I about to choose" and the answer to
/// "what is on right now" never compete for the same cells.
fn render_card(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    id: &str,
    active: bool,
    selected: bool,
    note: &str,
    body: Vec<Span<'static>>,
) {
    let mut title = vec![Span::raw(" ")];
    if active {
        title.push(Span::styled(
            theme::glyph(Symbol::MarkOk),
            theme::fg(Token::Accent),
        ));
        title.push(Span::raw(" "));
    }
    title.push(Span::styled(
        id.to_owned(),
        if active {
            theme::fg_bold(Token::TextBright)
        } else {
            theme::fg_bold(Token::TextPrimary)
        },
    ));
    title.push(Span::raw(" "));

    let inner = Surface::selectable(selected)
        .title(Line::from(title))
        .render(frame, rect);
    // Inside the frame, never over it: a fill that covered the border cells
    // too would paint out the one thing that says where the keyboard is,
    // on every card that is in force but not selected.
    if active {
        widget::fill(frame, inner, Token::SurfaceSelected);
    }

    let text = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(note.to_owned(), theme::fg(Token::TextMuted))),
            Line::from(body),
        ]),
        text,
    );
}

/// A theme's own colours, drawn in themselves.
///
/// Empty when the theme did not resolve, which is the honest answer: a file
/// with a typo in it has no palette to show, and inventing one here would
/// hide exactly the mistake the author needs to see.
fn swatch_spans(model: &TuiModel, id: &str) -> Vec<Span<'static>> {
    let Some(colours) = model.appearance_palettes.get(id) else {
        return Vec::new();
    };
    colours
        .iter()
        .map(|rgb| Span::styled("███ ", Style::default().fg(theme::swatch(*rgb))))
        .collect()
}

/// What a set asks of the machine, in the words a card has room for. The
/// sentence is in the drawer.
fn glyph_set_tagline(id: &str) -> &'static str {
    match id {
        "ascii" => "ASCII — any font at all",
        "nerd" => "needs a Nerd Font v3",
        _ => "Unicode — any modern font",
    }
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
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let inner = super::drawer(
        frame,
        content,
        ResizablePanel::AppearanceDrawer,
        model,
        hits,
    );
    let block = |label: &str| {
        Line::from(Span::styled(
            label.to_uppercase(),
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
