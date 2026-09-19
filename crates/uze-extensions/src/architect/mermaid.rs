//! Mermaid source, read into a [`Diagram`].
//!
//! The part of the language an architecture document actually uses:
//! flowcharts with subgraphs, the C4 views, and sequence diagrams. Styling
//! statements are skipped rather than refused — what a node looks like is
//! the host's palette to decide, and a diagram that sets colours should
//! still be read for what it says.
//!
//! Written here rather than depended on: Mermaid's own parser is
//! JavaScript bound to a browser DOM, and nothing published for Rust
//! clears this workspace's bar for a new dependency.

use super::model::{
    Cluster, Diagram, Edge, Flow, Graph, Participant, Sequence, SequenceStep, Shape, Stroke,
};

pub fn parse(source: &str) -> Result<Diagram, String> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines.next().ok_or("the diagram is empty")?;
    let keyword = header.split_whitespace().next().unwrap_or_default();
    match keyword {
        "flowchart" | "graph" => Ok(Diagram::Graph(flowchart(header, lines)?)),
        "sequenceDiagram" => Ok(Diagram::Sequence(sequence(lines))),
        _ if keyword.starts_with("C4") => Ok(Diagram::Graph(c4(lines)?)),
        other => Err(format!("`{other}` diagrams are not drawn yet")),
    }
}

fn flowchart<'a>(header: &str, lines: impl Iterator<Item = &'a str>) -> Result<Graph, String> {
    let flow = match header.split_whitespace().nth(1) {
        Some("LR" | "RL") => Flow::LeftRight,
        _ => Flow::TopDown,
    };
    let mut graph = Graph::new(flow);
    let mut open: Vec<usize> = Vec::new();
    for line in lines {
        let keyword = line.split_whitespace().next().unwrap_or_default();
        match keyword {
            "subgraph" => {
                let declared = line["subgraph".len()..].trim();
                graph.clusters.push(Cluster {
                    id: declared
                        .split(|c: char| c == '[' || c.is_whitespace())
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    title: subgraph_title(declared),
                    parent: open.last().copied(),
                });
                open.push(graph.clusters.len() - 1);
            }
            "end" => {
                open.pop();
            }
            "click" => link_clicked(&mut graph, line),
            "classDef" | "class" | "style" | "linkStyle" | "direction" => {}
            _ => Statement::new(line).read_into(&mut graph, open.last().copied())?,
        }
    }
    Ok(graph)
}

/// `click id href "path"`, or the older `click id "path"`: the one
/// flowchart statement that says where a node leads.
fn link_clicked(graph: &mut Graph, line: &str) {
    let mut words = line.split_whitespace().skip(1);
    let Some(id) = words.next() else {
        return;
    };
    let target = line
        .split_once('"')
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(target, _)| target.trim());
    if let (Some(target), Some(node)) = (
        target.filter(|target| !target.is_empty()),
        graph.nodes.iter_mut().find(|node| node.id == id),
    ) {
        node.link = Some(target.to_owned());
    }
}

fn subgraph_title(rest: &str) -> String {
    let title = match rest.split_once('[') {
        Some((_, bracketed)) => bracketed.trim_end_matches(']'),
        None => rest,
    };
    unquoted(title)
}

fn unquoted(text: &str) -> String {
    text.trim().trim_matches('"').trim().to_owned()
}

fn with_breaks(text: &str) -> String {
    text.replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
}

/// One flowchart statement: node groups joined by links, `A & B --> C`.
struct Statement {
    chars: Vec<char>,
    at: usize,
}

struct Link {
    stroke: Stroke,
    arrow: bool,
    label: Option<String>,
}

const SHAPES: [(&str, &str, Shape); 8] = [
    ("([", "])", Shape::Round),
    ("[(", ")]", Shape::Database),
    ("((", "))", Shape::Round),
    ("{{", "}}", Shape::Decision),
    ("[", "]", Shape::Box),
    ("(", ")", Shape::Round),
    ("{", "}", Shape::Decision),
    (">", "]", Shape::Box),
];

