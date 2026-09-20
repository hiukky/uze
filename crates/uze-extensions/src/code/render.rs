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
    CodeView, ContentMode, Focus, MapShowing, NavigatorMode, Showing,
    changes_tree::{FileTreeItem, file_tree_items, selected_tree_row},
    diff::content_line,
    editor::OpenFile,
};
use crate::shared::{canvas::Glyphs, checkout};
use crate::view::{
    Command, Content, ContentLine, Layout, LineTone, Mode, Navigator, NavigatorRow, Role, RowIcon,
    Size, Span, TrailStep, View,
};

/// `space` is advisory: it bounds how much content is worth producing,
/// never where any of it goes.
pub fn view(code: &CodeView, space: Size) -> View {
    let title = checkout::name(super::CATALOG.name);
    let caption = checkout::caption(&code.display_root, &code.branch);
    let footer = footer(code);

    // A surface-level failure — the tab's directory is gone, say — leaves
    // nothing to navigate. A *changes* failure does not: outside a git
    // repository the diff has nothing to say and the tree still works,
    // which is why that one is reported as content.
    if let Some(message) = &code.error {
        return View {
            title,
            caption,
            navigator: None,
            content: Content::Message {
                text: message.clone(),
                hint: None,
                role: Role::Danger,
            },
            footer,
            modes: Vec::new(),
            subjects: Vec::new(),
            layout: Layout::Sidebar,
            trail: Vec::new(),
        };
    }

    // The map is the one mode that is not read beside a list: it is a
    // picture of the whole checkout, and a picture of the whole in a
    // third of the width is a picture of nothing. So it takes the frame,
    // and the tree it replaces is what the toggle goes back to.
    if code.content == ContentMode::Map {
        return View {
            title,
            caption,
            navigator: None,
            content: map_content(code, space),
            footer,
            modes: modes(code),
            subjects: subjects(code),
            layout: Layout::Board,
            trail: map_trail(code),
        };
    }

    View {
        title,
        caption,
        navigator: Some(match code.navigator() {
            NavigatorMode::Changes => changes_navigator(code),
            NavigatorMode::Files => files_navigator(code),
        }),
        content: match code.content {
            ContentMode::Diff => diff_content(code, space),
            ContentMode::Contents => contents_content(code, space),
            ContentMode::Preview => preview_content(code, space),
            ContentMode::Map => unreachable!("the map took the frame above"),
        },
        footer,
        modes: modes(code),
        subjects: subjects(code),
        layout: Layout::Sidebar,
        trail: Vec::new(),
    }
}

/// The map drawn for the space it has, or the table it is a picture of.
fn map_content(code: &CodeView, space: Size) -> Content {
    let Some(map) = code.map_view() else {
        return Content::Message {
            text: "The checkout has not been measured yet".to_owned(),
            hint: None,
            role: Role::Muted,
        };
    };
    let cells = (i32::from(space.width), i32::from(space.height));
    let lines: Vec<ContentLine> = match code.map_showing() {
        MapShowing::Ranking => map
            .ranking()
            .lines()
            .map(|line| plain_line(line.to_owned()))
            .collect(),
        showing => {
            let glyphs = match showing {
                MapShowing::Ascii => Glyphs::Ascii,
                _ => Glyphs::Unicode,
            };
            map.paint(cells, glyphs).lines()
        }
    };
    // A board hands over its screen and nothing else: it is cut to the
    // frame rather than scrolled, so there is no scroll to apply.
    Content::Lines {
        heading: map.caption(cells),
        scroll: 0,
        first: 0,
        total: lines.len(),
        lines,
        caret: None,
    }
}

fn plain_line(text: String) -> ContentLine {
    ContentLine {
        gutter: " ".to_owned(),
        number: String::new(),
        tone: LineTone::Neutral,
        spans: vec![Span::new(text, Role::Default)],
    }
}

