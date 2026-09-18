//! Where a project keeps the artifacts that describe it.
//!
//! The manifest names a directory; this is that name resolved into a
//! place, or the reason it cannot be one. A presentation surface asks
//! this rather than reading `agents.yaml` itself, which keeps the file's
//! shape — and what counts as a path the project may point at — in one
//! place.

use std::path::{Component, Path, PathBuf};

use uze_core::{manifest, project_root};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectArtifacts {
    /// The project declares no `artifacts:`, which is an answer and not a
    /// fault: most projects have not drawn anything yet.
    Undeclared,
    /// The declared directory: where it is, and how the project spelled
    /// it — the form to show somebody, since it is the one they wrote. It
    /// may not exist yet; that is for whoever lists it to say.
    Declared {
        directory: PathBuf,
        declared: PathBuf,
    },
    /// Declared, and not something UZE will follow.
    Refused(String),
}

/// What the project `cwd` sits in declares about its artifacts.
pub fn project_artifacts(cwd: &Path) -> ProjectArtifacts {
    let Ok(root) = project_root::resolve_project_root(cwd) else {
        return ProjectArtifacts::Undeclared;
    };
    let declared = match manifest::load(&root) {
        Ok(Some(manifest)) => manifest.artifacts,
        Ok(None) => None,
        Err(error) => return ProjectArtifacts::Refused(error.to_string()),
    };
    match declared {
        None => ProjectArtifacts::Undeclared,
        Some(artifacts) if stays_inside(&artifacts.path) => ProjectArtifacts::Declared {
            directory: root.join(&artifacts.path),
            declared: artifacts.path,
        },
        Some(artifacts) => ProjectArtifacts::Refused(format!(
            "`artifacts.path` is `{}`, which leaves the project — it has to be a \
             directory inside it",
            artifacts.path.display()
        )),
    }
}

/// A manifest is checked in and travels: a path that climbs out of the
/// project, or starts at the machine's root, would point somewhere else
/// on every machine that reads it.
fn stays_inside(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(manifest: &str) -> uze_testkit::temp::TempDir {
        let directory = uze_testkit::temp::TempDir::new("project-artifacts");
        fs::write(directory.join("agents.yaml"), manifest).unwrap();
        directory
    }

    #[test]
    fn a_declared_directory_is_resolved_against_the_project_root() {
        let project = project("artifacts:\n  path: docs/architecture\n");
        let nested = project.path().join("src");
        fs::create_dir(&nested).unwrap();
        let ProjectArtifacts::Declared {
            directory,
            declared,
        } = project_artifacts(&nested)
        else {
            panic!("the project declares a directory");
        };
        assert_eq!(declared, PathBuf::from("docs/architecture"));
        assert!(directory.ends_with("docs/architecture"));
        assert!(directory.is_absolute());
    }

    #[test]
    fn a_project_that_declares_nothing_is_not_an_error() {
        let project = project("worktrees:\n  completion: handoff\n");
        assert_eq!(
            project_artifacts(project.path()),
            ProjectArtifacts::Undeclared
        );
    }

    #[test]
    fn a_path_that_leaves_the_project_is_refused() {
        for path in ["../elsewhere", "/etc"] {
            let project = project(&format!("artifacts:\n  path: {path}\n"));
            assert!(
                matches!(
                    project_artifacts(project.path()),
                    ProjectArtifacts::Refused(_)
                ),
                "{path} was followed"
            );
        }
    }
}