impl Statement {
    fn new(line: &str) -> Self {
        Self {
            chars: line.trim_end_matches(';').chars().collect(),
            at: 0,
        }
    }

    fn read_into(mut self, graph: &mut Graph, cluster: Option<usize>) -> Result<(), String> {
        let mut sources = self.group(graph, cluster)?;
        loop {
            self.skip_spaces();
            if self.at >= self.chars.len() {
                return Ok(());
            }
            let link = self.link()?;
            let targets = self.group(graph, cluster)?;
            for &from in &sources {
                for &to in &targets {
                    graph.edges.push(Edge {
                        from,
                        to,
                        label: link.label.clone(),
                        stroke: link.stroke,
                        arrow: link.arrow,
                    });
                }
            }
            sources = targets;
        }
    }

    fn group(&mut self, graph: &mut Graph, cluster: Option<usize>) -> Result<Vec<usize>, String> {
        let mut nodes = vec![self.node(graph, cluster)?];
        loop {
            self.skip_spaces();
            if self.peek() != Some('&') {
                return Ok(nodes);
            }
            self.at += 1;
            nodes.push(self.node(graph, cluster)?);
        }
    }

    fn node(&mut self, graph: &mut Graph, cluster: Option<usize>) -> Result<usize, String> {
        self.skip_spaces();
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            self.at += 1;
        }
        if start == self.at {
            return Err(format!("expected a node at `{}`", self.rest()));
        }
        let id: String = self.chars[start..self.at].iter().collect();
        let index = graph.node_named(&id, cluster);
        for (opening, closing, shape) in SHAPES {
            if !self.rest().starts_with(opening) {
                continue;
            }
            self.at += opening.chars().count();
            let text = self.until(closing)?;
            let text = with_breaks(&unquoted(&text));
            let (title, description) = match text.split_once('\n') {
                Some((title, rest)) => (title.to_owned(), Some(rest.replace('\n', " "))),
                None => (text, None),
            };
            let node = &mut graph.nodes[index];
            node.title = title;
            node.description = description;
            node.shape = shape;
            // Declaring a node inside a subgraph is what places it there,
            // wherever an earlier edge first mentioned it.
            node.cluster = cluster.or(node.cluster);
            break;
        }
        Ok(index)
    }

    fn link(&mut self) -> Result<Link, String> {
        let opening = self.run_of("-=.<>");
        if opening.is_empty() {
            return Err(format!("expected a link at `{}`", self.rest()));
        }
        let mut run = opening.clone();
        let mut label = None;
        if opening.chars().count() <= 2 {
            let start = self.at;
            while self.at < self.chars.len() && !self.closes_inline_label() {
                self.at += 1;
            }
            label = Some(unquoted(
                &self.chars[start..self.at].iter().collect::<String>(),
            ));
            run.push_str(&self.run_of("-=.<>"));
        }
        self.skip_spaces();
        if self.peek() == Some('|') {
            self.at += 1;
            label = Some(unquoted(&self.until("|")?));
        }
        Ok(Link {
            stroke: if run.contains('=') {
                Stroke::Thick
            } else if run.contains('.') {
                Stroke::Dotted
            } else {
                Stroke::Solid
            },
            arrow: run.ends_with('>'),
            label: label.filter(|text| !text.is_empty()),
        })
    }

    fn closes_inline_label(&self) -> bool {
        let here = self.chars[self.at];
        let next = self.chars.get(self.at + 1).copied().unwrap_or(' ');
        "-=.".contains(here) && "-=.>".contains(next)
    }

    fn run_of(&mut self, allowed: &str) -> String {
        let start = self.at;
        while self.peek().is_some_and(|c| allowed.contains(c)) {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }

    fn until(&mut self, closing: &str) -> Result<String, String> {
        let rest = self.rest();
        let end = rest
            .find(closing)
            .ok_or_else(|| format!("`{closing}` never closes in `{rest}`"))?;
        let text = rest[..end].to_owned();
        self.at += rest[..end].chars().count() + closing.chars().count();
        Ok(text)
    }

    fn rest(&self) -> String {
        self.chars[self.at.min(self.chars.len())..].iter().collect()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }
}