/// The descent the map is in: the checkout, then each directory entered.
fn map_trail(code: &CodeView) -> Vec<TrailStep> {
    let Some(map) = code.map_view() else {
        return Vec::new();
    };
    let crumbs = map.crumbs();
    let depth = crumbs.len();
    std::iter::once(code.checkout_name())
        .chain(crumbs)
        .enumerate()
        .map(|(step, name)| TrailStep::new(name, step == depth))
        .collect()
}

/// The ways this surface can show what it is showing.
///
/// Only for a document, and only while its contents are what is on
/// screen: offering "preview" beside a diff would be offering to leave
/// the diff, which is what the diff's own door is for. The map is
/// offered wherever there is one, because it is the same checkout seen
/// another way — and the entry that leaves it is what makes the toggle a
/// control somebody can point at rather than a key they have to know.
fn modes(code: &CodeView) -> Vec<Mode> {
    code.showings()
        .into_iter()
        .map(|showing| Mode {
            label: mode_label(showing),
            active: match showing {
                Showing::Selection(mode) => code.content == mode,
                Showing::Measured(map) => {
                    code.content == ContentMode::Map && code.map_showing() == map
                }
            },
        })
        .collect()
}

/// The two things this surface can be about, named as the halves they
/// are: the list of files, and the map of the checkout they are in.
///
/// Not named after the *rendering* the map happens to be in — that is
/// what the modes beside it are for, and a chip that said "ASCII" here
/// would be answering a question nobody asked of this end of the row.
fn subjects(code: &CodeView) -> Vec<Mode> {
    code.subjects()
        .into_iter()
        .map(|showing| Mode {
            label: match showing {
                Showing::Selection(_) => "Files".to_owned(),
                Showing::Measured(_) => "Map".to_owned(),
            },
            active: match showing {
                Showing::Selection(_) => code.content != ContentMode::Map,
                Showing::Measured(_) => code.content == ContentMode::Map,
            },
        })
        .collect()
}

fn mode_label(showing: Showing) -> String {
    match showing {
        Showing::Selection(ContentMode::Preview) => "Preview".to_owned(),
        Showing::Selection(ContentMode::Contents) => "Source".to_owned(),
        Showing::Selection(ContentMode::Diff) => "Changes".to_owned(),
        Showing::Selection(ContentMode::Map) => "Map".to_owned(),
        Showing::Measured(MapShowing::Unicode) => "Map".to_owned(),
        Showing::Measured(MapShowing::Ascii) => "ASCII".to_owned(),
        Showing::Measured(MapShowing::Ranking) => "Ranking".to_owned(),
    }
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
    // The map is the same checkout seen another way, so it is offered
    // wherever the checkout is — and from inside it, the way back out is
    // the same key.
    if code.content == ContentMode::Map {
        return vec![
            Command::SelectNext,
            Command::Activate,
            Command::ToggleMap,
            Command::Close,
        ];
    }
    let mut commands = vec![Command::SelectNext, Command::Collapse, Command::Expand];
    if code.selected_is_markdown() {
        commands.push(Command::TogglePreview);
    }
    if code.selected_is_a_file() {
        commands.push(Command::Edit);
        commands.push(Command::Delete);
    }
    if code
        .subjects()
        .iter()
        .any(|showing| matches!(showing, Showing::Measured(_)))
    {
        commands.push(Command::ToggleMap);
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
        choosing: None,
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
                    icon: RowIcon::None,
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
                    icon: RowIcon::None,
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
        choosing: None,
        rows: rows
            .iter()
            .enumerate()
            .map(|(index, row)| match row.directory {
                true => NavigatorRow::Group {
                    id: index,
                    name: row.name.clone(),
                    depth: row.depth,
                    collapsed: !row.expanded,
                    icon: super::files::icon_for(&row.name, true, row.expanded),
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
                    icon: super::files::icon_for(&row.name, false, false),
                },
            })
            .collect(),
    }
}

