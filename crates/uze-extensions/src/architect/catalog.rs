//! What there is to look at: areas, and the artifacts in each.
//!
//! An artifact is a Mermaid file in the directory the project declares.
//! Everything the surface needs to know about it is read off the file
//! itself — its area from the diagram's first word, its name from a
//! `title:` — so there is no index beside the files to keep in step with
//! them. A list of what a directory holds is the one document that is
//! wrong the moment somebody adds a file.

use std::path::{Path, PathBuf};

use crate::Host;

/// How deep the declared directory is read. Deep enough for a project
/// that sorts its diagrams into folders; bounded, because the path is
/// the project's to declare and a walk with no floor is how a surface
/// ends up reading a `node_modules`.
const DEPTH: usize = 4;
const EXTENSIONS: [&str; 2] = ["mmd", "mermaid"];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    C4,
    Sequence,
    Flowchart,
    /// Mermaid this surface does not draw yet. Listed rather than hidden:
    /// a file that silently fails to appear looks like a lost file.
    Other,
}

impl Kind {
    pub fn of(diagram: &str) -> Self {
        let keyword = first_word(diagram);
        match keyword {
            "flowchart" | "graph" => Self::Flowchart,
            "sequenceDiagram" => Self::Sequence,
            _ if keyword.starts_with("C4") => Self::C4,
            _ => Self::Other,
        }
    }

    /// How far down the model a diagram sits, for the kinds that are a
    /// model of levels. C4 is read from the outside in — the system, what
    /// is in it, what is in that — and its views are listed the way they
    /// are read; the two that are not a level of that descent come after
    /// the three that are. Everything else is on one level.
    fn depth(self, diagram: &str) -> u8 {
        if self != Self::C4 {
            return 0;
        }
        match first_word(diagram) {
            "C4Context" => 1,
            "C4Container" => 2,
            "C4Component" => 3,
            "C4Dynamic" => 4,
            _ => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::C4 => "C4",
            Self::Sequence => "Sequence",
            Self::Flowchart => "Flowchart",
            Self::Other => "Other",
        }
    }
}

/// The word a Mermaid diagram opens with, which says what it is.
fn first_word(diagram: &str) -> &str {
    diagram
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artifact {
    pub name: String,
    pub kind: Kind,
    /// Its level within its area; see [`Kind::depth`].
    depth: u8,
    /// Where it lives, as the project would say it.
    pub origin: String,
    /// The file as written, front matter and all — what `Source` shows.
    pub source: String,
}

impl Artifact {
    pub fn read(origin: impl Into<String>, source: impl Into<String>) -> Self {
        let (origin, source) = (origin.into(), source.into());
        let (front_matter, diagram) = split_front_matter(&source);
        let name = titled(front_matter)
            .or_else(|| titled(diagram))
            .unwrap_or_else(|| name_from_path(&origin));
        let kind = Kind::of(diagram);
        Self {
            name,
            kind,
            depth: kind.depth(diagram),
            origin,
            source,
        }
    }

    /// Which level of its area's model this is, or zero where the area
    /// is not a model of levels. See [`Kind::depth`].
    pub fn level(&self) -> u8 {
        self.depth
    }

    /// The diagram without its front matter, which Mermaid reads as
    /// settings and this surface reads only for the name.
    pub fn diagram(&self) -> &str {
        split_front_matter(&self.source).1
    }
}

/// Mermaid's own front matter: a YAML block between two `---` lines at
/// the very top of the file.
fn split_front_matter(source: &str) -> (&str, &str) {
    let Some(rest) = source.trim_start().strip_prefix("---") else {
        return ("", source);
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            (
                &rest[..end],
                after.split_once('\n').map_or("", |(_, body)| body),
            )
        }
        None => ("", source),
    }
}

/// A `title:` in front matter, or the `title` statement a diagram may
/// carry in its own body.
fn titled(text: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let rest = line.strip_prefix("title")?;
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        let starts_a_value = rest.starts_with(char::is_whitespace);
        let title = rest.trim().trim_matches('"').trim();
        (starts_a_value && !title.is_empty()).then(|| title.to_owned())
    })
}

fn name_from_path(origin: &str) -> String {
    let stem = Path::new(origin)
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(['-', '_'], " "))
        .unwrap_or_default();
    let mut letters = stem.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => origin.to_owned(),
    }
}

/// Every artifact, ordered by area, then by level, then by name — which
/// is also the order the menu is in, so an index into this is an
/// artifact's identity for as long as the catalog is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Catalog {
    artifacts: Vec<Artifact>,
}

