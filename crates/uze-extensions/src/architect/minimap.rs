//! Where the screen is on the board.
//!
//! A drawing larger than the screen needs the one thing a scrollbar
//! cannot give in two dimensions: the whole of it, small, with the part
//! being looked at marked. Braille gives eight dots to a cell, which is
//! enough resolution for boxes to keep their arrangement — and it is
//! text, so it goes wherever the rest of the diagram goes.
//!
//! The map is one size, in one place, for every drawing: the drawing is
//! scaled to fit the map, never the map to fit the drawing. A control
//! that changes shape between two diagrams has to be found again each
//! time, and this one exists so that nothing has to be looked for.
//!
//! The geometry is one value both halves use: what is painted and what a
//! click on it means cannot disagree about where a dot is.

use crate::view::Role;

use super::canvas::{Canvas, Cell, Corners, Frame, Glyphs};

const CELLS: (i32, i32) = (28, 7);
const DOTS: (i32, i32) = (CELLS.0 * 2, CELLS.1 * 4);
/// Below this the map would cover more of the board than it explains.
const SMALLEST_SCREEN: (i32, i32) = (70, 18);

/// Braille's dot numbering, as the bit each `(column, row)` of a cell sets.
const DOT_BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minimap {
    /// The framed map, in screen cells.
    pub frame: Frame,
    /// Board cells per dot across. A cell is twice as tall as it is wide
    /// and a dot is half a cell wide, so a dot down is half as many rows.
    scale: f32,
    /// Where the drawing starts inside the map, in dots: it is centred,
    /// since it rarely has the map's proportions.
    origin: (i32, i32),
}

impl Minimap {
    pub fn of(board: (i32, i32), screen: (i32, i32)) -> Option<Self> {
        if screen.0 < SMALLEST_SCREEN.0 || screen.1 < SMALLEST_SCREEN.1 || board.0 < 1 {
            return None;
        }
        let across = board.0 as f32 / DOTS.0 as f32;
        let down = board.1 as f32 * 2.0 / DOTS.1 as f32;
        let scale = across.max(down).max(0.01);
        let drawn = (
            (board.0 as f32 / scale) as i32,
            (board.1 as f32 * 2.0 / scale) as i32,
        );
        Some(Self {
            frame: Frame {
                x: screen.0 - CELLS.0 - 3,
                y: screen.1 - CELLS.1 - 2,
                w: CELLS.0 + 2,
                h: CELLS.1 + 2,
            },
            scale,
            origin: ((DOTS.0 - drawn.0) / 2, (DOTS.1 - drawn.1) / 2),
        })
    }

    fn dot_of(&self, x: i32, y: i32) -> (i32, i32) {
        (
            self.origin.0 + (x as f32 / self.scale) as i32,
            self.origin.1 + (y as f32 * 2.0 / self.scale) as i32,
        )
    }

    /// The board cell a click on this screen cell of the map points at.
    pub fn board_cell_at(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let inside = Frame {
            x: self.frame.x + 1,
            y: self.frame.y + 1,
            w: CELLS.0,
            h: CELLS.1,
        };
        inside.contains(x, y).then(|| {
            let dot = (
                (x - inside.x) * 2 + 1 - self.origin.0,
                (y - inside.y) * 4 + 2 - self.origin.1,
            );
            (
                (dot.0 as f32 * self.scale) as i32,
                (dot.1 as f32 * self.scale / 2.0) as i32,
            )
        })
    }

    pub fn paint(&self, screen: &mut Canvas, boxes: &[Frame], looking_at: Frame) {
        let mut dots = vec![0u32; (CELLS.0 * CELLS.1) as usize];
        let mut edge = vec![false; (CELLS.0 * CELLS.1) as usize];
        let mut mark = |dot: (i32, i32), on_edge: bool| {
            let inside = dot.0 >= 0 && dot.1 >= 0 && dot.0 < DOTS.0 && dot.1 < DOTS.1;
            if inside {
                let index = (dot.1 / 4 * CELLS.0 + dot.0 / 2) as usize;
                dots[index] |= DOT_BITS[(dot.0 % 2) as usize][(dot.1 % 4) as usize];
                edge[index] |= on_edge;
            }
        };
        for frame in boxes {
            let (from, to) = (
                self.dot_of(frame.x, frame.y),
                self.dot_of(frame.x + frame.w - 1, frame.y + frame.h - 1),
            );
            for y in from.1..=to.1 {
                for x in from.0..=to.0 {
                    mark((x, y), false);
                }
            }
        }
        // The part being looked at, as an outline — cut where it leaves
        // the map, which is what says the screen has gone past the edge.
        let (from, to) = (
            self.dot_of(looking_at.x, looking_at.y),
            self.dot_of(
                looking_at.x + looking_at.w - 1,
                looking_at.y + looking_at.h - 1,
            ),
        );
        for x in from.0..=to.0 {
            mark((x, from.1), true);
            mark((x, to.1), true);
        }
        for y in from.1..=to.1 {
            mark((from.0, y), true);
            mark((to.0, y), true);
        }

        for y in self.frame.y..self.frame.y + self.frame.h {
            for x in self.frame.x..self.frame.x + self.frame.w {
                screen.set(
                    x,
                    y,
                    Cell {
                        solid: true,
                        ..Cell::default()
                    },
                );
            }
        }
        screen.frame(self.frame, Corners::Rounded, Role::Dim);
        for cy in 0..CELLS.1 {
            for cx in 0..CELLS.0 {
                let index = (cy * CELLS.0 + cx) as usize;
                if dots[index] == 0 {
                    continue;
                }
                let glyph = match screen.glyphs {
                    Glyphs::Unicode => char::from_u32(0x2800 + dots[index]).unwrap_or(' '),
                    Glyphs::Ascii if edge[index] => '+',
                    Glyphs::Ascii => '#',
                };
                let role = if edge[index] {
                    Role::Accent
                } else {
                    Role::Secondary
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
    fn the_map_is_one_size_in_one_place_whatever_the_drawing() {
        let screen = (120, 40);
        let wide = Minimap::of((300, 25), screen).unwrap();
        let tall = Minimap::of((60, 200), screen).unwrap();
        let small = Minimap::of((40, 12), screen).unwrap();
        assert_eq!(wide.frame, tall.frame);
        assert_eq!(wide.frame, small.frame);
    }

    #[test]
    fn a_click_on_the_map_points_back_at_the_part_of_the_board_drawn_there() {
        let map = Minimap::of((190, 80), (120, 40)).unwrap();
        let near = map.board_cell_at(map.frame.x + 1, map.frame.y + 1).unwrap();
        let far = map
            .board_cell_at(map.frame.x + map.frame.w - 2, map.frame.y + 1)
            .unwrap();
        assert!(far.0 > near.0 + 100, "{near:?} → {far:?}");
        assert_eq!(map.board_cell_at(0, 0), None);
    }

    #[test]
    fn a_point_on_the_board_and_the_click_on_its_dot_agree() {
        let map = Minimap::of((190, 80), (120, 40)).unwrap();
        let dot = map.dot_of(100, 40);
        let cell = (map.frame.x + 1 + dot.0 / 2, map.frame.y + 1 + dot.1 / 4);
        let back = map.board_cell_at(cell.0, cell.1).unwrap();
        assert!(
            (back.0 - 100).abs() <= 8 && (back.1 - 40).abs() <= 6,
            "{back:?}"
        );
    }
}
