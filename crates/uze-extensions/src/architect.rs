//! The architect surface: a project's architecture, drawn in cells.
//!
//! A proof of concept, and what it proves is narrow on purpose — that
//! Mermaid source (flowcharts, C4, sequences) becomes a diagram a
//! terminal can show *without* a graphics protocol, and that owning the
//! layout buys what an image cannot: every box is addressable, so a
//! click selects it and lights what it connects to. The diagrams are
//! static ([`samples`]); where they come from is a later question.
//!
//! It describes and the host draws, like every surface here. A diagram
//! is [`Content::Lines`] whose spans happen to be box-drawing, which is
//! why this needed no new vocabulary — and also where the vocabulary
//! shows its one gap: the host wraps a long line, and a wrapped diagram
//! is noise, so this cuts every line to the space itself and pans.

mod canvas;
mod layout;
mod mermaid;
mod model;
mod paint;
mod route;
mod samples;
mod sequence;

use crate::{
    registry::BuiltinExtension,
    view::{
        Command, Content, ContentLine, LineTone, Mode, Navigator, NavigatorRow, Role, RowIcon,
        ScrollDirection, Size, Span, View, ViewHit,
    },
};

use canvas::{Canvas, Glyphs};
use model::Diagram;
use paint::Scene;
use samples::SAMPLES;

pub const CATALOG: BuiltinExtension = BuiltinExtension {
    id: "architect",
    name: "Architect",
    description: "A project's architecture as diagrams drawn in the terminal: Mermaid flowcharts, C4 views and sequences, laid out in cells with no graphics protocol.",
    surface: "Workspace TUI",
    usage: "Alt+A opens it over the workspace; click a box to light what it connects to.",
};

/// The margin the host keeps on each side of unnumbered content. Known
/// here only because a line any longer would wrap; see the module docs.
const READING_INSET: u16 = 2;
const PAN_STEP: i32 = 8;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Navigator,
    Content,
}

enum Drawing {
    Graph(Box<Scene>),
    Sequence(model::Sequence),
    Unreadable(String),
}

pub struct ArchitectView {
    sample: usize,
    showing: Showing,
    focus: Focus,
    scroll: u16,
    pan: i32,
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
            sample: 0,
            showing: Showing::Unicode,
            focus: Focus::Navigator,
            scroll: 0,
            pan: 0,
            picked: None,
            drawing: Drawing::Unreadable(String::new()),
            canvas: None,
        };
        view.open_sample(0);
        view
    }

    fn open_sample(&mut self, sample: usize) {
        self.sample = sample.min(SAMPLES.len() - 1);
        self.scroll = 0;
        self.pan = 0;
        self.picked = None;
        self.drawing = match mermaid::parse(SAMPLES[self.sample].source) {
            Ok(Diagram::Graph(graph)) => Drawing::Graph(Box::new(Scene::of(graph))),
            Ok(Diagram::Sequence(sequence)) => Drawing::Sequence(sequence),
            Err(reason) => Drawing::Unreadable(reason),
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
        self.scroll = 0;
        self.repaint();
    }

    fn total_rows(&self) -> usize {
        match (self.showing, &self.canvas) {
            (Showing::Source, _) => SAMPLES[self.sample].source.lines().count(),
            (_, Some(canvas)) => canvas.height as usize,
            _ => 0,
        }
    }

    fn scroll_by(&mut self, rows: i32) {
        let last = self.total_rows().saturating_sub(1) as i32;
        self.scroll = (i32::from(self.scroll) + rows).clamp(0, last.max(0)) as u16;
    }

    fn pan_by(&mut self, columns: i32, space: Size) {
        let width = self.canvas.as_ref().map_or(0, |canvas| canvas.width);
        let furthest = (width - i32::from(visible_columns(space))).max(0);
        self.pan = (self.pan + columns).clamp(0, furthest);
    }

    fn pick(&mut self, line: usize, cell: usize) {
        let Drawing::Graph(scene) = &self.drawing else {
            return;
        };
        let node = scene.placement.node_at(cell as i32 + self.pan, line as i32);
        // Clicking what is already picked lets go of it, so the diagram
        // can be read whole again without reaching for a key.
        self.picked = if node == self.picked { None } else { node };
        self.repaint();
    }

    fn heading(&self) -> String {
        let sample = &SAMPLES[self.sample];
        match &self.drawing {
            Drawing::Graph(scene) => {
                let graph = &scene.graph;
                let mut heading = format!(
                    "{} · {} boxes · {} edges",
                    sample.name,
                    graph.nodes.len(),
                    graph.edges.len()
                );
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
                    heading.push_str(&format!(
                        " · selected {} · {into} in · {out} out",
                        graph.nodes[picked].title
                    ));
                }
                heading
            }
            Drawing::Sequence(sequence) => format!(
                "{} · {} participants · {} steps",
                sample.name,
                sequence.participants.len(),
                sequence.steps.len()
            ),
            Drawing::Unreadable(_) => sample.name.to_owned(),
        }
    }
}