fn diff_content(code: &CodeView, space: Size) -> Content {
    if let Some(message) = &code.changes.error {
        return Content::Message {
            text: message.clone(),
            hint: None,
            role: Role::Danger,
        };
    }
    if code.changes.files.is_empty() {
        return Content::Message {
            text: "Nothing has changed".to_owned(),
            hint: Some("This checkout matches the branch it started from.".to_owned()),
            role: Role::Muted,
        };
    }
    let Some(index) = code.selected_change() else {
        return Content::Message {
            text: "No file selected".to_owned(),
            hint: Some("Pick one on the left to read what changed in it, line by line.".to_owned()),
            role: Role::Muted,
        };
    };
    if code.changes.diff_pending {
        // The selection moved and its diff is still being read. Saying so
        // beats showing the previous file's diff under the new file's
        // name, and beats an empty pane that reads as "no changes".
        return Content::Message {
            text: "reading…".to_owned(),
            hint: None,
            role: Role::Muted,
        };
    }
    let diff = &code.changes.diff;
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
        first: code.scroll as usize,
        // Twice the space rather than exactly it: a wrapped line occupies
        // more than one row, and only the host — which does the wrapping
        // — knows how many. Erring long costs a few unrendered lines;
        // erring short would show blank rows at the bottom of a long
        // diff.
        lines: diff
            .iter()
            .skip(code.scroll as usize)
            .take(usize::from(space.height).saturating_mul(2))
            .map(content_line)
            .collect(),
    }
}

/// The same file the contents mode holds, shown as the document it
/// describes. It reads the *buffer*, not the disk — so a preview of
/// something being edited shows what was typed, which is the whole point
/// of previewing while you write.
fn preview_content(code: &CodeView, space: Size) -> Content {
    let open = match readable_open_file(
        code,
        "No document selected",
        "Pick a file on the left. This shows it rendered; Source shows what is in it.",
    ) {
        Ok(open) => open,
        Err(message) => return message,
    };
    let (total, lines) = open.preview(
        code.scroll as usize,
        usize::from(space.height).saturating_mul(2),
    );
    Content::Lines {
        caret: None,
        total,
        heading: format!(
            "{} · preview",
            open.path
                .strip_prefix(&code.root)
                .unwrap_or(&open.path)
                .display()
        ),
        scroll: code.scroll,
        first: code.scroll as usize,
        lines,
    }
}

/// The open file, once there is one and it has been read — or the message
/// that stands where it would be: nothing open (`empty` and its `hint`), a
/// file that could not be read, or one still being read.
fn readable_open_file<'a>(
    code: &'a CodeView,
    empty: &str,
    hint: &str,
) -> Result<&'a OpenFile, Content> {
    let Some(open) = code.open.as_ref() else {
        return Err(Content::Message {
            text: empty.to_owned(),
            hint: Some(hint.to_owned()),
            role: Role::Muted,
        });
    };
    if let Some(message) = &open.error {
        return Err(Content::Message {
            text: message.clone(),
            hint: None,
            role: Role::Danger,
        });
    }
    if open.loading {
        return Err(Content::Message {
            text: "reading…".to_owned(),
            hint: None,
            role: Role::Muted,
        });
    }
    Ok(open)
}

fn contents_content(code: &CodeView, space: Size) -> Content {
    let open = match readable_open_file(
        code,
        "No file selected",
        "Pick one on the left to read it, or press e to edit it in place.",
    ) {
        Ok(open) => open,
        Err(message) => return message,
    };
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
        // The window starts where the viewer is, and says so: the line
        // numbers, the caret's line and the hits a click lands in are all
        // indices into the file, and `first` is what keeps them meaning
        // that on both sides.
        first: code.scroll as usize,
        lines: open
            .lines
            .iter()
            .enumerate()
            .skip(code.scroll as usize)
            .take(usize::from(space.height).saturating_mul(2))
            .map(|(index, text)| ContentLine {
                gutter: " ".to_owned(),
                number: (index + 1).to_string(),
                tone: LineTone::Neutral,
                // A line with no colouring yet — one past the glance the
                // read coloured, or one just typed — is drawn as the text
                // it is until the pass that colours it lands.
                spans: open
                    .highlighted
                    .get(index)
                    .filter(|spans| !spans.is_empty())
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
