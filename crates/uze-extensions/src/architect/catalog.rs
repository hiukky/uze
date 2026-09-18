//! What there is to look at: areas, and the artifacts in each.
//!
//! An area is a *kind* of diagram — C4, sequence, flowchart — and it is
//! read off the artifact itself: a Mermaid source says what it is in its
//! first word, so nothing beside it has to repeat that and nothing can
//! disagree with it.

use super::samples::SAMPLES;

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
    pub fn of(source: &str) -> Self {
        let keyword = source
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("%%"))
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or_default();
        match keyword {
            "flowchart" | "graph" => Self::Flowchart,
            "sequenceDiagram" => Self::Sequence,
            _ if keyword.starts_with("C4") => Self::C4,
            _ => Self::Other,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artifact {
    pub name: String,
    pub kind: Kind,
    pub source: String,
}

/// Every artifact, ordered by area and then by name — which is also the
/// order the tabs are in, so an index into this is an artifact's identity
/// for as long as the catalog is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Catalog {
    artifacts: Vec<Artifact>,
}

impl Catalog {
    pub fn of(mut artifacts: Vec<Artifact>) -> Self {
        artifacts.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        Self { artifacts }
    }

    pub fn built_in() -> Self {
        Self::of(
            SAMPLES
                .iter()
                .map(|sample| Artifact {
                    name: sample.name.to_owned(),
                    kind: Kind::of(sample.source),
                    source: sample.source.to_owned(),
                })
                .collect(),
        )
    }

    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    pub fn get(&self, index: usize) -> Option<&Artifact> {
        self.artifacts.get(index)
    }
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
    fn the_catalog_is_ordered_by_area_then_by_name() {
        let catalog = Catalog::built_in();
        let kinds: Vec<Kind> = catalog.artifacts().iter().map(|a| a.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted);
        assert_eq!(catalog.get(0).map(|a| a.kind), Some(Kind::C4));
    }
}
