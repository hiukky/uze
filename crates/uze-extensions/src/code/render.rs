//! What the code surface shows, as data.
//!
//! Reaches nothing: everything a view says was resolved when the checkout
//! was read (see [`CodeView::display_root`]). Drawing is the one thing in
//! this crate that touches no capability at all, and the architecture
//! suite holds the renderer to it.
//!
//! The two modes are two functions picked by [`CodeView::navigator`] and
//! [`CodeView::content`] — never a branch inside one that asks what the
//! state means. That is the rule the module doc states, expressed where
//! it is easiest to break.

use super::{
    CodeView, ContentMode, Focus, NavigatorMode,
    changes_tree::{FileTreeItem, file_tree_items, selected_tree_row},
    diff::{content_line, unified_lines},
};
use crate::view::{
    Command, Content, ContentLine, LineTone, Mode, Navigator, NavigatorRow, Role, Size, Span, View,
};

/// `space` is advisory: it bounds how much content is worth producing,
/// never where any of it goes.
pub fn view(code: &CodeView, space: Size) -> View {
    let title = title(code);
    let footer = footer(code);

    // A surface-level failure — the tab's directory is gone, say — leaves
    // nothing to navigate. A *changes* failure does not: outside a git
    // repository the diff has nothing to say and the tree still works,
    // which is why that one is reported as content.
    if let Some(message) = &code.error {
        return View {
            title,
            navigator: None,
            content: Content::Message {
                text: message.clone(),
                role: Role::Danger,
            },
            footer,
            modes: Vec::new(),
        };
    }

    View {
        title,
        navigator: Some(match code.navigator {
            NavigatorMode::Changes => changes_navigator(code),
            NavigatorMode::Files => files_navigator(code),
        }),
        content: match code.content {
            ContentMode::Diff => diff_content(code, space),
            ContentMode::Contents => contents_content(code, space),
            ContentMode::Preview => preview_content(code, space),
        },
        footer,
        modes: modes(code),
    }
}

/// The ways the selection can be shown, when there is more than one.
///
/// Only for a document, and only while its contents are what is on
/// screen: offering "preview" beside a diff would be offering to leave
/// the diff, which is what the diff's own door is for.
fn modes(code: &CodeView) -> Vec<Mode> {
    if !code.selected_is_markdown() || code.content == ContentMode::Diff {
        return Vec::new();
    }
    [
        ("Preview", ContentMode::Preview),
        ("Source", ContentMode::Contents),
    ]
    .into_iter()
    .map(|(label, mode)| Mode {
        label: label.to_owned(),
        active: code.content == mode,
    })
    .collect()
}

/// What the surface is, which checkout it is on, and which branch that
/// checkout is at — in that order, and told apart by weight.
///
/// The three are not equally interesting. The name is a label and is
/// said once; the directories leading to the checkout are context; the
/// checkout's own name and its branch are what identify it, and they are
/// what the eye should land on. One run of text gave all three the same
/// weight, which is how a title stops being read.
fn title(code: &CodeView) -> Vec<Span> {
    let (parent, name) = match code.display_root.rsplit_once('/') {
        Some((parent, name)) => (format!("{parent}/"), name.to_owned()),
        None => (String::new(), code.display_root.clone()),
    };
    let mut spans = vec![
        Span::new("code", Role::Muted),
        Span::new(" · ", Role::Faint),
        Span::new(parent, Role::Dim),
        Span::new(name, Role::Bright).bold(),
    ];
    if !code.branch.is_empty() {
        spans.push(Span::new(" · ", Role::Faint));
        spans.push(Span::new(code.branch.clone(), Role::Accent).bold());
    }
    spans
}

/// What this surface can be asked, in the order the footer should name
/// them. Commands, never keys: the host prints each with whatever chord
/// currently reaches it, so a rebound key needs no change here.
fn footer(code: &CodeView) -> Vec<Command> {
    if code.confirming_delete.is_some() {
        return vec![Command::ConfirmDelete, Command::Close];
    }
    if code.editing() {
        return vec![Command::Save, Command::Close];
    }
    let mut commands = vec![Command::SelectNext, Command::Collapse, Command::Expand];
    if code.selected_is_markdown() {
        commands.push(Command::TogglePreview);
    }
    if code.selected_is_a_file() {
        commands.push(Command::Edit);
        commands.push(Command::Delete);
    }
    commands.push(Command::FocusNext);
    commands.push(Command::Close);
    commands
}

pub(super) fn changes_navigator(code: &CodeView) -> Navigator {
    let items = file_tree_items(&code.changes, &code.root);
    let selected = code.selected_change();
    Navigator {
        heading: "CHANGES".to_owned(),
        badge: code.changes.files.len().to_string(),
        focused: code.focus == Focus::Navigator,
        anchor: selected_tree_row(&items, selected),
        rows: items
            .iter()
            .enumerate()
            .map(|(row, item)| match item {
                FileTreeItem::Directory {
                    name,
                    depth,
                    folded,
                    ..
                } => NavigatorRow::Group {
                    id: row,
                    name: format!("{name}/"),
                    depth: *depth,
                    collapsed: *folded,
                },
                FileTreeItem::File { index, name, depth } => NavigatorRow::Item {
                    id: *index,
                    name: name.clone(),
                    depth: depth + 1,
                    marker: Span::new(
                        code.changes.files[*index].status.glyph(),
                        code.changes.files[*index].status.role(),
                    ),
                    selected: selected == Some(*index),
                },
            })
            .collect(),
    }
}

