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
mod sequence;

use std::path::PathBuf;

use crate::{
    Host,
    registry::BuiltinExtension,
    view::{
        Command, Content, ContentLine, Layout, LineTone, Mode, Navigator, NavigatorRow,
        PanDirection, Role, RowIcon, ScrollDirection, Size, Span, View, ViewHit,
    },
};

use canvas::{Canvas, Frame, Glyphs};
use catalog::Catalog;

pub use catalog::Artifact;
use minimap::Minimap;
use model::Diagram;
use paint::{Leads, Scene};

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
    /// Why there is nothing on the board, while there is nothing: the
    /// read still in flight, or what it found instead of artifacts.
    nothing: Option<(String, Option<String>)>,
    /// The area highlighted in the list of areas, while it is open — by
    /// the index of its first artifact, which is how an area is named
    /// everywhere here.
    choosing: Option<usize>,
    /// The levels entered to reach what is on show: each the artifact
    /// that was left and the box it was left through, so coming back can
    /// put the viewer where they were standing.
    trail: Vec<(usize, String)>,
    /// The boundaries each artifact draws the inside of, by alias. What
    /// joins one level to the next is only this: the boundary `core` is
    /// the inside of the box `core`, wherever that box is drawn.
    insides: Vec<Vec<String>>,
    project: PathBuf,
}

/// Where the host found the project's artifacts to be declared. The
/// host's to say, because only it may read the project's manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactSource {
    /// The project declares none.
    Undeclared,
    /// The declared directory, how the project itself spells it, and the
    /// project it was declared in.
    Directory {
        path: PathBuf,
        declared: String,
        project: PathBuf,
    },
    /// Declared, and not something the host will follow.
    Refused(String),
}

/// What reading a source produced — everything the surface needs to stop
/// saying "reading".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactsAnswer {
    Found {
        artifacts: Vec<Artifact>,
        /// What a box's link is relative to.
        project: PathBuf,
    },
    /// Nothing to draw, in two levels: what is the matter, and what to do.
    Nothing { text: String, hint: String },
}

