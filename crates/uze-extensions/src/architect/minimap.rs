//! Where the screen is on the board.
//!
//! A drawing larger than the screen needs the one thing a scrollbar
//! cannot give in two dimensions: the whole of it, small, with the part
//! being looked at marked. Braille gives eight dots to a cell, which is
//! enough resolution for boxes to keep their arrangement — and it is
//! text, so it goes wherever the rest of the diagram goes.
//!
//! The geometry is one value both halves use: what is painted and what a
//! click on it means cannot disagree about where a dot is.

use crate::view::Role;

use super::canvas::{Canvas, Cell, Corners, Frame, Glyphs};

const DOTS_WIDE: i32 = 64;
const DOTS_TALL: i32 = 32;
/// Below this the map would cover more of the board than it explains.
const SMALLEST_SCREEN: (i32, i32) = (70, 18);

/// Braille's dot numbering, as the bit each `(column, row)` of a cell sets.
const DOT_BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Minimap {
    /// The framed map, in screen cells.
    pub frame: Frame,
    /// Board cells per dot across; a row of cells is twice as tall as it
    /// is wide, and a dot half a cell's width, so down is half of this.
    scale: i32,
}

impl Minimap {
    /// The map for a board of this size seen through a screen of that one,
    /// or `None` when the whole board is already on screen.
    pub fn of(board: (i32, i32), screen: (i32, i32)) -> Option<Self> {
        let fits = board.0 <= screen.0 && board.1 <= screen.1;
        if fits || screen.0 < SMALLEST_SCREEN.0 || screen.1 < SMALLEST_SCREEN.1 {
            return None;
        }
        let across = (board.0 + DOTS_WIDE - 1) / DOTS_WIDE;
        let down = (board.1 * 2 + DOTS_TALL - 1) / DOTS_TALL;
        let scale = across.max(down).max(1);
        let dots = (board.0 / scale + 1, board.1 * 2 / scale + 1);
        let (w, h) = ((dots.0 + 1) / 2 + 2, (dots.1 + 3) / 4 + 2);
        Some(Self {
            frame: Frame {
                x: screen.0 - w - 1,
                y: screen.1 - h,
                w,
                h,
            },
            scale,
        })
    }

    fn dot_of(&self, x: i32, y: i32) -> (i32, i32) {
        (x / self.scale, y * 2 / self.scale)
    }

    /// The board cell a click on this screen cell of the map points at.
    pub fn board_cell_at(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let inside = Frame {
            x: self.frame.x + 1,
            y: self.frame.y + 1,
            w: self.frame.w - 2,
            h: self.frame.h - 2,
        };
        inside.contains(x, y).then(|| {
            let dot = ((x - inside.x) * 2 + 1, (y - inside.y) * 4 + 2);
            (dot.0 * self.scale, dot.1 * self.scale / 2)
        })
    }

    pub fn paint(&self, screen: &mut Canvas, boxes: &[Frame], looking_at: Frame) {
        let (w, h) = (self.frame.w - 2, self.frame.h - 2);
        let mut dots = vec![0u32; (w * h) as usize];
        let mut edge = vec![false; (w * h) as usize];
        let mut mark = |x: i32, y: i32, on_edge: bool| {
            let (dx, dy) = self.dot_of(x, y);
            let (cx, cy) = (dx / 2, dy / 4);
            if cx >= 0 && cy >= 0 && cx < w && cy < h {
                let index = (cy * w + cx) as usize;
                dots[index] |= DOT_BITS[(dx % 2) as usize][(dy % 4) as usize];
                edge[index] |= on_edge;
            }
        };
        for frame in boxes {
            for y in frame.y..frame.y + frame.h {
                for x in frame.x..frame.x + frame.w {
                    mark(x, y, false);
                }
            }
        }
        let (right, bottom) = (
            looking_at.x + looking_at.w - 1,
            looking_at.y + looking_at.h - 1,
        );
        for x in looking_at.x..=right {
            mark(x, looking_at.y, true);
            mark(x, bottom, true);
        }
        for y in looking_at.y..=bottom {
            mark(looking_at.x, y, true);
            mark(right, y, true);
        }

        for y in self.frame.y..self.frame.y + self.frame.h {
            for x in self.frame.x..self.frame.x + self.frame.w {
                screen.set(x, y, Cell::default());
            }
        }
        screen.frame(self.frame, Corners::Rounded, Role::Faint);
        for cy in 0..h {
            for cx in 0..w {
                let index = (cy * w + cx) as usize;
                if dots[index] == 0 {
                    continue;
                }
                let glyph = match screen.glyphs {
                    Glyphs::Unicode => char::from_u32(0x2800 + dots[index]).unwrap_or(' '),
                    Glyphs::Ascii => '#',
                };
                let role = if edge[index] {
                    Role::Accent
                } else {
                    Role::Muted
                };
                screen.put(
                    self.frame.x + 1 + cx,
                    self.frame.y + 1 + cy,
                    glyph,
                    role,
                    false,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_that_fits_the_screen_needs_no_map() {
        assert_eq!(Minimap::of((80, 30), (120, 40)), None);
        assert!(Minimap::of((180, 30), (120, 40)).is_some());
    }

    #[test]
    fn a_click_on_the_map_points_back_at_the_part_of_the_board_drawn_there() {
        let map = Minimap::of((190, 80), (120, 40)).unwrap();
        let far = map
            .board_cell_at(map.frame.x + map.frame.w - 2, map.frame.y + 1)
            .unwrap();
        let near = map.board_cell_at(map.frame.x + 1, map.frame.y + 1).unwrap();
        assert!(far.0 > near.0 + 100, "{near:?} → {far:?}");
        assert_eq!(map.board_cell_at(0, 0), None);
    }
}
