//! A laid-out, routed graph, turned into cells.
//!
//! Kept apart from the layout because it is the cheap half: choosing
//! another glyph set or selecting a node repaints, and neither has any
//! reason to move a box or re-run a search.

use crate::view::Role;

use super::{
    canvas::{Canvas, Corners, EAST, Frame, Glyphs, NORTH, SOUTH, WEST, arrow_glyph, text_width},
    layout::{self, Placement, TextKind},
    model::{Graph, Node, Shape, Stroke},
    route::{self, Route, Routes},
};

pub struct Scene {
    pub graph: Graph,
    pub placement: Placement,
    pub routes: Routes,
}

impl Scene {
    pub fn of(graph: Graph) -> Self {
        let placement = layout::place(&graph);
        let routes = route::route(&graph, &placement);
        Self {
            graph,
            placement,
            routes,
        }
    }

    fn touches(&self, route: &Route, selected: Option<usize>) -> bool {
        let edge = &self.graph.edges[route.edge];
        selected.is_some_and(|node| edge.from == node || edge.to == node)
    }
}

/// Which sides of each cell a line leaves by, and how that line is drawn.
struct Lines {
    width: i32,
    sides: Vec<u8>,
    stroke: Vec<Stroke>,
    lit: Vec<bool>,
}

impl Lines {
    fn index(&self, x: i32, y: i32) -> usize {
        (y * self.width + x) as usize
    }
}

pub fn paint(scene: &Scene, glyphs: Glyphs, selected: Option<usize>) -> Canvas {
    let Placement { width, height, .. } = scene.placement;
    let mut canvas = Canvas::new(width, height, glyphs);
    let size = (width * height) as usize;
    let mut lines = Lines {
        width,
        sides: vec![0; size],
        stroke: vec![Stroke::Solid; size],
        lit: vec![false; size],
    };
    let mut fences = vec![0u8; size];

    for (frame, cluster) in scene.placement.clusters.iter().zip(&scene.graph.clusters) {
        canvas.frame(*frame, Corners::Square, Role::Faint);
        canvas.text(
            frame.x + 2,
            frame.y,
            &format!(" {} ", cluster.title),
            Role::Secondary,
            true,
        );
        for x in frame.x + 1..frame.x + frame.w - 1 {
            fences[lines.index(x, frame.y)] = EAST | WEST;
            fences[lines.index(x, frame.y + frame.h - 1)] = EAST | WEST;
        }
        for y in frame.y + 1..frame.y + frame.h - 1 {
            fences[lines.index(frame.x, y)] = NORTH | SOUTH;
            fences[lines.index(frame.x + frame.w - 1, y)] = NORTH | SOUTH;
        }
    }

    let mut arrows = Vec::new();
    for route in &scene.routes.routes {
        let edge = &scene.graph.edges[route.edge];
        let lit = scene.touches(route, selected);
        for (position, &(x, y, heading)) in route.cells.iter().enumerate() {
            let index = lines.index(x, y);
            let back = 1 << ((heading + 2) % 4);
            let onward = match route.cells.get(position + 1) {
                Some(&(_, _, next)) => 1 << next,
                None if edge.arrow => {
                    arrows.push((x, y, 1u8 << heading, lit));
                    0
                }
                None => 1 << heading,
            };
            // On a shared trunk the plainest stroke wins: a dotted edge
            // riding a solid one must not make the solid one look optional.
            if lines.sides[index] == 0 || edge.stroke != Stroke::Dotted {
                lines.stroke[index] = edge.stroke;
            }
            lines.sides[index] |= back | onward;
            lines.lit[index] |= lit;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let index = lines.index(x, y);
            if lines.sides[index] != 0 {
                let role = if lines.lit[index] {
                    Role::Accent
                } else {
                    Role::Dim
                };
                canvas.line(
                    x,
                    y,
                    lines.sides[index] | fences[index],
                    lines.stroke[index],
                    role,
                );
            }
        }
    }
    for (x, y, heading, lit) in arrows {
        let role = if lit { Role::Accent } else { Role::Muted };
        canvas.put(x, y, arrow_glyph(heading, glyphs), role, false);
    }

    for (index, node) in scene.graph.nodes.iter().enumerate() {
        paint_node(
            &mut canvas,
            node,
            scene.placement.nodes[index],
            selected == Some(index),
        );
    }
    for route in &scene.routes.routes {
        if let Some(label) = scene.graph.edges[route.edge].label.as_deref() {
            let role = if scene.touches(route, selected) {
                Role::Accent
            } else {
                Role::Secondary
            };
            // A label with nowhere to go is left off rather than written
            // over a line: a missing word costs less than a wrong edge.
            if !write_inline(&mut canvas, &mut lines, &scene.routes, route, label, role) {
                write_beside(&mut canvas, &lines, &scene.routes, route, label, role);
            }
        }
    }
    canvas
}

