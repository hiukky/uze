//! What a diagram *is*, before anything decides where it goes.
//!
//! Every notation this extension reads — a flowchart, a C4 view — lands
//! here, which is what lets one layout and one router serve all of them:
//! C4 is boxes with more text in them and boundaries around them, and
//! nothing downstream needs to know which notation asked.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Flow {
    TopDown,
    LeftRight,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Shape {
    #[default]
    Box,
    Round,
    Database,
    Decision,
    Person,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Stroke {
    #[default]
    Solid,
    Dotted,
    Thick,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Node {
    pub id: String,
    pub title: String,
    /// What kind of thing it is, as the notation names it — C4's
    /// `Container: Rust`. Drawn bracketed under the title.
    pub kind: Option<String>,
    pub description: Option<String>,
    pub shape: Shape,
    /// Outside the system being described, so drawn quieter than it.
    pub external: bool,
    pub cluster: Option<usize>,
    /// Where in the project this is, when the diagram says — Mermaid's
    /// own `$link` on a C4 element, or a flowchart's `click … href`.
    pub link: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub stroke: Stroke,
    pub arrow: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cluster {
    /// What the notation calls it — a C4 boundary's alias. It is what
    /// joins one level to the next: the boundary `core` is the inside of
    /// the box `core` a level up.
    pub id: String,
    pub title: String,
    pub parent: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Graph {
    pub flow: Flow,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
}

impl Graph {
    pub fn new(flow: Flow) -> Self {
        Self {
            flow,
            nodes: Vec::new(),
            edges: Vec::new(),
            clusters: Vec::new(),
        }
    }

    /// The node called `id`, created bare the first time it is named —
    /// both notations let an edge mention a node nothing declared.
    pub fn node_named(&mut self, id: &str, cluster: Option<usize>) -> usize {
        if let Some(index) = self.nodes.iter().position(|node| node.id == id) {
            return index;
        }
        self.nodes.push(Node {
            id: id.to_owned(),
            title: id.to_owned(),
            cluster,
            ..Node::default()
        });
        self.nodes.len() - 1
    }

    /// Whether `cluster` is `ancestor` or sits somewhere inside it.
    pub fn cluster_within(&self, cluster: usize, ancestor: usize) -> bool {
        let mut current = Some(cluster);
        while let Some(index) = current {
            if index == ancestor {
                return true;
            }
            current = self.clusters[index].parent;
        }
        false
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Participant {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceStep {
    Message {
        from: usize,
        to: usize,
        text: String,
        stroke: Stroke,
    },
    Note {
        over: usize,
        text: String,
    },
    /// A `loop`/`alt`/`opt` opening, an `else`, or the `end` that closes
    /// one — drawn as a rule across every lifeline.
    Divider(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Sequence {
    pub participants: Vec<Participant>,
    pub steps: Vec<SequenceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Diagram {
    Graph(Graph),
    Sequence(Sequence),
}