fn c4<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Graph, String> {
    let mut graph = Graph::new(Flow::TopDown);
    let mut open: Vec<usize> = Vec::new();
    for line in lines {
        if line.starts_with('}') {
            open.pop();
            continue;
        }
        let Some((name, rest)) = line.split_once('(') else {
            continue;
        };
        let name = name.trim();
        let inside = rest.rsplit_once(')').map_or(rest, |(inside, _)| inside);
        let link = named_argument(inside, "$link");
        let mut arguments = arguments(inside);
        // A dynamic view numbers its relations: `RelIndex(1, a, b, …)`.
        // The number is part of what the edge says, not one of its ends.
        let step = name
            .starts_with("RelIndex")
            .then(|| (!arguments.is_empty()).then(|| arguments.remove(0)))
            .flatten();
        let argument = |index: usize| arguments.get(index).cloned().filter(|a| !a.is_empty());
        // A deployment node holds things the way a boundary does, and is
        // drawn the same: what it is goes in the brackets.
        let is_node = name == "Node" || name.starts_with("Node_") || name == "Deployment_Node";
        if name.ends_with("Boundary") || is_node {
            let kind = argument(2).or_else(|| {
                name.strip_suffix("_Boundary")
                    .map(|prefix| prefix.replace('_', " "))
            });
            let label = argument(1).or_else(|| argument(0)).unwrap_or_default();
            graph.clusters.push(Cluster {
                id: argument(0).unwrap_or_default(),
                title: match kind {
                    Some(kind) => format!("{label} [{kind}]"),
                    None => label,
                },
                parent: open.last().copied(),
            });
            open.push(graph.clusters.len() - 1);
        } else if name.starts_with("Rel") || name.starts_with("BiRel") {
            let (Some(from), Some(to)) = (argument(0), argument(1)) else {
                return Err(format!("`{line}` names no two ends"));
            };
            let from = graph.node_named(&from, None);
            let to = graph.node_named(&to, None);
            graph.edges.push(Edge {
                from,
                to,
                label: match (step, argument(2), argument(3)) {
                    (Some(step), Some(label), _) => Some(format!("{step}. {label}")),
                    (None, Some(label), Some(technology)) => {
                        Some(format!("{label} [{technology}]"))
                    }
                    (_, label, _) => label,
                },
                stroke: Stroke::Solid,
                arrow: true,
            });
        } else if let Some(element) = C4Element::named(name) {
            let Some(alias) = argument(0) else {
                return Err(format!("`{line}` has no alias"));
            };
            let index = graph.node_named(&alias, open.last().copied());
            let technology = element.has_technology.then(|| argument(2)).flatten();
            let description = argument(if element.has_technology { 3 } else { 2 });
            let node = &mut graph.nodes[index];
            node.title = argument(1).unwrap_or(alias);
            node.kind = Some(match technology {
                Some(technology) => format!("{}: {technology}", element.kind),
                None => element.kind.to_owned(),
            });
            node.description = description;
            node.shape = element.shape;
            node.external = name.ends_with("_Ext");
            node.cluster = open.last().copied();
            node.link = link;
        }
    }
    Ok(graph)
}

struct C4Element {
    kind: &'static str,
    shape: Shape,
    has_technology: bool,
}

impl C4Element {
    fn named(name: &str) -> Option<Self> {
        let base = name.strip_suffix("_Ext").unwrap_or(name);
        let (kind, has_technology, stem) = if base.starts_with("Person") {
            ("Person", false, "Person")
        } else if base.starts_with("System") {
            ("Software System", false, "System")
        } else if base.starts_with("Container") {
            ("Container", true, "Container")
        } else if base.starts_with("Component") {
            ("Component", true, "Component")
        } else {
            return None;
        };
        let shape = match &base[stem.len()..] {
            "" if kind == "Person" => Shape::Person,
            "" => Shape::Box,
            "Db" => Shape::Database,
            "Queue" => Shape::Round,
            _ => return None,
        };
        Some(Self {
            kind,
            shape,
            has_technology,
        })
    }
}

