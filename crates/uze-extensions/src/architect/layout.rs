//! Where every box goes.
//!
//! A layered layout — rank by longest path, order by barycentre, then
//! pull each box toward what it connects to — run once per container and
//! from the inside out: a cluster is laid out on its own, and then it is
//! one box in its parent's layout. That recursion is what keeps a
//! boundary's members together without the layout knowing what a
//! boundary means.

use std::collections::BTreeSet;

use crate::shared::canvas::{Frame, text_width, wrapped};

use super::model::{Flow, Graph, Node, Shape};

/// Free cells around the whole diagram, so an edge that has to go around
/// the outside has an outside to go around.
const MARGIN: i32 = 3;
const DESCRIPTION_WIDTH: i32 = 26;
const SWEEPS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextKind {
    Title,
    Kind,
    Description,
    Blank,
}

pub struct Placement {
    pub nodes: Vec<Frame>,
    pub clusters: Vec<Frame>,
    pub width: i32,
    pub height: i32,
}

impl Placement {
    pub fn node_at(&self, x: i32, y: i32) -> Option<usize> {
        self.nodes.iter().position(|frame| frame.contains(x, y))
    }
}

/// What a node's box says, a line at a time. One source for the size the
/// layout reserves and the text the painter writes, so they cannot drift.
pub fn node_text(node: &Node) -> Vec<(TextKind, String)> {
    let mut lines = vec![(TextKind::Title, node.title.clone())];
    if let Some(kind) = &node.kind {
        lines.push((TextKind::Kind, format!("[{kind}]")));
    }
    if let Some(description) = &node.description {
        let measure = DESCRIPTION_WIDTH.max(text_width(&node.title));
        if node.kind.is_some() {
            lines.push((TextKind::Blank, String::new()));
        }
        lines.extend(
            wrapped(description, measure)
                .into_iter()
                .map(|line| (TextKind::Description, line)),
        );
    }
    lines
}

fn node_size(node: &Node) -> (i32, i32) {
    let lines = node_text(node);
    let widest = lines
        .iter()
        .map(|(_, line)| text_width(line))
        .max()
        .unwrap_or(0);
    let lid = i32::from(node.shape == Shape::Database);
    (widest + 4, lines.len() as i32 + 2 + lid)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Item {
    Node(usize),
    Cluster(usize),
}

struct Child {
    item: Item,
    w: i32,
    h: i32,
    rank: usize,
    /// Position across the flow: `x` top-down, `y` left-right.
    cross: i32,
}

impl Child {
    fn cross_size(&self, flow: Flow) -> i32 {
        match flow {
            Flow::TopDown => self.w,
            Flow::LeftRight => self.h,
        }
    }

    fn main_size(&self, flow: Flow) -> i32 {
        match flow {
            Flow::TopDown => self.h,
            Flow::LeftRight => self.w,
        }
    }

    fn cross_center(&self, flow: Flow) -> i32 {
        self.cross + self.cross_size(flow) / 2
    }
}

pub fn place(graph: &Graph) -> Placement {
    let mut placement = Placement {
        nodes: vec![Frame::default(); graph.nodes.len()],
        clusters: vec![Frame::default(); graph.clusters.len()],
        width: 0,
        height: 0,
    };
    let (w, h) = place_container(graph, None, &mut placement);
    shift(graph, None, &mut placement, MARGIN, MARGIN);
    placement.width = w + MARGIN * 2;
    placement.height = h + MARGIN * 2;
    placement
}

/// Lays out what sits directly inside `container`, leaving every frame
/// relative to the container's own origin, and answers its size.
fn place_container(
    graph: &Graph,
    container: Option<usize>,
    placement: &mut Placement,
) -> (i32, i32) {
    let flow = graph.flow;
    let mut children: Vec<Child> = Vec::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        if node.cluster == container {
            let (w, h) = node_size(node);
            children.push(child(Item::Node(index), w, h));
        }
    }
    for (index, cluster) in graph.clusters.iter().enumerate() {
        if cluster.parent == container {
            let (w, h) = place_container(graph, Some(index), placement);
            // A border and a column of air each side; the title sits on
            // the top border, so it costs no row of its own.
            let w = (w + 6).max(text_width(&cluster.title) + 8);
            shift(graph, Some(index), placement, 3, 2);
            children.push(child(Item::Cluster(index), w, h + 4));
        }
    }
    if children.is_empty() {
        return (0, 0);
    }

    let links = lifted_links(graph, container, &children);
    if links.is_empty() {
        rank_as_grid(&mut children);
    } else {
        rank_by_longest_path(&mut children, &links);
    }
    let ranks = ordered_ranks(&children, &links);
    let gaps = rank_gaps(graph, container, &children, &links, ranks.len());
    spread(&mut children, &ranks, &links, flow);

    let mut main = 0;
    let lowest = children.iter().map(|c| c.cross).min().unwrap_or(0);
    let (mut width, mut height) = (0, 0);
    for (rank, members) in ranks.iter().enumerate() {
        let thickness = members
            .iter()
            .map(|&m| children[m].main_size(flow))
            .max()
            .unwrap_or(0);
        for &member in members {
            let child = &children[member];
            // Centred in its rank, so a short box beside a tall one does
            // not hang from the top of the row.
            let along = main + (thickness - child.main_size(flow)) / 2;
            let across = child.cross - lowest;
            let (x, y) = match flow {
                Flow::TopDown => (across, along),
                Flow::LeftRight => (along, across),
            };
            let frame = Frame {
                x,
                y,
                w: child.w,
                h: child.h,
            };
            width = width.max(x + child.w);
            height = height.max(y + child.h);
            match child.item {
                Item::Node(index) => placement.nodes[index] = frame,
                Item::Cluster(index) => {
                    placement.clusters[index] = frame;
                    shift(graph, Some(index), placement, x, y);
                }
            }
        }
        main += thickness + gaps.get(rank).copied().unwrap_or(0);
    }
    (width, height)
}

