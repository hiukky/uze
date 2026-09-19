//! How an edge gets from one box to another.
//!
//! A shortest-path search over the cell grid itself, rather than channels
//! reserved by the layout: the grid is small, the search is exact about
//! what is already drawn, and it needs no special case for an edge that
//! climbs back up, leaves a boundary or has to go around the outside.
//!
//! What makes the result read as a diagram is in the costs. A bend is
//! dear, so lines run straight; crossing another line is dearer, so they
//! go around when around is near; and running *along* a foreign line is
//! refused outright, because two edges sharing cells are one edge to
//! whoever reads it. The exception is deliberate — edges with the same
//! source, or the same target, may share a trunk and fork, which is how
//! a hand-drawn diagram does it too.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::shared::canvas::Frame;

use super::{
    layout::Placement,
    model::{Flow, Graph},
};

const STEP: i32 = 2;
const TURN: i32 = 6;
const CROSSING: i32 = 10;
const BOUNDARY: i32 = 3;
/// Hugging a box it has nothing to do with.
const HALO: i32 = 3;
/// Leaving or arriving by a side that does not face the other end.
const SIDE: i32 = 12;
/// Riding a trunk a mate already drew. Small, so edges still bundle where
/// there is one way through, and enough that they part early — a label
/// can only say whose it is on a stretch that belongs to one edge.
const SHARED: i32 = 1;

/// North, east, south, west — the index is the bit in a cell's sides.
const HEADINGS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

fn opposite(heading: usize) -> usize {
    (heading + 2) % 4
}

fn is_horizontal(heading: usize) -> bool {
    heading % 2 == 1
}

pub struct Route {
    pub edge: usize,
    /// Every cell the line occupies, from beside the source to beside
    /// the target, with the heading it was entered by.
    pub cells: Vec<(i32, i32, usize)>,
}

pub struct Routes {
    pub routes: Vec<Route>,
    pub unrouted: usize,
    width: i32,
    usage: Vec<u8>,
}