/// One `$key="value"` argument, which the positional ones leave out.
fn named_argument(inside: &str, key: &str) -> Option<String> {
    let (_, rest) = inside.split_once(key)?;
    let value = rest.trim_start().strip_prefix('=')?.trim_start();
    let value = match value.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').map_or(quoted, |(value, _)| value),
        None => value.split(',').next().unwrap_or_default(),
    };
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// A macro's arguments: comma-separated, quotes optional, and a comma
/// inside quotes is text. `$key=value` arguments are styling, so skipped.
fn arguments(inside: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in inside.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => arguments.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    arguments.push(current);
    arguments
        .into_iter()
        .map(|argument| argument.trim().to_owned())
        .filter(|argument| !argument.starts_with('$'))
        .collect()
}

fn sequence<'a>(lines: impl Iterator<Item = &'a str>) -> Sequence {
    let mut sequence = Sequence::default();
    for line in lines {
        let (keyword, rest) = line.split_once(' ').unwrap_or((line, ""));
        match keyword {
            "participant" | "actor" => {
                let (id, title) = rest.split_once(" as ").unwrap_or((rest, rest));
                let index = participant(&mut sequence, id.trim());
                sequence.participants[index].title = title.trim().to_owned();
            }
            "Note" | "note" => {
                if let Some((place, text)) = rest.split_once(':') {
                    let first = place
                        .trim_start_matches("over")
                        .trim_start_matches("right of")
                        .trim_start_matches("left of")
                        .split(',')
                        .next()
                        .unwrap_or_default()
                        .trim();
                    let over = participant(&mut sequence, first);
                    sequence.steps.push(SequenceStep::Note {
                        over,
                        text: with_breaks(text.trim()).replace('\n', " "),
                    });
                }
            }
            "loop" | "alt" | "opt" | "par" | "else" | "and" | "critical" | "break" => {
                sequence
                    .steps
                    .push(SequenceStep::Divider(line.trim().to_owned()));
            }
            "end" => sequence.steps.push(SequenceStep::Divider("end".to_owned())),
            _ => message(&mut sequence, line),
        }
    }
    sequence
}

fn message(sequence: &mut Sequence, line: &str) {
    let Some((ends, text)) = line.split_once(':') else {
        return;
    };
    let Some(arrow_at) = ends.find('-') else {
        return;
    };
    let from = ends[..arrow_at].trim();
    let arrow_len = ends[arrow_at..]
        .find(|c: char| !"->x)".contains(c))
        .unwrap_or(ends.len() - arrow_at);
    let arrow = &ends[arrow_at..arrow_at + arrow_len];
    let to = ends[arrow_at + arrow_len..].trim_matches(|c: char| c == '+' || c == '-' || c == ' ');
    if from.is_empty() || to.is_empty() {
        return;
    }
    let from = participant(sequence, from);
    let to = participant(sequence, to);
    sequence.steps.push(SequenceStep::Message {
        from,
        to,
        text: text.trim().to_owned(),
        stroke: if arrow.starts_with("--") {
            Stroke::Dotted
        } else {
            Stroke::Solid
        },
    });
}