fn files_navigator(code: &CodeView) -> Navigator {
    let rows = code.files.rows(&code.root);
    let anchor = code
        .selected
        .as_ref()
        .and_then(|path| rows.iter().position(|row| &row.path == path));
    Navigator {
        heading: "FILES".to_owned(),
        badge: rows.iter().filter(|row| !row.directory).count().to_string(),
        focused: code.focus == Focus::Navigator,
        anchor,
        rows: rows
            .iter()
            .enumerate()
            .map(|(index, row)| match row.directory {
                true => NavigatorRow::Group {
                    id: index,
                    name: row.name.clone(),
                    depth: row.depth,
                    collapsed: !row.expanded,
                },
                false => NavigatorRow::Item {
                    id: index,
                    name: row.name.clone(),
                    // A file that changed says so here too, so the two
                    // lists mark the same file the same way.
                    marker: match code.changes.position_of(Some(&row.path)) {
                        Some(index) => Span::new(
                            code.changes.files[index].status.glyph(),
                            code.changes.files[index].status.role(),
                        ),
                        None => Span::new(String::new(), Role::Muted),
                    },
                    depth: row.depth,
                    selected: code.selected.as_ref() == Some(&row.path),
                },
            })
            .collect(),
    }
}

fn diff_content(code: &CodeView, space: Size) -> Content {
    if let Some(message) = &code.changes.error {
        return Content::Message {
            text: message.clone(),
            role: Role::Danger,
        };
    }
    if code.changes.files.is_empty() {
        return Content::Message {
            text: "no changes".to_owned(),
            role: Role::Muted,
        };
    }
    let Some(index) = code.selected_change() else {
        return Content::Message {
            text: "select a changed file".to_owned(),
            role: Role::Muted,
        };
    };
    if code.changes.diff_pending {
        // The selection moved and its diff is still being read. Saying so
        // beats showing the previous file's diff under the new file's
        // name, and beats an empty pane that reads as "no changes".
        return Content::Message {
            text: "reading…".to_owned(),
            role: Role::Muted,
        };
    }
    let diff = unified_lines(&code.changes.diff);
    Content::Lines {
        caret: None,
        total: diff.len(),
        heading: format!(
            "DIFF · {}",
            code.changes.files[index]
                .path
                .strip_prefix(&code.root)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| code.display_root.clone())
        ),
        scroll: code.scroll,
        // Bounded by the space plus what is scrolled past, not by the
        // space alone: a wrapped line occupies more than one row, and
        // only the host — which does the wrapping — knows how many.
        // Erring long costs a few unrendered lines; erring short would
        // show blank rows at the bottom of a long diff.
        lines: diff
            .into_iter()
            .take(usize::from(space.height).saturating_mul(2) + code.scroll as usize)
            .map(content_line)
            .collect(),
    }
}

/// The same file the contents mode holds, shown as the document it
/// describes. It reads the *buffer*, not the disk — so a preview of
/// something being edited shows what was typed, which is the whole point
/// of previewing while you write.
fn preview_content(code: &CodeView, space: Size) -> Content {
    let Some(open) = code.open.as_ref() else {
        return Content::Message {
            text: "select a document".to_owned(),
            role: Role::Muted,
        };
    };
    if let Some(message) = &open.error {
        return Content::Message {
            text: message.clone(),
            role: Role::Danger,
        };
    }
    if open.loading {
        return Content::Message {
            text: "reading…".to_owned(),
            role: Role::Muted,
        };
    }
    let lines = super::markdown::render(&open.contents(), &open.theme);
    Content::Lines {
        caret: None,
        total: lines.len(),
        heading: format!(
            "{} · preview",
            open.path
                .strip_prefix(&code.root)
                .unwrap_or(&open.path)
                .display()
        ),
        scroll: code.scroll,
        lines: lines
            .into_iter()
            .take(usize::from(space.height).saturating_mul(2) + code.scroll as usize)
            .collect(),
    }
}

fn contents_content(code: &CodeView, space: Size) -> Content {
    let Some(open) = code.open.as_ref() else {
        return Content::Message {
            text: "select a file".to_owned(),
            role: Role::Muted,
        };
    };
    if let Some(message) = &open.error {
        return Content::Message {
            text: message.clone(),
            role: Role::Danger,
        };
    }
    if open.loading {
        return Content::Message {
            text: "reading…".to_owned(),
            role: Role::Muted,
        };
    }
    Content::Lines {
        total: open.lines.len(),
        heading: format!(
            "{}{}",
            open.path
                .strip_prefix(&code.root)
                .unwrap_or(&open.path)
                .display(),
            match (open.editing, open.modified) {
                (_, true) => " · unsaved",
                (true, false) => " · editing",
                (false, false) => "",
            }
        ),
        scroll: code.scroll,
        // From the first line, never from the scrolled-to one: the host
        // is what applies `scroll`, so skipping here would move the
        // content twice for one wheel notch — and the caret's line index
        // has to keep meaning the same thing on both sides of that.
        lines: open
            .lines
            .iter()
            .enumerate()
            .take(usize::from(space.height).saturating_mul(2) + code.scroll as usize)
            .map(|(index, text)| ContentLine {
                gutter: " ".to_owned(),
                number: (index + 1).to_string(),
                tone: LineTone::Neutral,
                spans: open
                    .highlighted
                    .get(index)
                    .map(|spans| {
                        spans
                            .iter()
                            .map(|(colour, piece)| Span {
                                text: piece.clone(),
                                role: Role::Default,
                                color: Some(*colour),
                                bold: false,
                                italic: false,
                            })
                            .collect()
                    })
                    .unwrap_or_else(|| vec![Span::new(text.clone(), Role::Default)]),
            })
            .collect(),
        caret: (open.editing && code.content == ContentMode::Contents).then_some(open.caret),
    }
}