/// Reads a source. Unbounded — a directory walk and a read per file — so
/// the host runs it off the thread that draws and hands the answer to
/// [`ArchitectView::absorb`].
pub fn read_artifacts(host: &dyn Host, source: ArtifactSource) -> ArtifactsAnswer {
    let nothing = |text: String, hint: &str| ArtifactsAnswer::Nothing {
        text,
        hint: hint.to_owned(),
    };
    match source {
        ArtifactSource::Undeclared => nothing(
            "This project declares no artifacts yet".to_owned(),
            "Add `artifacts:` with a `path:` to agents.yaml, and keep Mermaid files (.mmd) \
             in that directory — C4 views, sequences and flowcharts are drawn here.",
        ),
        ArtifactSource::Refused(reason) => nothing(
            reason,
            "Fix `artifacts:` in agents.yaml and open this again.",
        ),
        ArtifactSource::Directory {
            path,
            declared,
            project,
        } => match catalog::read(host, &path) {
            Ok(artifacts) if artifacts.is_empty() => nothing(
                format!("`{declared}` holds no Mermaid files yet"),
                "Add a .mmd file there: a diagram that starts with `C4Context`, \
                 `sequenceDiagram` or `flowchart` is drawn here.",
            ),
            Ok(artifacts) => ArtifactsAnswer::Found { artifacts, project },
            Err(reason) => nothing(
                format!("`{declared}` could not be read"),
                &format!("{reason}. It is the `artifacts.path` agents.yaml declares."),
            ),
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchitectOutcome {
    Stay,
    Close,
    /// A box that stands for code was followed: the host shows `target`,
    /// which is where the board ends and the checkout begins. `project`
    /// is what the diagram wrote the path relative to.
    OpenPath {
        project: PathBuf,
        target: PathBuf,
    },
}

impl ArchitectView {
    /// The surface before its artifacts have been read.
    pub fn opening() -> Self {
        Self {
            catalog: Catalog::default(),
            selected: 0,
            showing: Showing::Unicode,
            corner: None,
            picked: None,
            drawing: Drawing::Unreadable(String::new()),
            canvas: None,
            nothing: Some(("Reading the project's artifacts".to_owned(), None)),
            choosing: None,
            trail: Vec::new(),
            insides: Vec::new(),
            project: PathBuf::new(),
        }
    }

    pub fn absorb(&mut self, answer: ArtifactsAnswer) {
        match answer {
            ArtifactsAnswer::Found { artifacts, project } => {
                self.catalog = Catalog::of(artifacts);
                self.insides = self
                    .catalog
                    .artifacts()
                    .iter()
                    // Only a C4 view is the inside of something: levels are
                    // that model's idea, and a flowchart's subgraph sharing
                    // a name with a container is a coincidence, not a zoom.
                    .map(|artifact| match mermaid::parse(artifact.diagram()) {
                        Ok(Diagram::Graph(graph)) if artifact.kind == catalog::Kind::C4 => graph
                            .clusters
                            .into_iter()
                            .map(|cluster| cluster.id)
                            .filter(|id| !id.is_empty())
                            .collect(),
                        _ => Vec::new(),
                    })
                    .collect();
                self.project = project;
                self.nothing = None;
                self.open(0);
            }
            ArtifactsAnswer::Nothing { text, hint } => {
                self.catalog = Catalog::default();
                self.nothing = Some((text, Some(hint)));
            }
        }
    }

    /// Every area, as the index of its first artifact.
    fn areas(&self) -> Vec<usize> {
        let artifacts = self.catalog.artifacts();
        (0..artifacts.len())
            .filter(|&index| index == 0 || artifacts[index - 1].kind != artifacts[index].kind)
            .collect()
    }

    /// The area the artifact on show belongs to.
    fn area(&self) -> Option<usize> {
        self.areas()
            .into_iter()
            .take_while(|&first| first <= self.selected)
            .last()
    }

    /// Moves the highlight in the open list of areas, round and round.
    fn highlight(&mut self, step: isize) {
        let areas = self.areas();
        let Some(at) = self
            .choosing
            .and_then(|first| areas.iter().position(|&area| area == first))
        else {
            return;
        };
        let next = (at as isize + step).rem_euclid(areas.len() as isize) as usize;
        self.choosing = Some(areas[next]);
    }

    /// Shows an artifact chosen from the menu: a fresh start, so whatever
    /// was entered to reach the last one is no longer the way here.
    fn open(&mut self, artifact: usize) {
        self.trail.clear();
        self.show_artifact(artifact);
    }

    fn show_artifact(&mut self, artifact: usize) {
        self.choosing = None;
        let count = self.catalog.artifacts().len().max(1);
        self.selected = artifact % count;
        self.corner = None;
        self.picked = None;
        self.drawing = match self.catalog.get(self.selected) {
            Some(artifact) => match mermaid::parse(artifact.diagram()) {
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
            Drawing::Graph(scene) => {
                let leads: Vec<Leads> = (0..scene.graph.nodes.len())
                    .map(|node| self.leads(scene, node))
                    .collect();
                Some(paint::paint(scene, glyphs, self.picked, &leads))
            }
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
    fn click(&mut self, x: i32, y: i32, space: Size) -> ArchitectOutcome {
        if let Some(target) = self.minimap(space).and_then(|map| map.board_cell_at(x, y)) {
            let half = (i32::from(space.width) / 2, i32::from(space.height) / 2);
            self.corner = Some((target.0 - half.0, target.1 - half.1));
            self.corner = Some(self.corner(space));
            return ArchitectOutcome::Stay;
        }
        let Drawing::Graph(scene) = &self.drawing else {
            return ArchitectOutcome::Stay;
        };
        let corner = self.corner(space);
        let at = (x + corner.0, y + corner.1);
        let node = scene.placement.node_at(at.0, at.1);
        // A box that leads somewhere is followed by a click on its mark,
        // or by a second click on the box — which is what a double click
        // is. One that leads nowhere is let go of by that second click,
        // so the diagram can be read whole again without a key.
        let follows = node.is_some_and(|node| {
            let leads = self.leads(scene, node) != Leads::Nowhere;
            let on_mark = paint::mark_cells(scene.placement.nodes[node]).contains(at.0, at.1);
            leads && (on_mark || self.picked == Some(node))
        });
        if follows {
            self.picked = node;
            return self.enter();
        }
        self.picked = if node == self.picked { None } else { node };
        self.repaint();
        ArchitectOutcome::Stay
    }

    fn leads(&self, scene: &Scene, node: usize) -> Leads {
        let node = &scene.graph.nodes[node];
        if self.inside_of(&node.id).is_some() {
            Leads::Inside
        } else if node.link.is_some() {
            Leads::ToCode
        } else {
            Leads::Nowhere
        }
    }

    /// The artifact that draws the inside of the box `alias` names.
    fn inside_of(&self, alias: &str) -> Option<usize> {
        self.insides
            .iter()
            .enumerate()
            .find(|(artifact, insides)| {
                *artifact != self.selected && insides.iter().any(|inside| inside == alias)
            })
            .map(|(artifact, _)| artifact)
    }

    /// Follows the picked box: into the level below it, or out to the
    /// code it names.
    fn enter(&mut self) -> ArchitectOutcome {
        let (Drawing::Graph(scene), Some(picked)) = (&self.drawing, self.picked) else {
            return ArchitectOutcome::Stay;
        };
        let node = &scene.graph.nodes[picked];
        if let Some(below) = self.inside_of(&node.id) {
            self.trail.push((self.selected, node.id.clone()));
            self.show_artifact(below);
            return ArchitectOutcome::Stay;
        }
        match &node.link {
            Some(link) => ArchitectOutcome::OpenPath {
                project: self.project.clone(),
                target: self.project.join(link),
            },
            None => ArchitectOutcome::Stay,
        }
    }

    /// Back to the level `depth` steps in, with the box that was entered
    /// picked and in the middle of the screen — where the viewer stood.
    fn back_to(&mut self, depth: usize, space: Size) {
        let Some((artifact, through)) = self.trail.get(depth).cloned() else {
            return;
        };
        self.trail.truncate(depth);
        self.show_artifact(artifact);
        let Drawing::Graph(scene) = &self.drawing else {
            return;
        };
        if let Some(node) = scene.graph.nodes.iter().position(|node| node.id == through) {
            self.picked = Some(node);
            self.bring_into_view(node, space, true);
            self.repaint();
        }
    }

    fn bring_into_view(&mut self, node: usize, space: Size, always: bool) {
        let Drawing::Graph(scene) = &self.drawing else {
            return;
        };
        let frame = scene.placement.nodes[node];
        let corner = self.corner(space);
        let screen = (i32::from(space.width), i32::from(space.height));
        let visible = frame.x >= corner.0
            && frame.y >= corner.1
            && frame.x + frame.w <= corner.0 + screen.0
            && frame.y + frame.h <= corner.1 + screen.1;
        if always || !visible {
            let centre = frame.center();
            self.corner = Some((centre.0 - screen.0 / 2, centre.1 - screen.1 / 2));
            self.corner = Some(self.corner(space));
        }
    }

    /// Picks the box that lies `direction` of the picked one — or, with
    /// nothing picked, the one nearest the middle of the screen. A row of
    /// cells is twice as tall as a column is wide, so a step down counts
    /// double: "nearest" has to mean what it looks like.
    fn pick_toward(&mut self, direction: PanDirection, space: Size) {
        let Drawing::Graph(scene) = &self.drawing else {
            return;
        };
        let corner = self.corner(space);
        let from = match self.picked {
            Some(picked) => scene.placement.nodes[picked].center(),
            None => (
                corner.0 + i32::from(space.width) / 2,
                corner.1 + i32::from(space.height) / 2,
            ),
        };
        let nearest = scene
            .placement
            .nodes
            .iter()
            .enumerate()
            .filter(|(node, _)| Some(*node) != self.picked)
            .filter_map(|(node, frame)| {
                let centre = frame.center();
                let (dx, dy) = (centre.0 - from.0, (centre.1 - from.1) * 2);
                let (along, across) = match direction {
                    PanDirection::Left => (-dx, dy),
                    PanDirection::Right => (dx, dy),
                    PanDirection::Up => (-dy, dx),
                    PanDirection::Down => (dy, dx),
                };
                let ahead = self.picked.is_none() || along > 0;
                ahead.then(|| (along.abs() + across.abs() * 2, node))
            })
            .min()
            .map(|(_, node)| node);
        if let Some(node) = nearest {
            self.picked = Some(node);
            self.bring_into_view(node, space, false);
            self.repaint();
        }
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

    /// What the content says about itself, and which file it came from —
    /// the second half is what somebody needs to go and change it.
    fn caption(&self) -> String {
        let origin = self.catalog.get(self.selected).map(|a| a.origin.as_str());
        if let (Drawing::Graph(scene), Some(picked)) = (&self.drawing, self.picked) {
            let said = match self.leads(scene, picked) {
                Leads::Inside => Some("enter goes inside".to_owned()),
                Leads::ToCode => scene.graph.nodes[picked]
                    .link
                    .as_ref()
                    .map(|link| format!("enter opens {link}")),
                Leads::Nowhere => None,
            };
            if let Some(said) = said {
                return format!("{} · {said}", self.heading());
            }
        }
        match (self.heading(), origin) {
            (heading, Some(origin)) if heading.is_empty() => origin.to_owned(),
            (heading, Some(origin)) => format!("{heading} · {origin}"),
            (heading, None) => heading,
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
            choosing: state.choosing,
        }),
        content: content(state, space),
        footer: vec![
            Command::Close,
            Command::ChooseGroup,
            Command::NextView,
            Command::NextMode,
        ],
        modes: MODES
            .iter()
            .map(|&(showing, label)| Mode {
                label: label.to_owned(),
                active: showing == state.showing,
            })
            .collect(),
        layout: Layout::Board,
        trail: match state.trail.is_empty() {
            true => Vec::new(),
            false => state
                .trail
                .iter()
                .map(|(artifact, _)| *artifact)
                .chain(std::iter::once(state.selected))
                .filter_map(|artifact| state.catalog.get(artifact))
                .map(|artifact| artifact.name.clone())
                .collect(),
        },
    }
}

fn content(state: &ArchitectView, space: Size) -> Content {
    if let Some((text, hint)) = &state.nothing {
        return Content::Message {
            text: text.clone(),
            hint: hint.clone(),
            role: Role::Muted,
        };
    }
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
                heading: state.caption(),
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
                heading: state.caption(),
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
    // The open list of areas takes the keys that mean something in a
    // list, and the one that leaves: a list open over the board is what
    // is being talked to, and the board under it waits.
    if state.choosing.is_some() {
        match command {
            Command::Pan(PanDirection::Up) => state.highlight(-1),
            Command::Pan(PanDirection::Down) => state.highlight(1),
            Command::Activate => {
                if let Some(first) = state.choosing {
                    state.open(first);
                }
            }
            Command::Close | Command::ChooseGroup => state.choosing = None,
            _ => {}
        }
        return ArchitectOutcome::Stay;
    }
    match command {
        Command::Close => return ArchitectOutcome::Close,
        Command::ChooseGroup => state.choosing = state.area(),
        Command::SelectToward(direction) => state.pick_toward(direction, space),
        Command::Activate => return state.enter(),
        Command::Back => state.back_to(state.trail.len().saturating_sub(1), space),
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
    // A click anywhere but on the list shuts it, and is spent doing so —
    // the same rule every menu over this client follows.
    let list_was_open = state.choosing.take().is_some();
    match hit {
        Some(ViewHit::Close) => return ArchitectOutcome::Close,
        Some(ViewHit::ChooseGroup) if !list_was_open => state.choosing = state.area(),
        Some(ViewHit::ToggleGroup(first)) => state.open(first),
        _ if list_was_open => {}
        Some(ViewHit::SelectItem(artifact)) => state.open(artifact),
        Some(ViewHit::SelectMode(mode)) => {
            if let Some(&(showing, _)) = MODES.get(mode) {
                state.show(showing);
            }
        }
        Some(ViewHit::PlaceCaret { line, cell }) if state.showing != Showing::Source => {
            return state.click(cell as i32, line as i32, space);
        }
        Some(ViewHit::SelectTrail(depth)) => state.back_to(depth, space),
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