fn visible_columns(space: Size) -> u16 {
    space.width.saturating_sub(READING_INSET * 2).max(1)
}

pub fn view(state: &ArchitectView, space: Size) -> View {
    let mut rows = Vec::new();
    let mut group = "";
    for (index, sample) in SAMPLES.iter().enumerate() {
        if sample.group != group {
            group = sample.group;
            rows.push(NavigatorRow::Group {
                id: index,
                name: group.to_owned(),
                depth: 0,
                collapsed: false,
                icon: RowIcon::DirectoryOpen,
            });
        }
        rows.push(NavigatorRow::Item {
            id: index,
            name: sample.name.to_owned(),
            depth: 1,
            marker: Span::default(),
            selected: index == state.sample,
            icon: RowIcon::Markup,
        });
    }
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, NavigatorRow::Item { id, .. } if *id == state.sample));
    View {
        title: vec![
            Span::new("Architect", Role::Bright).bold(),
            Span::new("  proof of concept", Role::Dim),
        ],
        navigator: Some(Navigator {
            heading: "DIAGRAMS".to_owned(),
            badge: SAMPLES.len().to_string(),
            focused: state.focus == Focus::Navigator,
            rows,
            anchor: selected_row,
        }),
        content: content(state, space),
        footer: vec![
            Command::Close,
            Command::FocusNext,
            Command::SelectNext,
            Command::Collapse,
            Command::Expand,
            Command::TogglePreview,
            Command::ScrollPageDown,
        ],
        modes: MODES
            .iter()
            .map(|&(showing, label)| Mode {
                label: label.to_owned(),
                active: showing == state.showing,
            })
            .collect(),
    }
}

fn content(state: &ArchitectView, space: Size) -> Content {
    if let Drawing::Unreadable(reason) = &state.drawing {
        return Content::Message {
            text: "This diagram could not be read".to_owned(),
            hint: Some(reason.clone()),
            role: Role::Warning,
        };
    }
    let lines = match (state.showing, &state.canvas) {
        (Showing::Source, _) => source_lines(SAMPLES[state.sample].source),
        (_, Some(canvas)) => canvas.window(
            state.pan,
            i32::from(visible_columns(space)),
            usize::from(state.scroll),
            usize::from(space.height) * 2,
        ),
        _ => Vec::new(),
    };
    Content::Lines {
        heading: state.heading(),
        scroll: state.scroll,
        total: state.total_rows(),
        lines,
        caret: None,
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
    match command {
        Command::Close => return ArchitectOutcome::Close,
        Command::FocusNext => {
            state.focus = match state.focus {
                Focus::Navigator => Focus::Content,
                Focus::Content => Focus::Navigator,
            };
        }
        Command::SelectNext if state.focus == Focus::Content => state.scroll_by(1),
        Command::SelectPrevious if state.focus == Focus::Content => state.scroll_by(-1),
        Command::SelectNext => state.open_sample(state.sample + 1),
        Command::SelectPrevious => state.open_sample(state.sample.saturating_sub(1)),
        Command::Collapse => state.pan_by(-PAN_STEP, space),
        Command::Expand => state.pan_by(PAN_STEP, space),
        Command::ScrollPageDown => state.scroll_by(page),
        Command::ScrollPageUp => state.scroll_by(-page),
        Command::TogglePreview => {
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

pub fn handle_mouse(state: &mut ArchitectView, hit: Option<ViewHit>) -> ArchitectOutcome {
    match hit {
        Some(ViewHit::Close) => return ArchitectOutcome::Close,
        Some(ViewHit::SelectItem(sample)) => {
            state.focus = Focus::Navigator;
            state.open_sample(sample);
        }
        Some(ViewHit::SelectMode(mode)) => {
            if let Some(&(showing, _)) = MODES.get(mode) {
                state.show(showing);
            }
        }
        Some(ViewHit::PlaceCaret { line, cell }) if state.showing != Showing::Source => {
            state.focus = Focus::Content;
            state.pick(line, cell);
        }
        _ => {}
    }
    ArchitectOutcome::Stay
}

pub fn handle_scroll(state: &mut ArchitectView, direction: ScrollDirection) {
    state.scroll_by(match direction {
        ScrollDirection::Up => -3,
        ScrollDirection::Down => 3,
    });
}

pub fn scroll_to(state: &mut ArchitectView, first: usize) {
    state.scroll = first.min(state.total_rows().saturating_sub(1)) as u16;
}

#[cfg(test)]
mod tests;