impl Routes {
    /// Whether more than one edge runs through this cell — a trunk, where
    /// a label would not say whose it is.
    pub fn is_shared(&self, x: i32, y: i32) -> bool {
        self.usage
            .get((y * self.width + x) as usize)
            .is_some_and(|&count| count > 1)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Boundary {
    None,
    Horizontal,
    Vertical,
}

/// The two ends an edge joins. Two edges are mates when they share one.
type Ends = (usize, usize);

fn mates(a: Ends, b: Ends) -> bool {
    a.0 == b.0 || a.1 == b.1
}

struct Grid {
    width: i32,
    height: i32,
    blocked: Vec<bool>,
    halo: Vec<bool>,
    boundary: Vec<Boundary>,
    horizontal: Vec<Option<Ends>>,
    vertical: Vec<Option<Ends>>,
}

impl Grid {
    fn new(graph: &Graph, placement: &Placement) -> Self {
        let size = (placement.width * placement.height).max(0) as usize;
        let mut grid = Self {
            width: placement.width,
            height: placement.height,
            blocked: vec![false; size],
            halo: vec![false; size],
            boundary: vec![Boundary::None; size],
            horizontal: vec![None; size],
            vertical: vec![None; size],
        };
        for (frame, cluster) in placement.clusters.iter().zip(&graph.clusters) {
            grid.fence(*frame, crate::shared::canvas::text_width(&cluster.title));
        }
        for frame in &placement.nodes {
            for y in frame.y - 1..=frame.y + frame.h {
                for x in frame.x - 1..=frame.x + frame.w {
                    if let Some(index) = grid.index(x, y) {
                        grid.halo[index] = true;
                        grid.blocked[index] |= frame.contains(x, y);
                    }
                }
            }
        }
        grid
    }

    /// A boundary can be crossed, square on and away from its corners
    /// and its title — never walked along.
    fn fence(&mut self, frame: Frame, title_width: i32) {
        let (right, bottom) = (frame.x + frame.w - 1, frame.y + frame.h - 1);
        for x in frame.x..=right {
            for y in [frame.y, bottom] {
                if let Some(index) = self.index(x, y) {
                    let corner = x == frame.x || x == right;
                    let titled = y == frame.y && x <= frame.x + title_width + 3;
                    self.boundary[index] = Boundary::Horizontal;
                    self.blocked[index] |= corner || titled;
                }
            }
        }
        for y in frame.y + 1..bottom {
            for x in [frame.x, right] {
                if let Some(index) = self.index(x, y) {
                    self.boundary[index] = Boundary::Vertical;
                }
            }
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width && y < self.height)
            .then(|| (y * self.width + x) as usize)
    }

    fn owners(&self, index: usize, heading: usize) -> (Option<Ends>, Option<Ends>) {
        if is_horizontal(heading) {
            (self.horizontal[index], self.vertical[index])
        } else {
            (self.vertical[index], self.horizontal[index])
        }
    }

    /// What entering this cell by `heading` costs, or `None` if it may
    /// not be entered that way at all.
    fn entering(&self, x: i32, y: i32, heading: usize, ends: Ends) -> Option<i32> {
        let index = self.index(x, y)?;
        if self.blocked[index] {
            return None;
        }
        let along = match self.boundary[index] {
            Boundary::Horizontal => is_horizontal(heading),
            Boundary::Vertical => !is_horizontal(heading),
            Boundary::None => false,
        };
        if along {
            return None;
        }
        let (same, across) = self.owners(index, heading);
        if same.is_some_and(|owner| !mates(owner, ends)) {
            return None;
        }
        let mut cost = STEP;
        if same.is_some() {
            cost += SHARED;
        }
        if self.halo[index] {
            cost += HALO;
        }
        if self.boundary[index] != Boundary::None {
            cost += BOUNDARY;
        }
        if across.is_some_and(|owner| !mates(owner, ends)) {
            cost += CROSSING;
        }
        Some(cost)
    }

    /// A bend needs a cell of its own: not on a boundary, and not where
    /// a foreign line already runs, since a corner drawn on a crossing
    /// reads as the two lines joining.
    fn may_turn(&self, index: usize, ends: Ends) -> bool {
        let foreign = |owner: Option<Ends>| owner.is_some_and(|owner| !mates(owner, ends));
        self.boundary[index] == Boundary::None
            && !foreign(self.horizontal[index])
            && !foreign(self.vertical[index])
    }

    fn claim(&mut self, index: usize, heading: usize, ends: Ends) {
        let lane = if is_horizontal(heading) {
            &mut self.horizontal[index]
        } else {
            &mut self.vertical[index]
        };
        lane.get_or_insert(ends);
    }
}

pub fn route(graph: &Graph, placement: &Placement) -> Routes {
    let mut grid = Grid::new(graph, placement);
    let mut order: Vec<usize> = (0..graph.edges.len())
        .filter(|&edge| graph.edges[edge].from != graph.edges[edge].to)
        .collect();
    // Short edges first: they have one good path and nothing to go
    // around yet, and a long edge can afford the detour they leave it.
    order.sort_by_key(|&edge| {
        let (ax, ay) = placement.nodes[graph.edges[edge].from].center();
        let (bx, by) = placement.nodes[graph.edges[edge].to].center();
        (ax - bx).abs() + (ay - by).abs()
    });
    let mut routes = Routes {
        routes: Vec::new(),
        unrouted: 0,
        width: placement.width,
        usage: vec![0; grid.blocked.len()],
    };
    for edge in order {
        let ends = (graph.edges[edge].from, graph.edges[edge].to);
        let from = placement.nodes[ends.0];
        let to = placement.nodes[ends.1];
        match shortest(&grid, graph.flow, from, to, ends) {
            Some(cells) => {
                for (position, &(x, y, heading)) in cells.iter().enumerate() {
                    let index = grid.index(x, y).expect("a routed cell is on the grid");
                    grid.claim(index, heading, ends);
                    if let Some(&(_, _, leaving)) = cells.get(position + 1) {
                        grid.claim(index, leaving, ends);
                    }
                    routes.usage[index] = routes.usage[index].saturating_add(1);
                }
                routes.routes.push(Route { edge, cells });
            }
            None => routes.unrouted += 1,
        }
    }
    routes.routes.sort_by_key(|route| route.edge);
    routes
}

/// The side of `from` that faces `to`, as a heading.
fn facing(flow: Flow, from: Frame, to: Frame) -> usize {
    let below = to.y >= from.y + from.h;
    let above = to.y + to.h <= from.y;
    let right = to.x >= from.x + from.w;
    let left = to.x + to.w <= from.x;
    let (fx, fy) = from.center();
    let (tx, ty) = to.center();
    match flow {
        Flow::TopDown if below => 2,
        Flow::TopDown if above => 0,
        Flow::TopDown => {
            if tx >= fx {
                1
            } else {
                3
            }
        }
        Flow::LeftRight if right => 1,
        Flow::LeftRight if left => 3,
        Flow::LeftRight => {
            if ty >= fy {
                2
            } else {
                0
            }
        }
    }
}

/// The cells just outside `frame`, one side at a time and never at a
/// corner: `(x, y, side, distance from that side's middle)`.
fn ports(frame: Frame) -> Vec<(i32, i32, usize, i32)> {
    let (cx, cy) = frame.center();
    let mut ports = Vec::new();
    for x in frame.x + 1..frame.x + frame.w - 1 {
        ports.push((x, frame.y - 1, 0, (x - cx).abs()));
        ports.push((x, frame.y + frame.h, 2, (x - cx).abs()));
    }
    for y in frame.y + 1..frame.y + frame.h - 1 {
        ports.push((frame.x + frame.w, y, 1, (y - cy).abs()));
        ports.push((frame.x - 1, y, 3, (y - cy).abs()));
    }
    ports
}

fn side_cost(side: usize, preferred: usize) -> i32 {
    if side == preferred {
        0
    } else if side == opposite(preferred) {
        SIDE * 2
    } else {
        SIDE
    }
}

fn shortest(
    grid: &Grid,
    flow: Flow,
    from: Frame,
    to: Frame,
    ends: Ends,
) -> Option<Vec<(i32, i32, usize)>> {
    let states = grid.blocked.len() * 4;
    let state = |x: i32, y: i32, heading: usize| (y * grid.width + x) as usize * 4 + heading;
    let mut best = vec![i32::MAX; states];
    let mut came_from = vec![usize::MAX; states];
    let mut arrival = vec![-1; states];
    let mut open = BinaryHeap::new();

    let exit = facing(flow, from, to);
    let entry = opposite(facing(flow, from, to));
    for (x, y, side, off_centre) in ports(to) {
        if grid.index(x, y).is_some() {
            // Arriving by a side means travelling *into* it.
            arrival[state(x, y, opposite(side))] = side_cost(side, entry) + off_centre;
        }
    }
    let remaining = |x: i32, y: i32| {
        let dx = (to.x - 1 - x).max(x - (to.x + to.w)).max(0);
        let dy = (to.y - 1 - y).max(y - (to.y + to.h)).max(0);
        (dx + dy) * STEP
    };
    for (x, y, side, off_centre) in ports(from) {
        let Some(cost) = grid.entering(x, y, side, ends) else {
            continue;
        };
        let cost = cost + side_cost(side, exit) + off_centre;
        let start = state(x, y, side);
        if cost < best[start] {
            best[start] = cost;
            open.push(Reverse((cost + remaining(x, y), cost, x, y, side)));
        }
    }

    let mut finish: Option<(i32, usize)> = None;
    while let Some(Reverse((estimate, cost, x, y, heading))) = open.pop() {
        if finish.is_some_and(|(total, _)| estimate >= total) {
            break;
        }
        let here = state(x, y, heading);
        if cost > best[here] {
            continue;
        }
        if arrival[here] >= 0 {
            let total = cost + arrival[here];
            if finish.is_none_or(|(known, _)| total < known) {
                finish = Some((total, here));
            }
        }
        let index = here / 4;
        // The cell beside the source is a stub, never a corner: a line
        // that bends the moment it leaves a box reads as part of its border.
        let may_turn = came_from[here] != usize::MAX && grid.may_turn(index, ends);
        for (next, (dx, dy)) in HEADINGS.iter().enumerate() {
            if next == opposite(heading) || (next != heading && !may_turn) {
                continue;
            }
            let (nx, ny) = (x + dx, y + dy);
            let Some(step) = grid.entering(nx, ny, next, ends) else {
                continue;
            };
            let total = cost + step + if next == heading { 0 } else { TURN };
            let there = state(nx, ny, next);
            if total < best[there] {
                best[there] = total;
                came_from[there] = here;
                open.push(Reverse((total + remaining(nx, ny), total, nx, ny, next)));
            }
        }
    }

    let (_, mut at) = finish?;
    let mut cells = Vec::new();
    while at != usize::MAX {
        let index = (at / 4) as i32;
        cells.push((index % grid.width, index / grid.width, at % 4));
        at = came_from[at];
    }
    cells.reverse();
    Some(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architect::{layout, mermaid, model::Diagram};

    fn routed(source: &str) -> (Placement, Routes) {
        let Diagram::Graph(graph) = mermaid::parse(source).unwrap() else {
            panic!("expected a graph");
        };
        let placement = layout::place(&graph);
        let routes = route(&graph, &placement);
        (placement, routes)
    }

    #[test]
    fn every_edge_of_a_tangle_is_routed_and_none_enters_a_box() {
        let (placement, routes) = routed(
            "flowchart TD\n a --> b & c & d\n b --> e\n c --> e\n d --> a\n e --> a\n b --> d",
        );
        assert_eq!(routes.unrouted, 0);
        assert_eq!(routes.routes.len(), 8);
        for route in &routes.routes {
            for &(x, y, _) in &route.cells {
                assert!(
                    placement.node_at(x, y).is_none(),
                    "({x},{y}) is inside a box"
                );
            }
        }
    }

    #[test]
    fn a_route_is_one_unbroken_line_that_ends_facing_its_target() {
        let (placement, routes) = routed("flowchart LR\n a --> b\n b --> c\n a --> c");
        for route in &routes.routes {
            for pair in route.cells.windows(2) {
                let ((ax, ay, _), (bx, by, heading)) = (pair[0], pair[1]);
                assert_eq!((bx - ax, by - ay), HEADINGS[heading]);
            }
            let &(x, y, heading) = route.cells.last().unwrap();
            let (dx, dy) = HEADINGS[heading];
            assert!(placement.node_at(x + dx, y + dy).is_some());
        }
    }
}
