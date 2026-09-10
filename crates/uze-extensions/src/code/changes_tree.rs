//! The changed files, projected into the navigator's tree.
//!
//! Strictly presentation state: the view keeps a flat list of changed
//! files because loading a diff and moving a selection are both
//! file-oriented, and this is the one place that flat list is turned into
//! something with directories in it. A directory holding one child is
//! compacted into its parent's row, the way every file tree a person has
//! used before this one does it.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use super::changes::Changes;

#[derive(Default)]
pub(super) struct FileTreeNode {
    pub(super) file_index: Option<usize>,
    pub(super) children: BTreeMap<String, FileTreeNode>,
}

pub(super) enum FileTreeItem {
    Directory {
        /// The directory as [`Changes::folded`] names it: its compacted
        /// path from the root, so `src/ui` folded stays folded when a
        /// sibling appears under `src` and the row stops being compact.
        path: String,
        name: String,
        depth: usize,
        folded: bool,
    },
    File {
        index: usize,
        name: String,
        depth: usize,
    },
}

impl FileTreeItem {
    pub(super) fn file_index(&self) -> Option<usize> {
        match self {
            FileTreeItem::File { index, .. } => Some(*index),
            FileTreeItem::Directory { .. } => None,
        }
    }
}

/// Builds a stable, compact change navigator from repository-relative paths.
/// The model retains a flat `files` vec because diff loading and selection
/// are file-oriented; this projection is strictly presentation state.
pub(super) fn file_tree_items(changes: &Changes, root: &Path) -> Vec<FileTreeItem> {
    tree_items(changes, root, &changes.folded)
}

/// The navigator's rows with `folded` directories shut — the view's own
/// folds normally, or none, for a question about where a file sits
/// regardless of what is showing.
pub(super) fn tree_items(
    changes: &Changes,
    root: &Path,
    folded: &BTreeSet<String>,
) -> Vec<FileTreeItem> {
    let mut tree = FileTreeNode::default();
    for (index, file) in changes.files.iter().enumerate() {
        let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
        let components: Vec<String> = relative
            .components()
            .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
            .collect();
        let mut node = &mut tree;
        for component in &components {
            node = node.children.entry(component.clone()).or_default();
        }
        node.file_index = Some(index);
    }
    let mut items = Vec::new();
    collect_tree_items(&tree, "", 0, folded, &mut items);
    items
}

pub(super) fn collect_tree_items(
    node: &FileTreeNode,
    parent: &str,
    depth: usize,
    folded: &BTreeSet<String>,
    items: &mut Vec<FileTreeItem>,
) {
    for (name, child) in &node.children {
        if child.file_index.is_none() {
            let (name, child) = compact_directory(name, child);
            let path = if parent.is_empty() {
                name.clone()
            } else {
                format!("{parent}/{name}")
            };
            let is_folded = folded.contains(&path);
            items.push(FileTreeItem::Directory {
                name,
                depth,
                folded: is_folded,
                path: path.clone(),
            });
            if !is_folded {
                collect_tree_items(child, &path, depth + 1, folded, items);
            }
        }
    }
    for (name, child) in &node.children {
        if let Some(index) = child.file_index {
            items.push(FileTreeItem::File {
                index,
                name: name.clone(),
                depth,
            });
        }
    }
}

pub(super) fn compact_directory<'a>(
    name: &str,
    mut node: &'a FileTreeNode,
) -> (String, &'a FileTreeNode) {
    let mut path = name.to_owned();
    while node.file_index.is_none() && node.children.len() == 1 {
        let Some((child_name, child)) = node.children.first_key_value() else {
            break;
        };
        if child.file_index.is_some() {
            break;
        }
        path.push('/');
        path.push_str(child_name);
        node = child;
    }
    (path, node)
}

pub(super) fn selected_tree_row(items: &[FileTreeItem], selected: Option<usize>) -> Option<usize> {
    let selected = selected?;
    items
        .iter()
        .position(|item| matches!(item, FileTreeItem::File { index, .. } if *index == selected))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::code::changes::{ChangedFile, FileStatus};

    #[test]
    fn projects_changed_paths_as_a_compact_navigator() {
        let root = PathBuf::from("/repo");
        let changes = Changes {
            files: vec![
                ChangedFile {
                    status: FileStatus::Modified,
                    path: root.join("src/ui/git_diff.rs"),
                },
                ChangedFile {
                    status: FileStatus::Added,
                    path: root.join("src/ui.rs"),
                },
                ChangedFile {
                    status: FileStatus::Untracked,
                    path: root.join("README.md"),
                },
            ],
            ..Changes::default()
        };
        let items = file_tree_items(&changes, &root);
        assert!(
            matches!(items[0], FileTreeItem::Directory { ref name, depth: 0, .. } if name == "src")
        );
        assert!(
            matches!(items[1], FileTreeItem::Directory { ref name, depth: 1, .. } if name == "ui")
        );
        assert!(
            matches!(items[2], FileTreeItem::File { ref name, depth: 2, .. } if name == "git_diff.rs")
        );
        assert!(
            matches!(items[3], FileTreeItem::File { ref name, depth: 1, .. } if name == "ui.rs")
        );
        assert!(
            matches!(items[4], FileTreeItem::File { ref name, depth: 0, .. } if name == "README.md")
        );
        assert_eq!(items.len(), 5, "no worktree header rows above the tree");
        assert_eq!(selected_tree_row(&items, Some(0)), Some(2));
    }
}
