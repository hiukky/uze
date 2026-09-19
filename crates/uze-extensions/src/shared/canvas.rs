//! A grid of cells a diagram is painted onto, and the glyphs it is
//! painted with.
//!
//! A cell is the only unit every terminal agrees on: it survives SSH, a
//! multiplexer and a terminal with no graphics protocol, and it is what
//! the host already knows how to colour. Lines are kept as *which sides
//! of the cell they leave by* until the very end, so two lines meeting
//! become a junction rather than whichever was drawn last.

use unicode_width::UnicodeWidthChar;

use crate::view::{ContentLine, LineTone, Role, Span};

/// How a line is drawn. A property of the line rather than of whatever
/// the line stands for, which is why it lives beside the drawing and not
/// beside the model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Stroke {
    #[default]
    Solid,
    Dotted,
    Thick,
}

pub const NORTH: u8 = 1;
pub const EAST: u8 = 2;
pub const SOUTH: u8 = 4;
pub const WEST: u8 = 8;

/// The second half of a double-width glyph: occupied, and never emitted.
const CONTINUATION: char = '\0';

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Glyphs {
    #[default]
    Unicode,
    Ascii,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Corners {
    Square,
    Rounded,
    Marked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub role: Role,
    pub bold: bool,
    /// Written to, even if with a space: the inside of a box and the gap
    /// between two words are the drawing's, not the board showing through.
    pub solid: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            role: Role::Default,
            bold: false,
            solid: false,
        }
    }
}