fn paint_node(canvas: &mut Canvas, node: &Node, frame: Frame, selected: bool) {
    let corners = match node.shape {
        Shape::Box => Corners::Square,
        Shape::Decision => Corners::Marked,
        Shape::Round | Shape::Database | Shape::Person => Corners::Rounded,
    };
    let border = match (selected, node.external) {
        (true, _) => Role::Accent,
        (_, true) => Role::Faint,
        _ => Role::Muted,
    };
    canvas.frame(frame, corners, border);
    let mut row = frame.y + 1;
    if node.shape == Shape::Database {
        let right = frame.x + frame.w - 1;
        canvas.line(frame.x, row, NORTH | EAST | SOUTH, Stroke::Solid, border);
        canvas.line(right, row, NORTH | SOUTH | WEST, Stroke::Solid, border);
        for x in frame.x + 1..right {
            canvas.line(x, row, EAST | WEST, Stroke::Solid, border);
        }
        row += 1;
    }
    for (kind, text) in layout::node_text(node) {
        let (role, bold) = match (kind, selected, node.external) {
            (TextKind::Title, true, _) => (Role::Accent, true),
            (TextKind::Title, _, true) => (Role::Muted, false),
            (TextKind::Title, ..) => (Role::Bright, true),
            (TextKind::Kind, ..) => (Role::Dim, false),
            _ => (Role::Muted, false),
        };
        let x = frame.x + 1 + (frame.w - 2 - text_width(&text)) / 2;
        canvas.text(x, row, &text, role, bold);
        row += 1;
    }
}

/// On the line itself, where a horizontal run is long enough to be
/// interrupted by it — and preferably a run no other edge shares.
fn write_inline(
    canvas: &mut Canvas,
    lines: &mut Lines,
    routes: &Routes,
    route: &Route,
    label: &str,
    role: Role,
) -> bool {
    let needed = text_width(label) + 2;
    for allow_shared in [false, true] {
        let mut run: Vec<(i32, i32)> = Vec::new();
        let body = &route.cells[..route.cells.len().saturating_sub(1)];
        for &(x, y, _) in body.iter().chain(std::iter::once(&(i32::MIN, 0, 0))) {
            let plain = x != i32::MIN
                && lines.sides[lines.index(x, y)] == EAST | WEST
                && (allow_shared || !routes.is_shared(x, y));
            let continues = plain
                && run
                    .last()
                    .is_none_or(|&(px, py)| py == y && (px - x).abs() == 1);
            if continues {
                run.push((x, y));
                continue;
            }
            if run.len() as i32 >= needed + 2 {
                let left = run.iter().map(|&(x, _)| x).min().unwrap_or(0);
                let start = left + (run.len() as i32 - needed) / 2;
                canvas.text(start, run[0].1, &format!(" {label} "), role, false);
                // No longer a line, so the next label cannot land on this one.
                for x in start..start + needed {
                    let index = lines.index(x, run[0].1);
                    lines.sides[index] = 0;
                }
                return true;
            }
            run.clear();
            if plain {
                run.push((x, y));
            }
        }
    }
    false
}

/// Beside a vertical run, nearest its middle, wherever the cells are
/// free — on one row, or broken over two when one is too narrow a gap.
fn write_beside(
    canvas: &mut Canvas,
    lines: &Lines,
    routes: &Routes,
    route: &Route,
    label: &str,
    role: Role,
) -> bool {
    let broken = broken_in_two(label);
    let shapes: Vec<Vec<&str>> = std::iter::once(vec![label])
        .chain(
            broken
                .as_ref()
                .map(|(first, second)| vec![first.as_str(), second.as_str()]),
        )
        .collect();
    for rows in &shapes {
        let width = rows.iter().map(|row| text_width(row)).max().unwrap_or(0);
        for allow_shared in [false, true] {
            let upright: Vec<(i32, i32)> = route
                .cells
                .iter()
                .map(|&(x, y, _)| (x, y))
                .filter(|&(x, y)| lines.sides[lines.index(x, y)] == NORTH | SOUTH)
                .filter(|&(x, y)| allow_shared || !routes.is_shared(x, y))
                .collect();
            let middle = upright.len() / 2;
            let mut nearest: Vec<usize> = (0..upright.len()).collect();
            nearest.sort_by_key(|&index| index.abs_diff(middle));
            for index in nearest {
                let (x, y) = upright[index];
                for start in [x + 2, x - 1 - width] {
                    let free = (0..rows.len() as i32).all(|row| {
                        (start - 1..=start + width).all(|column| canvas.is_blank(column, y + row))
                    });
                    if free {
                        for (row, text) in rows.iter().enumerate() {
                            canvas.text(start, y + row as i32, text, role, false);
                        }
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// The label split at the space nearest its middle, if it has one.
fn broken_in_two(label: &str) -> Option<(String, String)> {
    let middle = label.len() / 2;
    let at = label
        .match_indices(' ')
        .map(|(index, _)| index)
        .min_by_key(|index| index.abs_diff(middle))?;
    Some((label[..at].to_owned(), label[at + 1..].to_owned()))
}