fn child(item: Item, w: i32, h: i32) -> Child {
    Child {
        item,
        w,
        h,
        rank: 0,
        cross: 0,
    }
}

/// Moves everything strictly inside `container` by an offset. The
/// container's own frame is its parent's to place, so it stays.
fn shift(graph: &Graph, container: Option<usize>, placement: &mut Placement, dx: i32, dy: i32) {
    let inside = |cluster: Option<usize>| match (cluster, container) {
        (_, None) => true,
        (Some(cluster), Some(container)) => graph.cluster_within(cluster, container),
        (None, Some(_)) => false,
    };
    for (index, node) in graph.nodes.iter().enumerate() {
        if inside(node.cluster) {
            placement.nodes[index].x += dx;
            placement.nodes[index].y += dy;
        }
    }
    for (index, cluster) in graph.clusters.iter().enumerate() {
        if inside(cluster.parent) && Some(index) != container {
            placement.clusters[index].x += dx;
            placement.clusters[index].y += dy;
        }
    }
}

/// Which direct child of `container` holds `node`, if any does.
fn holder(
    graph: &Graph,
    container: Option<usize>,
    children: &[Child],
    node: usize,
) -> Option<usize> {
    let mut cluster = graph.nodes[node].cluster;
    if cluster == container {
        return children.iter().position(|c| c.item == Item::Node(node));
    }
    while let Some(index) = cluster {
        if graph.clusters[index].parent == container {
            return children.iter().position(|c| c.item == Item::Cluster(index));
        }
        cluster = graph.clusters[index].parent;
    }
    None
}

/// The graph's edges as they look from inside `container`: an edge into a
/// nested cluster is an edge to that cluster, and one that never leaves a
/// single child is not this container's business.
fn lifted_links(
    graph: &Graph,
    container: Option<usize>,
    children: &[Child],
) -> Vec<(usize, usize)> {
    let mut links = BTreeSet::new();
    for edge in &graph.edges {
        let from = holder(graph, container, children, edge.from);
        let to = holder(graph, container, children, edge.to);
        if let (Some(from), Some(to)) = (from, to)
            && from != to
        {
            links.insert((from, to));
        }
    }
    links.into_iter().collect()
}

/// Children nothing connects have no order to honour, and a single row of
/// them is as wide as the diagram gets — so they tile instead.
fn rank_as_grid(children: &mut [Child]) {
    let columns = (children.len() as f64).sqrt().ceil() as usize;
    for (index, child) in children.iter_mut().enumerate() {
        child.rank = index / columns.max(1);
    }
}

