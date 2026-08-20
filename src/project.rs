use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    capability::{EnhancementKind, ItemKind, PortableKind, ProjectItem},
    error::{Result, UzeError},
};

#[derive(Clone, Debug)]
pub struct ResolvedProject {
    pub root: PathBuf,
    pub portable_core: Vec<ProjectItem>,
    pub enhancements: Vec<ProjectItem>,
}

pub fn resolve_project(root: impl AsRef<Path>) -> Result<ResolvedProject> {
    let root = root.as_ref();
    if !root.exists() {
        return Err(UzeError::MissingPath(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(UzeError::NotDirectory(root.to_path_buf()));
    }

    let root = root.canonicalize().map_err(|source| UzeError::Read {
        path: root.to_path_buf(),
        source,
    })?;
    let mut portable_core = Vec::new();
    let mut enhancements = Vec::new();

    push_file_if_present(
        &mut portable_core,
        root.join("AGENTS.md"),
        ItemKind::Portable(PortableKind::Instruction),
    )?;
    discover_skills(&root, &mut portable_core)?;
    discover_mcp(&root, &mut portable_core)?;
    discover_enhancements(&root, &mut enhancements)?;

    sort_items(&mut portable_core);
    sort_items(&mut enhancements);
    Ok(ResolvedProject {
        root,
        portable_core,
        enhancements,
    })
}

fn discover_skills(root: &Path, items: &mut Vec<ProjectItem>) -> Result<()> {
    let skills_root = root.join(".agents/skills");
    if !skills_root.is_dir() {
        return Ok(());
    }

    for path in files_named(&skills_root, "SKILL.md")? {
        push_file_if_present(items, path, ItemKind::Portable(PortableKind::Skill))?;
    }
    Ok(())
}

fn discover_mcp(root: &Path, items: &mut Vec<ProjectItem>) -> Result<()> {
    for name in ["mcp.json", ".mcp.json"] {
        let path = root.join(name);
        if path.is_file() {
            let payload = read_file(&path)?;
            serde_json::from_slice::<serde_json::Value>(&payload).map_err(|source| {
                UzeError::Json {
                    path: path.clone(),
                    source,
                }
            })?;
            items.push(ProjectItem {
                kind: ItemKind::Portable(PortableKind::Mcp),
                path,
                payload,
            });
        }
    }
    Ok(())
}

fn discover_enhancements(root: &Path, items: &mut Vec<ProjectItem>) -> Result<()> {
    for directory in [".claude", ".codex", ".cursor", ".opencode"] {
        let path = root.join(directory);
        if !path.is_dir() {
            continue;
        }

        let initial_len = items.len();
        for (relative_path, kind) in [
            ("commands", EnhancementKind::Command),
            ("hooks", EnhancementKind::Hook),
            ("agents", EnhancementKind::Subagent),
            ("settings.json", EnhancementKind::Permission),
            ("permissions.json", EnhancementKind::Permission),
        ] {
            let candidate = path.join(relative_path);
            if candidate.exists() {
                let payload = if candidate.is_file() {
                    read_file(&candidate)?
                } else {
                    Vec::new()
                };
                items.push(ProjectItem {
                    kind: ItemKind::Enhancement(kind),
                    path: candidate,
                    payload,
                });
            }
        }

        if items.len() == initial_len {
            items.push(ProjectItem {
                kind: ItemKind::Enhancement(EnhancementKind::VendorDirectory),
                path,
                payload: Vec::new(),
            });
        }
    }
    Ok(())
}

pub(crate) fn files_named(root: &Path, expected_name: &str) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|source| UzeError::Read {
                path: directory.clone(),
                source,
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| UzeError::Read {
                path: directory.clone(),
                source,
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(expected_name) {
                matches.push(path);
            }
        }
    }
    matches.sort();
    Ok(matches)
}

pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| UzeError::Read {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn push_file_if_present(
    items: &mut Vec<ProjectItem>,
    path: PathBuf,
    kind: ItemKind,
) -> Result<()> {
    if path.is_file() {
        let payload = read_file(&path)?;
        items.push(ProjectItem {
            kind,
            path,
            payload,
        });
    }
    Ok(())
}

fn sort_items(items: &mut [ProjectItem]) {
    items.sort_by(|left, right| left.path.cmp(&right.path));
}