impl Catalog {
    pub fn of(mut artifacts: Vec<Artifact>) -> Self {
        artifacts.sort_by(|a, b| (a.kind, a.depth, &a.name).cmp(&(b.kind, b.depth, &b.name)));
        Self { artifacts }
    }

    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    pub fn get(&self, index: usize) -> Option<&Artifact> {
        self.artifacts.get(index)
    }
}

/// Every Mermaid file under `directory`, or why it could not be listed.
pub fn read(host: &dyn Host, directory: &Path) -> Result<Vec<Artifact>, String> {
    let mut artifacts = Vec::new();
    let mut pending: Vec<(PathBuf, usize)> = vec![(directory.to_path_buf(), 0)];
    while let Some((folder, depth)) = pending.pop() {
        let entries = match host.list_dir(&folder) {
            Ok(entries) => entries,
            Err(reason) if depth == 0 => return Err(reason),
            Err(_) => continue,
        };
        for entry in entries {
            let path = folder.join(&entry.name);
            if entry.name.starts_with('.') {
                continue;
            }
            if entry.directory {
                if depth < DEPTH {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            let is_mermaid = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| EXTENSIONS.contains(&extension));
            if !is_mermaid {
                continue;
            }
            let origin = path
                .strip_prefix(directory)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            // An unreadable file still gets its tab: the reason is what
            // the board shows, which is more use than the file going missing.
            let source = host
                .read_file(&path)
                .unwrap_or_else(|reason| format!("%% {reason}\n"));
            artifacts.push(Artifact::read(origin, source));
        }
    }
    Ok(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_artifact_says_which_area_it_belongs_to() {
        assert_eq!(Kind::of("%% note\n\nC4Container\n title x"), Kind::C4);
        assert_eq!(Kind::of("sequenceDiagram\n a->>b: hi"), Kind::Sequence);
        assert_eq!(Kind::of("graph LR\n a --> b"), Kind::Flowchart);
        assert_eq!(Kind::of("gantt\n title x"), Kind::Other);
    }

    #[test]
    fn an_artifact_names_itself_and_falls_back_to_its_file() {
        let titled = Artifact::read("a/ctx.mmd", "---\ntitle: System context\n---\nC4Context\n");
        assert_eq!(
            (titled.name.as_str(), titled.kind),
            ("System context", Kind::C4)
        );
        assert_eq!(titled.diagram().trim(), "C4Context");

        let in_body = Artifact::read("x.mmd", "C4Container\n  title \"Containers of uze\"\n");
        assert_eq!(in_body.name, "Containers of uze");

        let unnamed = Artifact::read("flows/install-pipeline.mmd", "flowchart LR\n a --> b\n");
        assert_eq!(unnamed.name, "Install pipeline");
        assert_eq!(unnamed.diagram(), "flowchart LR\n a --> b\n");
    }

    #[test]
    fn a_node_called_title_is_not_a_title() {
        let artifact = Artifact::read("x.mmd", "flowchart TD\n  titles --> b\n  title[Heading]\n");
        assert_eq!(artifact.name, "X");
    }

    #[test]
    fn c4_views_are_listed_from_the_outside_in() {
        let catalog = Catalog::of(vec![
            Artifact::read("a.mmd", "---\ntitle: Core components\n---\nC4Component\n"),
            Artifact::read("b.mmd", "---\ntitle: Containers\n---\nC4Container\n"),
            Artifact::read("c.mmd", "---\ntitle: Deployment\n---\nC4Deployment\n"),
            Artifact::read("d.mmd", "---\ntitle: System context\n---\nC4Context\n"),
            Artifact::read("e.mmd", "---\ntitle: Agent components\n---\nC4Component\n"),
        ]);
        let names: Vec<&str> = catalog
            .artifacts()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "System context",
                "Containers",
                "Agent components",
                "Core components",
                "Deployment"
            ],
            "by level, and by name only within one"
        );
    }

    #[test]
    fn the_catalog_is_ordered_by_area_then_by_name() {
        let catalog = Catalog::of(vec![
            Artifact::read("b.mmd", "flowchart TD\n a --> b"),
            Artifact::read("z.mmd", "C4Context\n"),
            Artifact::read("a.mmd", "C4Context\n"),
        ]);
        let names: Vec<&str> = catalog
            .artifacts()
            .iter()
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(names, ["A", "Z", "B"]);
    }
}
