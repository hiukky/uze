//! Text selected in a pane with the pointer, and the clipboard it goes to.
//!
//! The client owns the mouse (it has to, for its own chrome), which takes
//! the host terminal's native selection away from the panes it draws. This
//! gives it back the way every terminal does it: press, drag, release — and
//! the release copies, since a selection nobody asked to copy is the one
//! gesture a pane's reader never wanted.

use ratatui::layout::Rect;
use ratatui::text::Span;
use uze_terminal::{PaneId, PaneSnapshot};

/// A press in a pane and where the pointer has carried it since, in the
/// pane's own 0-indexed cells. Ordered the way it was drawn, not the way it
/// reads: `anchor` is where the press landed and may come after `head`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PaneSelection {
    pub(super) pane: PaneId,
    anchor: (u16, u16),
    head: (u16, u16),
    /// Whether the pointer has left the cell it was pressed in. A press
    /// that never moved is a click, and a click selects nothing.
    moved: bool,
}

impl PaneSelection {
    pub(super) fn pressed(pane: PaneId, area: Rect, column: u16, row: u16) -> Self {
        let at = cell_in(area, column, row);
        Self {
            pane,
            anchor: at,
            head: at,
            moved: false,
        }
    }

    /// Follows the pointer, clamped to the pane: a drag that overshoots the
    /// edge still means "to the edge", which is how a selection reaches the
    /// last column without the pointer landing exactly on it.
    pub(super) fn follow(&mut self, area: Rect, column: u16, row: u16) -> bool {
        let head = cell_in(area, column, row);
        let changed = head != self.head;
        self.head = head;
        self.moved |= head != self.anchor;
        changed
    }

    pub(super) fn is_visible(&self) -> bool {
        self.moved
    }

    /// Whether the cell is inside the selection, read as text reads: whole
    /// rows between the first and last, partial rows at either end.
    pub(super) fn contains(&self, column: u16, row: u16) -> bool {
        let (start, end) = self.ordered();
        self.moved && (row, column) >= (start.1, start.0) && (row, column) <= (end.1, end.0)
    }

    /// The selected cells as text: trailing blanks dropped from each row,
    /// since a terminal pads every line to its width and nobody selected
    /// the padding, and the cell a wide character spills into skipped,
    /// since it holds a blank that is not in the text.
    pub(super) fn text(&self, snapshot: &PaneSnapshot) -> String {
        if !self.moved {
            return String::new();
        }
        let (start, end) = self.ordered();
        let columns = snapshot.columns;
        let last_column = columns.saturating_sub(1);
        let mut lines = Vec::new();
        for row in start.1..=end.1.min(snapshot.rows.saturating_sub(1)) {
            let from = if row == start.1 { start.0 } else { 0 };
            let to = if row == end.1 { end.0 } else { last_column };
            let mut line = String::new();
            let mut column = from;
            while column <= to.min(last_column) {
                let index = usize::from(row) * usize::from(columns) + usize::from(column);
                let Some(cell) = snapshot.cells.get(index) else {
                    break;
                };
                line.push(cell.character);
                column += Span::raw(cell.character.to_string()).width().max(1) as u16;
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }

    fn ordered(&self) -> ((u16, u16), (u16, u16)) {
        let reads_first = |(column, row): (u16, u16)| (row, column);
        if reads_first(self.anchor) <= reads_first(self.head) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

fn cell_in(area: Rect, column: u16, row: u16) -> (u16, u16) {
    let column = column.clamp(area.x, area.right().saturating_sub(1)) - area.x;
    let row = row.clamp(area.y, area.bottom().saturating_sub(1)) - area.y;
    (column, row)
}

/// The OSC 52 sequence that sets the system clipboard to `text` through the
/// host terminal. The terminal is the only thing that can reach the
/// clipboard of the machine the operator sits at — over SSH, from WSL into
/// Windows — so writing it there rather than calling a platform tool is
/// what makes the copy land where the reader will paste it.
pub(super) fn osc52(text: &str) -> Vec<u8> {
    let mut sequence = b"\x1b]52;c;".to_vec();
    sequence.extend(base64(text.as_bytes()).into_bytes());
    sequence.extend(b"\x07");
    sequence
}

fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let triple = u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]]);
        for (position, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if position <= chunk.len() {
                encoded.push(ALPHABET[(triple >> shift & 0x3f) as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use uze_terminal::{CellAttributes, Cursor, MouseMode, RenderCell, TerminalColor};

    fn snapshot(lines: &[&str], columns: u16) -> PaneSnapshot {
        let cells = lines
            .iter()
            .flat_map(|line| {
                let mut row: Vec<char> = line.chars().collect();
                row.resize(usize::from(columns), ' ');
                row
            })
            .map(|character| RenderCell {
                character,
                foreground: TerminalColor::DefaultForeground,
                background: TerminalColor::DefaultBackground,
                attributes: CellAttributes::default(),
            })
            .collect();
        PaneSnapshot {
            pane: PaneId(1),
            columns,
            rows: lines.len() as u16,
            cursor: Cursor { column: 0, row: 0 },
            alternate_screen: false,
            mouse: MouseMode::default(),
            bracketed_paste: false,
            cells,
        }
    }

    const AREA: Rect = Rect {
        x: 10,
        y: 5,
        width: 12,
        height: 3,
    };

    fn dragged(from: (u16, u16), to: (u16, u16)) -> PaneSelection {
        let mut selection =
            PaneSelection::pressed(PaneId(1), AREA, AREA.x + from.0, AREA.y + from.1);
        selection.follow(AREA, AREA.x + to.0, AREA.y + to.1);
        selection
    }

    #[test]
    fn a_press_that_never_moved_selects_nothing() {
        let selection = PaneSelection::pressed(PaneId(1), AREA, 12, 6);
        assert!(!selection.is_visible());
        assert_eq!(selection.text(&snapshot(&["hello"], 12)), "");
    }

    #[test]
    fn a_drag_across_rows_reads_as_text_without_the_padding() {
        let pane = snapshot(&["first line", "second", "third"], 12);
        assert_eq!(dragged((6, 0), (2, 2)).text(&pane), "line\nsecond\nthi");
    }

    #[test]
    fn a_drag_backwards_selects_the_same_text() {
        let pane = snapshot(&["first line", "second", "third"], 12);
        assert_eq!(
            dragged((2, 2), (6, 0)).text(&pane),
            dragged((6, 0), (2, 2)).text(&pane)
        );
    }

    #[test]
    fn a_drag_past_the_edge_selects_to_the_edge() {
        let pane = snapshot(&["abc", "defghijklmno"], 12);
        let mut selection = PaneSelection::pressed(PaneId(1), AREA, AREA.x, AREA.y + 1);
        selection.follow(AREA, AREA.right() + 20, AREA.y + 1);
        assert_eq!(selection.text(&pane), "defghijklmno");
    }

    #[test]
    fn the_blank_a_wide_character_spills_into_is_not_copied() {
        let pane = snapshot(&["日 本 x"], 12);
        assert_eq!(dragged((0, 0), (4, 0)).text(&pane), "日本x");
    }

    #[test]
    fn highlight_follows_reading_order() {
        let selection = dragged((6, 0), (2, 2));
        assert!(selection.contains(11, 0));
        assert!(!selection.contains(5, 0));
        assert!(selection.contains(0, 1));
        assert!(selection.contains(2, 2));
        assert!(!selection.contains(3, 2));
    }

    #[test]
    fn osc52_carries_the_text_base64_encoded() {
        assert_eq!(osc52("hi!"), b"\x1b]52;c;aGkh\x07");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