fn rank_by_longest_path(children: &mut [Child], links: &[(usize, usize)]) {
    let forward = without_cycles(children.len(), links);
    let mut changed = true;
    // Bounded by the child count: a longest path in an acyclic graph
    // visits each node once, so it settles within that many passes.
    for _ in 0..children.len() {
        if !changed {
            break;
        }
        changed = false;
        for &(from, to) in &forward {
            if children[to].rank < children[from].rank + 1 {
                children[to].rank = children[from].rank + 1;
                changed = true;
            }
        }
    }
}

/// The links with every one that closes a cycle turned around, which is
/// what lets a rank exist at all: a back edge is still drawn, only it no
/// longer gets a say in who sits above whom.
fn without_cycles(count: usize, links: &[(usize, usize)]) -> Vec<(usize, usize)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unseen,
        Open,
        Done,
    }
    fn visit(
        node: usize,
        links: &[(usize, usize)],
        marks: &mut [Mark],
        forward: &mut Vec<(usize, usize)>,
    ) {
        marks[node] = Mark::Open;
        for &(from, to) in links.iter().filter(|(from, _)| *from == node) {
            match marks[to] {
                Mark::Open => forward.push((to, from)),
                Mark::Unseen => {
                    forward.push((from, to));
                    visit(to, links, marks, forward);
                }
                Mark::Done => forward.push((from, to)),
            }
        }
        marks[node] = Mark::Done;
    }
    let mut marks = vec![Mark::Unseen; count];
    let mut forward = Vec::new();
    for node in 0..count {
        if marks[node] == Mark::Unseen {
            visit(node, links, &mut marks, &mut forward);
        }
    }
    forward
}