pub struct Canvas {
    pub width: i32,
    pub height: i32,
    pub glyphs: Glyphs,
    cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(width: i32, height: i32, glyphs: Glyphs) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            glyphs,
            cells: vec![Cell::default(); (width * height) as usize],
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width && y < self.height)
            .then(|| (y * self.width + x) as usize)
    }

    pub fn put(&mut self, x: i32, y: i32, glyph: char, role: Role, bold: bool) {
        if let Some(index) = self.index(x, y) {
            self.cells[index] = Cell {
                glyph,
                role,
                bold,
                solid: true,
            };
        }
    }

    pub fn is_blank(&self, x: i32, y: i32) -> bool {
        self.index(x, y)
            .is_some_and(|index| !self.cells[index].solid)
    }

    /// Writes `text` from `x`, one cell per column it occupies, and
    /// answers the column after it.
    pub fn text(&mut self, x: i32, y: i32, text: &str, role: Role, bold: bool) -> i32 {
        let mut column = x;
        for glyph in text.chars() {
            let width = glyph.width().unwrap_or(0) as i32;
            if width == 0 {
                continue;
            }
            self.put(column, y, glyph, role, bold);
            if width == 2 {
                self.put(column + 1, y, CONTINUATION, role, bold);
            }
            column += width;
        }
        column
    }

    pub fn line(&mut self, x: i32, y: i32, sides: u8, stroke: Stroke, role: Role) {
        let glyph = line_glyph(sides, stroke, self.glyphs, Corners::Rounded);
        self.put(x, y, glyph, role, false);
    }

    pub fn frame(&mut self, frame: Frame, corners: Corners, role: Role) {
        let Frame { x, y, w, h } = frame;
        let (right, bottom) = (x + w - 1, y + h - 1);
        for column in x + 1..right {
            for row in [y, bottom] {
                let glyph = line_glyph(EAST | WEST, Stroke::Solid, self.glyphs, corners);
                self.put(column, row, glyph, role, false);
            }
        }
        for row in y + 1..bottom {
            for column in [x, right] {
                let glyph = line_glyph(NORTH | SOUTH, Stroke::Solid, self.glyphs, corners);
                self.put(column, row, glyph, role, false);
            }
        }
        for (column, row, sides) in [
            (x, y, EAST | SOUTH),
            (right, y, SOUTH | WEST),
            (x, bottom, NORTH | EAST),
            (right, bottom, NORTH | WEST),
        ] {
            let glyph = line_glyph(sides, Stroke::Solid, self.glyphs, corners);
            self.put(column, row, glyph, role, false);
        }
    }

    pub fn cell(&self, x: i32, y: i32) -> Option<Cell> {
        self.index(x, y).map(|index| self.cells[index])
    }

    pub fn set(&mut self, x: i32, y: i32, cell: Cell) {
        if let Some(index) = self.index(x, y) {
            self.cells[index] = cell;
        }
    }

    /// Every row, as lines the host can draw as they are.
    pub fn lines(&self) -> Vec<ContentLine> {
        (0..self.height)
            .map(|row| ContentLine {
                gutter: String::new(),
                number: String::new(),
                tone: LineTone::Neutral,
                spans: self.spans(row, 0, self.width),
            })
            .collect()
    }

    fn spans(&self, row: i32, pan: i32, columns: i32) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for column in pan.max(0)..(pan + columns).min(self.width) {
            let Some(index) = self.index(column, row) else {
                continue;
            };
            let cell = self.cells[index];
            if cell.glyph == CONTINUATION {
                continue;
            }
            match spans.last_mut() {
                Some(last) if last.role == cell.role && last.bold == cell.bold => {
                    last.text.push(cell.glyph);
                }
                _ => {
                    let mut span = Span::new(cell.glyph.to_string(), cell.role);
                    span.bold = cell.bold;
                    spans.push(span);
                }
            }
        }
        if let Some(last) = spans.last_mut() {
            let trimmed = last.text.trim_end().len();
            last.text.truncate(trimmed);
        }
        spans
    }

    #[cfg(test)]
    pub fn to_text(&self) -> String {
        (0..self.height)
            .map(|row| {
                self.spans(row, 0, self.width)
                    .into_iter()
                    .map(|span| span.text)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Frame {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// What a box leads to, written into its border: further into the
/// drawing, or out of it to the code it stands for.
pub fn leads_glyph(out_of_the_board: bool, glyphs: Glyphs) -> &'static str {
    match (glyphs, out_of_the_board) {
        (Glyphs::Unicode, false) => "»",
        (Glyphs::Unicode, true) => "↗",
        (Glyphs::Ascii, false) => ">>",
        (Glyphs::Ascii, true) => "^",
    }
}

/// The mark on something that differs from what was last committed.
pub fn changed_glyph(glyphs: Glyphs) -> char {
    match glyphs {
        Glyphs::Unicode => '●',
        Glyphs::Ascii => '*',
    }
}

/// `text` cut to `width` columns, saying so when it was cut.
pub fn fitted(text: &str, width: i32, glyphs: Glyphs) -> String {
    if text_width(text) <= width {
        return text.to_owned();
    }
    let cut = match glyphs {
        Glyphs::Unicode => '…',
        Glyphs::Ascii => '~',
    };
    let mut fitted = String::new();
    let mut used = 1;
    for glyph in text.chars() {
        used += glyph.width().unwrap_or(0) as i32;
        if used > width {
            break;
        }
        fitted.push(glyph);
    }
    if width >= 1 {
        fitted.push(cut);
    }
    fitted
}

pub fn arrow_glyph(heading: u8, glyphs: Glyphs) -> char {
    match (glyphs, heading) {
        (Glyphs::Unicode, NORTH) => '▲',
        (Glyphs::Unicode, EAST) => '►',
        (Glyphs::Unicode, SOUTH) => '▼',
        (Glyphs::Unicode, _) => '◄',
        (Glyphs::Ascii, NORTH) => '^',
        (Glyphs::Ascii, EAST) => '>',
        (Glyphs::Ascii, SOUTH) => 'v',
        (Glyphs::Ascii, _) => '<',
    }
}

/// The glyph for a line leaving a cell by `sides`. Only a straight run
/// carries its stroke: box-drawing has no dotted corner, and a junction
/// drawn half-heavy reads as a rendering fault rather than as emphasis.
pub fn line_glyph(sides: u8, stroke: Stroke, glyphs: Glyphs, corners: Corners) -> char {
    let vertical = sides & (EAST | WEST) == 0;
    let horizontal = sides & (NORTH | SOUTH) == 0;
    if sides == 0 {
        return ' ';
    }
    if glyphs == Glyphs::Ascii {
        return match (vertical, horizontal, stroke) {
            (true, _, Stroke::Dotted) => ':',
            (true, _, _) => '|',
            (_, true, Stroke::Dotted) => '.',
            (_, true, Stroke::Thick) => '=',
            (_, true, Stroke::Solid) => '-',
            _ if corners == Corners::Marked => '*',
            _ => '+',
        };
    }
    if vertical {
        return match stroke {
            Stroke::Solid => '│',
            Stroke::Dotted => '┆',
            Stroke::Thick => '┃',
        };
    }
    if horizontal {
        return match stroke {
            Stroke::Solid => '─',
            Stroke::Dotted => '┄',
            Stroke::Thick => '━',
        };
    }
    let corner = |square: char, rounded: char| match corners {
        Corners::Square => square,
        Corners::Rounded => rounded,
        Corners::Marked => '◇',
    };
    match sides {
        s if s == NORTH | EAST => corner('└', '╰'),
        s if s == EAST | SOUTH => corner('┌', '╭'),
        s if s == SOUTH | WEST => corner('┐', '╮'),
        s if s == NORTH | WEST => corner('┘', '╯'),
        s if s == NORTH | EAST | SOUTH => '├',
        s if s == NORTH | SOUTH | WEST => '┤',
        s if s == EAST | SOUTH | WEST => '┬',
        s if s == NORTH | EAST | WEST => '┴',
        _ => '┼',
    }
}

pub fn text_width(text: &str) -> i32 {
    unicode_width::UnicodeWidthStr::width(text) as i32
}

/// Greedy word wrap to `width` columns; a word longer than that keeps a
/// line to itself rather than being cut.
pub fn wrapped(text: &str, width: i32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if text_width(line) + 1 + text_width(word) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines
}
