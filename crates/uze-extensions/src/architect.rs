//! The architect surface: a project's architecture, drawn in cells.
//!
//! A proof of concept, and what it proves is narrow on purpose — that
//! Mermaid source (flowcharts, C4, sequences) becomes a diagram a
//! terminal can show *without* a graphics protocol, and that owning the
//! layout buys what an image cannot: every box is addressable, so a
//! click selects it and lights what it connects to. The diagrams are
//! static ([`samples`]); where they come from is a later question.
//!
//! It describes and the host draws, like every surface here — but as a
//! [`Layout::Board`], because a drawing is not a document: it is larger
//! than the screen in both directions, it is moved rather than scrolled,
//! and the list of diagrams is how it is switched rather than something
//! to read beside it. What the board shows is a *screen* cut from the
//! whole drawing: this side decides which part, because it owns the
//! position; the host only draws the cells it is handed.

mod canvas;
mod catalog;
mod layout;
mod mermaid;
mod minimap;
mod model;
mod paint;
mod route;
mod samples;
mod sequence;

use crate::{
    registry::BuiltinExtension,
    view::{
        Command, Content, ContentLine, Layout, LineTone, Mode, Navigator, NavigatorRow,
        PanDirection, Role, RowIcon, ScrollDirection, Size, Span, View, ViewHit,
    },
};

use canvas::{Canvas, Frame, Glyphs};
use catalog::Catalog;
use minimap::Minimap;
use model::Diagram;
use paint::Scene;

pub const CATALOG: BuiltinExtension = BuiltinExtension {
    id: "architect",
    name: "Architect",
    description: "A project's architecture as diagrams drawn in the terminal: Mermaid flowcharts, C4 views and sequences, laid out in cells with no graphics protocol.",
    surface: "Workspace TUI",
    usage: "Alt+A opens it over the workspace, as does its chip in the tab strip; drag the board to move it, click a box to light what it connects to.",
};