/// Each rank's members in the order that crosses the fewest links:
/// repeatedly sorted by where their neighbours in the rank beside them
/// already sit.
fn ordered_ranks(children: &[Child], links: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let count = children.iter().map(|c| c.rank).max().map_or(0, |r| r + 1);
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, child) in children.iter().enumerate() {
        ranks[child.rank].push(index);
    }
    for sweep in 0..SWEEPS {
        let downward = sweep % 2 == 0;
        let order: Vec<usize> = if downward {
            (1..count).collect()
        } else {
            (0..count.saturating_sub(1)).rev().collect()
        };
        for rank in order {
            let beside = if downward { rank - 1 } else { rank + 1 };
            let position = |member: usize| ranks[beside].iter().position(|&m| m == member);
            let mut keyed: Vec<(f64, usize)> = ranks[rank]
                .iter()
                .enumerate()
                .map(|(current, &member)| {
                    let neighbours: Vec<usize> = links
                        .iter()
                        .filter_map(|&(from, to)| match (from == member, to == member) {
                            (true, _) => position(to),
                            (_, true) => position(from),
                            _ => None,
                        })
                        .collect();
                    let key = if neighbours.is_empty() {
                        current as f64
                    } else {
                        neighbours.iter().sum::<usize>() as f64 / neighbours.len() as f64
                    };
                    (key, member)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            ranks[rank] = keyed.into_iter().map(|(_, member)| member).collect();
        }
    }
    ranks
}

/// The room between one rank and the next. Along a left-to-right flow
/// that room is where a label is written, so it has to hold the longest.
fn rank_gaps(
    graph: &Graph,
    container: Option<usize>,
    children: &[Child],
    links: &[(usize, usize)],
    ranks: usize,
) -> Vec<i32> {
    let base = match graph.flow {
        Flow::TopDown => 5,
        Flow::LeftRight => 8,
    };
    let mut gaps = vec![base; ranks];
    if graph.flow == Flow::TopDown || links.is_empty() {
        return gaps;
    }
    for edge in &graph.edges {
        let (Some(from), Some(to)) = (
            holder(graph, container, children, edge.from),
            holder(graph, container, children, edge.to),
        ) else {
            continue;
        };
        let label = edge.label.as_deref().map_or(0, text_width);
        let upper = children[from].rank.min(children[to].rank);
        gaps[upper] = gaps[upper].max(label + 8);
    }
    gaps
}

/// Positions across the flow: packed in order first, then each rank is
/// pulled toward the ranks beside it until a parent sits over its
/// children rather than over wherever packing left it.
fn spread(children: &mut [Child], ranks: &[Vec<usize>], links: &[(usize, usize)], flow: Flow) {
    let gap = match flow {
        Flow::TopDown => 5,
        Flow::LeftRight => 2,
    };
    for members in ranks {
        let mut cursor = 0;
        for &member in members {
            children[member].cross = cursor;
            cursor += children[member].cross_size(flow) + gap;
        }
    }
    for sweep in 0..SWEEPS {
        let order: Vec<usize> = if sweep % 2 == 0 {
            (0..ranks.len()).collect()
        } else {
            (0..ranks.len()).rev().collect()
        };
        for rank in order {
            let wanted: Vec<i32> = ranks[rank]
                .iter()
                .map(|&member| {
                    let centers: Vec<i32> = links
                        .iter()
                        .filter_map(|&(from, to)| match (from == member, to == member) {
                            (true, _) => Some(to),
                            (_, true) => Some(from),
                            _ => None,
                        })
                        .filter(|&other| children[other].rank != rank)
                        .map(|other| children[other].cross_center(flow))
                        .collect();
                    if centers.is_empty() {
                        children[member].cross
                    } else {
                        centers.iter().sum::<i32>() / centers.len() as i32
                            - children[member].cross_size(flow) / 2
                    }
                })
                .collect();
            settle(children, &ranks[rank], &wanted, gap, flow);
        }
    }
}

/// Puts each member as near where it wants to be as its neighbours
/// allow, keeping the order: pushed apart left to right, then the whole
/// row moved back by however far that pushing carried it on average.
fn settle(children: &mut [Child], members: &[usize], wanted: &[i32], gap: i32, flow: Flow) {
    let mut placed: Vec<i32> = Vec::with_capacity(members.len());
    for (index, &wanted) in wanted.iter().enumerate() {
        let floor = match index {
            0 => i32::MIN,
            _ => placed[index - 1] + children[members[index - 1]].cross_size(flow) + gap,
        };
        placed.push(wanted.max(floor));
    }
    let drift: i32 = placed
        .iter()
        .zip(wanted)
        .map(|(placed, wanted)| placed - wanted)
        .sum::<i32>()
        / members.len().max(1) as i32;
    for (&member, position) in members.iter().zip(placed) {
        children[member].cross = position - drift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architect::{mermaid, model::Diagram};

    fn placed(source: &str) -> (Graph, Placement) {
        let Diagram::Graph(graph) = mermaid::parse(source).unwrap() else {
            panic!("expected a graph");
        };
        let placement = place(&graph);
        (graph, placement)
    }

    #[test]
    fn a_target_sits_below_its_source_in_a_top_down_flow() {
        let (_, placement) = placed("flowchart TD\n a --> b --> c\n a --> c");
        assert!(placement.nodes[1].y > placement.nodes[0].y + placement.nodes[0].h);
        assert!(placement.nodes[2].y > placement.nodes[1].y + placement.nodes[1].h);
    }

    #[test]
    fn a_cycle_still_gets_a_layout() {
        let (_, placement) = placed("flowchart LR\n a --> b --> a");
        assert!(placement.nodes[1].x > placement.nodes[0].x);
    }

    #[test]
    fn no_two_boxes_overlap_and_a_cluster_holds_its_members() {
        let (graph, placement) = placed(
            "flowchart TD\n x --> a\n subgraph one [One]\n a --> b\n subgraph two [Two]\n c\n end\n end\n b --> c --> y",
        );
        for (i, first) in placement.nodes.iter().enumerate() {
            for second in placement.nodes.iter().skip(i + 1) {
                let apart = first.x + first.w <= second.x
                    || second.x + second.w <= first.x
                    || first.y + first.h <= second.y
                    || second.y + second.h <= first.y;
                assert!(apart, "{first:?} overlaps {second:?}");
            }
        }
        for (index, node) in graph.nodes.iter().enumerate() {
            if let Some(cluster) = node.cluster {
                let (frame, inner) = (placement.clusters[cluster], placement.nodes[index]);
                assert!(frame.contains(inner.x, inner.y));
                assert!(frame.contains(inner.x + inner.w - 1, inner.y + inner.h - 1));
            }
        }
    }
}