fn participant(sequence: &mut Sequence, id: &str) -> usize {
    if let Some(index) = sequence.participants.iter().position(|p| p.id == id) {
        return index;
    }
    sequence.participants.push(Participant {
        id: id.to_owned(),
        title: id.to_owned(),
    });
    sequence.participants.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(source: &str) -> Graph {
        match parse(source).expect("the diagram parses") {
            Diagram::Graph(graph) => graph,
            Diagram::Sequence(_) => panic!("expected a graph"),
        }
    }

    #[test]
    fn a_chain_with_a_fan_out_becomes_one_edge_per_pair() {
        let graph = graph("flowchart TD\n  a[Alpha] --> b(Beta) & c[(Gamma)] -.->|reads| d");
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.edges.len(), 4);
        assert_eq!(graph.nodes[2].shape, Shape::Database);
        assert_eq!(graph.edges[3].label.as_deref(), Some("reads"));
        assert_eq!(graph.edges[3].stroke, Stroke::Dotted);
    }

    #[test]
    fn an_inline_label_is_the_same_label_as_a_piped_one() {
        let graph = graph("graph LR\n  a -- ships to --> b\n  a == must ==> c");
        assert_eq!(graph.flow, Flow::LeftRight);
        assert_eq!(graph.edges[0].label.as_deref(), Some("ships to"));
        assert_eq!(graph.edges[1].stroke, Stroke::Thick);
    }

    #[test]
    fn a_node_declared_in_a_subgraph_lives_there() {
        let graph =
            graph("flowchart TD\n  x --> a\n  subgraph core [The core]\n    a[Alpha]\n  end");
        assert_eq!(graph.clusters[0].title, "The core");
        assert_eq!(graph.nodes[1].cluster, Some(0));
        assert_eq!(graph.nodes[0].cluster, None);
    }

    #[test]
    fn c4_elements_carry_their_kind_and_their_boundary() {
        let graph = graph(
            "C4Container\n  Person(dev, \"Developer\", \"Writes, reviews\")\n  \
             System_Boundary(uze, \"uze\") {\n    \
             ContainerDb(store, \"Store\", \"Filesystem\", \"Package bytes\")\n  }\n  \
             System_Ext(git, \"Git\")\n  Rel(dev, store, \"Installs\", \"CLI\")",
        );
        assert_eq!(
            graph.nodes[0].description.as_deref(),
            Some("Writes, reviews")
        );
        assert_eq!(
            graph.nodes[1].kind.as_deref(),
            Some("Container: Filesystem")
        );
        assert_eq!(graph.nodes[1].cluster, Some(0));
        assert!(graph.nodes[2].external);
        assert_eq!(graph.clusters[0].title, "uze [System]");
        assert_eq!(graph.clusters[0].id, "uze");
        assert_eq!(graph.edges[0].label.as_deref(), Some("Installs [CLI]"));
    }

    #[test]
    fn a_dynamic_view_numbers_its_relations_without_losing_their_ends() {
        let graph = graph(
            "C4Dynamic\n  Container(a, \"A\")\n  Container(b, \"B\")\n  RelIndex(1, a, b, \"Asks\")",
        );
        assert_eq!((graph.edges[0].from, graph.edges[0].to), (0, 1));
        assert_eq!(graph.edges[0].label.as_deref(), Some("1. Asks"));
        assert_eq!(graph.nodes.len(), 2, "the index did not become a box");
    }

    #[test]
    fn a_deployment_node_holds_what_is_declared_inside_it() {
        let graph = graph(
            "C4Deployment\n  Deployment_Node(host, \"Laptop\", \"Linux\") {\n    \
             Container(cli, \"CLI\", \"Rust\")\n  }",
        );
        assert_eq!(graph.clusters[0].title, "Laptop [Linux]");
        assert_eq!(graph.nodes[0].cluster, Some(0));
    }

    #[test]
    fn a_box_says_where_in_the_project_it_is() {
        let c4 = graph(
            "C4Component\n  Component(store, \"store\", \"Rust\", \"Owns bytes\", \
             $link=\"crates/uze-core/src/package/store.rs\")",
        );
        assert_eq!(
            c4.nodes[0].link.as_deref(),
            Some("crates/uze-core/src/package/store.rs")
        );
        assert_eq!(c4.nodes[0].description.as_deref(), Some("Owns bytes"));

        let flow = graph("flowchart TD\n  a[Alpha] --> b\n  click a href \"src/alpha.rs\"");
        assert_eq!(flow.nodes[0].link.as_deref(), Some("src/alpha.rs"));
    }

    #[test]
    fn a_sequence_keeps_its_participants_in_the_order_they_appear() {
        let Diagram::Sequence(sequence) = parse(
            "sequenceDiagram\n  actor U as User\n  U->>CLI: uze install\n  CLI-->>U: done\n  \
             Note over CLI: cached",
        )
        .unwrap() else {
            panic!("expected a sequence");
        };
        assert_eq!(sequence.participants[0].title, "User");
        assert_eq!(sequence.participants[1].id, "CLI");
        assert_eq!(sequence.steps.len(), 3);
    }
}