const PAN_COLUMNS: i32 = 8;
const PAN_ROWS: i32 = 3;
/// The board's grid: a dot every so many cells, faint enough to read
/// through and regular enough that moving the board is visible even
/// where there is nothing drawn.
const GRID: (i32, i32) = (6, 3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Showing {
    Unicode,
    Ascii,
    Source,
}

const MODES: [(Showing, &str); 3] = [
    (Showing::Unicode, "Unicode"),
    (Showing::Ascii, "ASCII"),
    (Showing::Source, "Source"),
];

enum Drawing {
    Graph(Box<Scene>),
    Sequence(model::Sequence),
    Unreadable(String),
}

pub struct ArchitectView {
    catalog: Catalog,
    selected: usize,
    showing: Showing,
    /// The board cell at the screen's top-left corner, once the board has
    /// been moved. `None` is *home* — the drawing in the middle of the
    /// screen — which cannot be a number here, because it depends on a
    /// screen size this side only learns when it is asked to draw.
    corner: Option<(i32, i32)>,
    picked: Option<usize>,
    drawing: Drawing,
    canvas: Option<Canvas>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchitectOutcome {
    Stay,
    Close,
}

impl ArchitectView {
    pub fn opening() -> Self {
        let mut view = Self {
            catalog: Catalog::built_in(),
            selected: 0,
            showing: Showing::Unicode,
            corner: None,
            picked: None,
            drawing: Drawing::Unreadable(String::new()),
            canvas: None,
        };
        view.open(0);
        view
    }

    fn open(&mut self, artifact: usize) {
        let count = self.catalog.artifacts().len().max(1);
        self.selected = artifact % count;
        self.corner = None;
        self.picked = None;
        self.drawing = match self.catalog.get(self.selected) {
            Some(artifact) => match mermaid::parse(&artifact.source) {
                Ok(Diagram::Graph(graph)) => Drawing::Graph(Box::new(Scene::of(graph))),
                Ok(Diagram::Sequence(sequence)) => Drawing::Sequence(sequence),
                Err(reason) => Drawing::Unreadable(reason),
            },
            None => Drawing::Unreadable("there is nothing to draw".to_owned()),
        };
        self.repaint();
    }

    fn repaint(&mut self) {
        let glyphs = match self.showing {
            Showing::Ascii => Glyphs::Ascii,
            Showing::Unicode | Showing::Source => Glyphs::Unicode,
        };
        self.canvas = match &self.drawing {
            Drawing::Graph(scene) => Some(paint::paint(scene, glyphs, self.picked)),
            Drawing::Sequence(sequence) => Some(sequence::paint(sequence, glyphs)),
            Drawing::Unreadable(_) => None,
        };
    }

    fn show(&mut self, showing: Showing) {
        self.showing = showing;
        self.corner = None;
        self.repaint();
    }

    fn source(&self) -> &str {
        self.catalog
            .get(self.selected)
            .map_or("", |artifact| artifact.source.as_str())
    }

    fn board_size(&self) -> (i32, i32) {
        match (self.showing, &self.canvas) {
            (Showing::Source, _) => (0, self.source().lines().count() as i32),
            (_, Some(canvas)) => (canvas.width, canvas.height),
            _ => (0, 0),
        }
    }

    /// How far the corner may go, each way. A board is not a document:
    /// its edge may be brought to the middle of the screen, from either
    /// side, because what is being read is as often at the edge of the
    /// drawing as in it — and a drawing pinned to the screen's border
    /// cannot be looked at the way its middle can. The source *is* a
    /// document, and scrolls like one.
    fn reach(&self, space: Size) -> ((i32, i32), (i32, i32)) {
        let board = self.board_size();
        let screen = (i32::from(space.width), i32::from(space.height));
        match self.showing {
            Showing::Source => ((0, 0), (0, (board.1 - screen.1).max(0))),
            _ => (
                (-screen.0 / 2, board.0 - screen.0 / 2),
                (-screen.1 / 2, board.1 - screen.1 / 2),
            ),
        }
    }

    /// Where the corner is when nothing has moved it: the drawing in the
    /// middle of the screen, or its top-left in view when it is larger.
    fn home(&self, space: Size) -> (i32, i32) {
        let board = self.board_size();
        let screen = (i32::from(space.width), i32::from(space.height));
        match self.showing {
            Showing::Source => (0, 0),
            _ => (
                ((board.0 - screen.0) / 2).min(0),
                ((board.1 - screen.1) / 2).min(0),
            ),
        }
    }

    fn corner(&self, space: Size) -> (i32, i32) {
        let (across, down) = self.reach(space);
        let corner = self.corner.unwrap_or_else(|| self.home(space));
        (
            corner.0.clamp(across.0, across.1.max(across.0)),
            corner.1.clamp(down.0, down.1.max(down.0)),
        )
    }

    fn move_by(&mut self, columns: i32, rows: i32, space: Size) {
        let corner = self.corner(space);
        self.corner = Some((corner.0 + columns, corner.1 + rows));
        self.corner = Some(self.corner(space));
    }

    fn minimap(&self, space: Size) -> Option<Minimap> {
        let screen = (i32::from(space.width), i32::from(space.height));
        match self.showing {
            Showing::Source => None,
            _ => Minimap::of(self.board_size(), screen),
        }
    }

    /// A click, given as the screen cell it landed on. The map answers
    /// first: it is drawn over the board.
    fn click(&mut self, x: i32, y: i32, space: Size) {
        if let Some(target) = self.minimap(space).and_then(|map| map.board_cell_at(x, y)) {
            let half = (i32::from(space.width) / 2, i32::from(space.height) / 2);
            self.corner = Some((target.0 - half.0, target.1 - half.1));
            self.corner = Some(self.corner(space));
            return;
        }
        let Drawing::Graph(scene) = &self.drawing else {
            return;
        };
        let corner = self.corner(space);
        let node = scene.placement.node_at(x + corner.0, y + corner.1);
        // Clicking what is already picked lets go of it, so the diagram
        // can be read whole again without reaching for a key.
        self.picked = if node == self.picked { None } else { node };
        self.repaint();
    }

    fn heading(&self) -> String {
        match &self.drawing {
            Drawing::Graph(scene) => {
                let graph = &scene.graph;
                let mut heading =
                    format!("{} boxes · {} edges", graph.nodes.len(), graph.edges.len());
                if scene.routes.unrouted > 0 {
                    heading.push_str(&format!(" · {} not routed", scene.routes.unrouted));
                }
                if let Some(picked) = self.picked {
                    let into = graph.edges.iter().filter(|edge| edge.to == picked).count();
                    let out = graph
                        .edges
                        .iter()
                        .filter(|edge| edge.from == picked)
                        .count();
                    heading = format!("{} · {into} in · {out} out", graph.nodes[picked].title);
                }
                heading
            }
            Drawing::Sequence(sequence) => format!(
                "{} participants · {} steps",
                sequence.participants.len(),
                sequence.steps.len()
            ),
            Drawing::Unreadable(_) => String::new(),
        }
    }

    /// The part of the board the screen is over, as the screen's own
    /// cells: the drawing, the grid behind it, the map over it.
    fn screen(&self, space: Size) -> Option<Canvas> {
        let board = self.canvas.as_ref()?;
        let (columns, rows) = (i32::from(space.width), i32::from(space.height));
        let corner = self.corner(space);
        let mut screen = Canvas::new(columns, rows, board.glyphs);
        let grid_dot = match board.glyphs {
            Glyphs::Unicode => '·',
            Glyphs::Ascii => '.',
        };
        for y in 0..rows {
            for x in 0..columns {
                let at = (x + corner.0, y + corner.1);
                match board.cell(at.0, at.1) {
                    Some(cell) if cell.solid => screen.set(x, y, cell),
                    _ if at.0.rem_euclid(GRID.0) == 0 && at.1.rem_euclid(GRID.1) == 0 => {
                        screen.put(x, y, grid_dot, Role::Faint, false);
                    }
                    _ => {}
                }
            }
        }
        if let Some(map) = self.minimap(space) {
            let boxes: &[Frame] = match &self.drawing {
                Drawing::Graph(scene) => &scene.placement.nodes,
                _ => &[],
            };
            let looking_at = Frame {
                x: corner.0,
                y: corner.1,
                w: columns,
                h: rows,
            };
            map.paint(&mut screen, boxes, looking_at);
        }
        Some(screen)
    }
}

pub fn view(state: &ArchitectView, space: Size) -> View {
    let mut rows = Vec::new();
    let mut area = None;
    for (index, artifact) in state.catalog.artifacts().iter().enumerate() {
        if area != Some(artifact.kind) {
            area = Some(artifact.kind);
            rows.push(NavigatorRow::Group {
                id: index,
                name: artifact.kind.name().to_owned(),
                depth: 0,
                collapsed: false,
                icon: RowIcon::None,
            });
        }
        rows.push(NavigatorRow::Item {
            id: index,
            name: artifact.name.clone(),
            depth: 1,
            marker: Span::default(),
            selected: index == state.selected,
            icon: RowIcon::None,
        });
    }
    View {
        title: vec![Span::new("Architect", Role::Bright).bold()],
        navigator: Some(Navigator {
            heading: "ARTIFACTS".to_owned(),
            badge: state.catalog.artifacts().len().to_string(),
            focused: false,
            rows,
            anchor: None,
        }),
        content: content(state, space),
        footer: vec![Command::Close, Command::NextView, Command::NextMode],
        modes: MODES
            .iter()
            .map(|&(showing, label)| Mode {
                label: label.to_owned(),
                active: showing == state.showing,
            })
            .collect(),
        layout: Layout::Board,
    }
}

fn content(state: &ArchitectView, space: Size) -> Content {
    if let Drawing::Unreadable(reason) = &state.drawing {
        return Content::Message {
            text: "This artifact could not be drawn".to_owned(),
            hint: Some(reason.clone()),
            role: Role::Warning,
        };
    }
    match state.showing {
        Showing::Source => {
            let lines = source_lines(state.source());
            Content::Lines {
                heading: state.heading(),
                scroll: state.corner(space).1.max(0) as u16,
                total: lines.len(),
                lines,
                caret: None,
            }
        }
        // A board hands over its screen and nothing else: there is no
        // "scrolled past" on a surface that moves both ways, so there is
        // no scroll for the host to apply and no bar for it to draw.
        _ => {
            let lines = state.screen(space).map(|s| s.lines()).unwrap_or_default();
            Content::Lines {
                heading: state.heading(),
                scroll: 0,
                total: lines.len(),
                lines,
                caret: None,
            }
        }
    }
}

fn source_lines(source: &str) -> Vec<ContentLine> {
    source
        .lines()
        .enumerate()
        .map(|(index, line)| ContentLine {
            gutter: " ".to_owned(),
            number: (index + 1).to_string(),
            tone: LineTone::Neutral,
            spans: vec![Span::new(line, Role::Default)],
        })
        .collect()
}

pub fn handle_command(
    state: &mut ArchitectView,
    command: Command,
    space: Size,
) -> ArchitectOutcome {
    let page = i32::from(space.height.saturating_sub(2).max(1));
    let count = state.catalog.artifacts().len().max(1);
    match command {
        Command::Close => return ArchitectOutcome::Close,
        Command::NextView => state.open(state.selected + 1),
        Command::PreviousView => state.open(state.selected + count - 1),
        Command::Pan(PanDirection::Left) => state.move_by(-PAN_COLUMNS, 0, space),
        Command::Pan(PanDirection::Right) => state.move_by(PAN_COLUMNS, 0, space),
        Command::Pan(PanDirection::Up) => state.move_by(0, -PAN_ROWS, space),
        Command::Pan(PanDirection::Down) => state.move_by(0, PAN_ROWS, space),
        Command::ScrollPageDown => state.move_by(0, page, space),
        Command::ScrollPageUp => state.move_by(0, -page, space),
        Command::NextMode => {
            let current = MODES
                .iter()
                .position(|&(showing, _)| showing == state.showing)
                .unwrap_or(0);
            state.show(MODES[(current + 1) % MODES.len()].0);
        }
        _ => {}
    }
    ArchitectOutcome::Stay
}

pub fn handle_mouse(
    state: &mut ArchitectView,
    hit: Option<ViewHit>,
    space: Size,
) -> ArchitectOutcome {
    match hit {
        Some(ViewHit::Close) => return ArchitectOutcome::Close,
        Some(ViewHit::SelectItem(artifact)) => state.open(artifact),
        // An area is entered by its first artifact: the group's id is
        // that artifact's index, so there is nothing to look up.
        Some(ViewHit::ToggleGroup(first)) => state.open(first),
        Some(ViewHit::SelectMode(mode)) => {
            if let Some(&(showing, _)) = MODES.get(mode) {
                state.show(showing);
            }
        }
        Some(ViewHit::PlaceCaret { line, cell }) if state.showing != Showing::Source => {
            state.click(cell as i32, line as i32, space);
        }
        _ => {}
    }
    ArchitectOutcome::Stay
}

/// The board, taken hold of and moved: it follows the pointer, so a drag
/// to the right brings what was off to the left into view — the way a
/// sheet of paper moves, not the way a scrollbar does.
pub fn drag_by(state: &mut ArchitectView, columns: i32, rows: i32, space: Size) {
    state.move_by(-columns, -rows, space);
}

/// A sideways wheel, where the terminal reports one.
pub fn pan(state: &mut ArchitectView, columns: i32, space: Size) {
    state.move_by(columns, 0, space);
}

pub fn handle_scroll(state: &mut ArchitectView, direction: ScrollDirection, space: Size) {
    let rows = match direction {
        ScrollDirection::Up => -PAN_ROWS,
        ScrollDirection::Down => PAN_ROWS,
    };
    state.move_by(0, rows, space);
}

pub fn scroll_to(state: &mut ArchitectView, first: usize, space: Size) {
    let corner = state.corner(space);
    state.corner = Some((corner.0, first as i32));
    state.corner = Some(state.corner(space));
}

#[cfg(test)]
mod tests;
