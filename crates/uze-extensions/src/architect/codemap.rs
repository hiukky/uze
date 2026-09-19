//! The checkout as a map: where its lines are, and which of them are hot.
//!
//! The one artifact nobody writes. It is measured rather than read — a
//! file's lines, how many commits touched it this year, whether it differs
//! from what was committed — and drawn as a treemap, which is the diagram
//! a grid of cells is best at: rectangles in rectangles, and no edge to
//! route.
//!
//! Unlike every other drawing here it has no size of its own. A map is
//! for seeing the whole, so it is laid out *for* the space it is given and
//! laid out again when that changes, and going closer is entering a
//! directory rather than moving a board. That is why what is selected is a
//! path and never an index: the next layout would give the index to
//! another tile.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{
    Host,
    view::{PanDirection, Role},
};

use super::{
    canvas::{self, Canvas, Corners, EAST, Frame, Glyphs, NORTH, SOUTH, WEST, text_width},
    model::Stroke,
    treemap,
};

/// The smallest tile that can carry a name: a border, a row, a border,
/// and enough columns for a few letters between two more.
const NAMED: (i32, i32) = (7, 3);
/// The smallest frame worth opening a directory in; under it the
/// directory is one tile.
const OPENED: (i32, i32) = (16, 5);
/// How many directories are opened inside one another. Every level costs
/// a border all round, and past a few the frames are most of what is
/// drawn; what is deeper is one tile, and entered.
const NESTED: usize = 3;
/// What marks the folded tile's path. No file is called this.
const FOLDED: &str = "\0folded";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileMeasure {
    /// Relative to the checkout, `/`-separated, as Git says it.
    pub path: String,
    pub lines: u32,
    /// Commits that touched it in the last year.
    pub commits: u32,
    /// Differs from what was last committed, or was never committed.
    pub changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Measure {
    pub root: PathBuf,
    pub files: Vec<FileMeasure>,
}

/// Measures the checkout `within` sits in. Three Git reads and nothing
/// else — but one of them opens every file, so this is for a thread.
///
/// `grep -c ''` is the line count: it matches every line of every text
/// file Git tracks or does not ignore, in one process, and `-I` is what
/// keeps binaries out. Asking the filesystem instead would need an ignore
/// parser to not measure a build directory.
pub fn measure(host: &dyn Host, within: &Path) -> Result<Measure, String> {
    let root = host.repository_root(within)?;
    let counted = host.git(&root, &["grep", "-I", "-c", "-z", "--untracked", ""], &[1])?;
    let touched = host.git(
        &root,
        &[
            "log",
            "--no-renames",
            "--format=",
            "--name-only",
            "-z",
            "--since=1.year",
        ],
        &[128],
    )?;
    let status = host.git(&root, &["status", "--porcelain", "-z"], &[])?;

    let mut commits: HashMap<&str, u32> = HashMap::new();
    for path in touched.split('\0').map(|path| path.trim_matches('\n')) {
        if !path.is_empty() {
            *commits.entry(path).or_default() += 1;
        }
    }
    let changed = changed_paths(&status);
    let files = counted
        .split('\n')
        .filter_map(|record| record.rsplit_once('\0'))
        .filter_map(|(path, lines)| Some((path, lines.trim().parse::<u32>().ok()?)))
        .map(|(path, lines)| FileMeasure {
            path: path.to_owned(),
            lines,
            commits: commits.get(path).copied().unwrap_or(0),
            changed: changed.contains(path),
        })
        .collect();
    Ok(Measure { root, files })
}

/// The paths of a `status --porcelain -z`. A rename is two records, the
/// second being where it came from — which is not a path that exists.
fn changed_paths(status: &str) -> HashSet<&str> {
    let mut changed = HashSet::new();
    let mut records = status.split('\0');
    while let Some(record) = records.next() {
        let Some((state, path)) = record.split_at_checked(3) else {
            continue;
        };
        changed.insert(path);
        if state.contains(['R', 'C']) {
            records.next();
        }
    }
    changed
}

/// How often a file changed, against the rest of the map.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Heat {
    Untouched,
    Touched,
    Warm,
    Hot,
}

impl Heat {
    fn role(self) -> Role {
        match self {
            Self::Untouched => Role::Faint,
            Self::Touched => Role::Muted,
            Self::Warm => Role::Warning,
            Self::Hot => Role::Danger,
        }
    }

    fn shade(self) -> u8 {
        match self {
            Self::Untouched | Self::Touched => 0,
            Self::Warm => 1,
            Self::Hot => 2,
        }
    }
}

/// Where the steps are, in commits. Ranked rather than scaled: one file
/// every commit touches would stretch a scale until it was the only one
/// with a colour.
#[derive(Clone, Copy, Debug, Default)]
struct Steps {
    hot: Option<u32>,
    warm: Option<u32>,
}

impl Steps {
    fn of(files: &[FileMeasure]) -> Self {
        let mut ranked: Vec<u32> = files
            .iter()
            .map(|file| file.commits)
            .filter(|&commits| commits > 0)
            .collect();
        ranked.sort_unstable_by(|a, b| b.cmp(a));
        let at = |share: usize| ranked.get(ranked.len() * share / 100).copied();
        Self {
            hot: at(5),
            warm: at(20),
        }
    }

    fn heat(self, commits: u32) -> Heat {
        match commits {
            0 => Heat::Untouched,
            _ if self.hot.is_some_and(|hot| commits >= hot) => Heat::Hot,
            _ if self.warm.is_some_and(|warm| commits >= warm) => Heat::Warm,
            _ => Heat::Touched,
        }
    }

    /// Several entries drawn as one tile: as hot as its lines are on
    /// average, so one busy file does not paint a quiet directory red.
    fn together(self, entries: &[Entry]) -> (u32, Heat) {
        let lines: u64 = entries.iter().map(|entry| entry.lines).sum();
        let weighed: u64 = entries
            .iter()
            .map(|entry| u64::from(entry.commits) * entry.lines)
            .sum();
        let commits = weighed.checked_div(lines).unwrap_or(0) as u32;
        let touched = entries.iter().any(|entry| entry.commits > 0);
        let heat = self.heat(commits.max(u32::from(touched)));
        (commits, heat)
    }
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    path: String,
    directory: bool,
    lines: u64,
    files: usize,
    commits: u32,
    changed: bool,
    heat: Heat,
    /// Largest first, which is the order a treemap is laid in — and what
    /// makes "the ones too small to name" always the tail.
    children: Vec<Entry>,
}

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<(String, FileMeasure)>,
}

impl Folder {
    fn entry(self, name: String, path: String, steps: Steps) -> Entry {
        let child_path = |child: &str| match path.is_empty() {
            true => child.to_owned(),
            false => format!("{path}/{child}"),
        };
        let mut children: Vec<Entry> = self
            .files
            .into_iter()
            .map(|(name, file)| Entry {
                path: child_path(&name),
                name,
                directory: false,
                lines: u64::from(file.lines),
                files: 1,
                commits: file.commits,
                changed: file.changed,
                heat: steps.heat(file.commits),
                children: Vec::new(),
            })
            .collect();
        for (folder_name, folder) in self.folders {
            let folder_path = child_path(&folder_name);
            children.push(folder.entry(folder_name, folder_path, steps));
        }
        children.retain(|child| child.lines > 0);
        children.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.name.cmp(&b.name)));

        // A directory holding one directory is a longer name, not a level:
        // drawn as two frames it spends four columns saying nothing.
        if !path.is_empty() && children.len() == 1 && children[0].directory {
            let mut only = children.remove(0);
            only.name = format!("{name}/{}", only.name);
            return only;
        }
        let (commits, heat) = steps.together(&children);
        Entry {
            name,
            path,
            directory: true,
            lines: children.iter().map(|child| child.lines).sum(),
            files: children.iter().map(|child| child.files).sum(),
            commits,
            changed: children.iter().any(|child| child.changed),
            heat,
            children,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TileKind {
    File,
    /// A directory, drawn around its contents or — too small — as a tile.
    Directory {
        opened: bool,
    },
    /// The tail of a directory, too small to name one by one: everything
    /// from child `from` on.
    Folded {
        from: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tile {
    pub frame: Frame,
    pub kind: TileKind,
    /// What it is selected by. A folded tile's is its directory's, marked.
    pub path: String,
    pub name: String,
    pub lines: u64,
    pub files: usize,
    pub commits: u32,
    pub changed: bool,
    pub heat: Heat,
}

/// One step closer: a directory, and how much of its head is left out —
/// which is how a folded tile is entered.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Step {
    directory: String,
    from: usize,
}

pub enum Followed {
    Nothing,
    /// The map is now of something smaller.
    Entered,
    /// A file, relative to the checkout.
    Open(String),
}

pub struct CodeMap {
    root: PathBuf,
    whole: Entry,
    steps: Steps,
    zoom: Vec<Step>,
    picked: Option<String>,
}

impl CodeMap {
    pub fn of(measure: Measure) -> Self {
        let steps = Steps::of(&measure.files);
        let mut top = Folder::default();
        for file in measure.files {
            let mut folder = &mut top;
            let mut parts: Vec<&str> = file.path.split('/').collect();
            let name = parts.pop().unwrap_or_default().to_owned();
            for part in parts {
                folder = folder.folders.entry(part.to_owned()).or_default();
            }
            folder.files.push((name, file));
        }
        Self {
            root: measure.root,
            whole: top.entry(String::new(), String::new(), steps),
            steps,
            zoom: Vec::new(),
            picked: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_empty(&self) -> bool {
        self.whole.children.is_empty()
    }

    /// As a fresh artifact is opened: the whole, nothing selected.
    pub fn reset(&mut self) {
        self.zoom.clear();
        self.picked = None;
    }

    fn entry(&self, path: &str) -> Option<&Entry> {
        let mut entry = &self.whole;
        while entry.path != path {
            entry = entry.children.iter().find(|child| {
                path == child.path
                    || path
                        .strip_prefix(child.path.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })?;
        }
        Some(entry)
    }

    /// What the map is of right now: a directory's children, from one on.
    fn showing(&self) -> (&Entry, usize) {
        self.zoom
            .last()
            .and_then(|step| Some((self.entry(&step.directory)?, step.from)))
            .unwrap_or((&self.whole, 0))
    }

    /// The way in, one name per step — what the menu's trail shows after
    /// the artifact's own name.
    pub fn crumbs(&self) -> Vec<String> {
        let mut above = "";
        self.zoom
            .iter()
            .filter_map(|step| {
                let entry = self.entry(&step.directory)?;
                // Each crumb says the way from the one before it, which is
                // the whole path when a directory was entered from the
                // middle of the map rather than from its parent.
                let name = match step.from {
                    0 => entry
                        .path
                        .strip_prefix(above)
                        .map_or(entry.path.as_str(), |rest| rest.trim_start_matches('/'))
                        .to_owned(),
                    from => folded_name(&entry.children[from.min(entry.children.len())..]),
                };
                above = &step.directory;
                Some(name)
            })
            .collect()
    }

    /// Every tile for a space, a directory before what is in it — so the
    /// last tile that contains a cell is the innermost one there.
    pub fn tiles(&self, space: (i32, i32)) -> Vec<Tile> {
        let mut tiles = Vec::new();
        let (entry, from) = self.showing();
        let within = Frame {
            x: 0,
            y: 0,
            w: space.0,
            h: space.1,
        };
        lay(entry, from, within, self.steps, NESTED, &mut tiles);
        tiles
    }

    pub fn paint(&self, space: (i32, i32), glyphs: Glyphs) -> Canvas {
        let mut canvas = Canvas::new(space.0, space.1, glyphs);
        for tile in self.tiles(space) {
            let picked = self.picked.as_deref() == Some(tile.path.as_str());
            paint_tile(&mut canvas, &tile, picked);
        }
        canvas
    }

    /// A click: selects, or — on what is already selected — follows.
    pub fn click(&mut self, x: i32, y: i32, space: (i32, i32)) -> Followed {
        let tiles = self.tiles(space);
        let Some(tile) = tiles.iter().rev().find(|tile| tile.frame.contains(x, y)) else {
            self.picked = None;
            return Followed::Nothing;
        };
        if self.picked.as_deref() == Some(tile.path.as_str()) {
            return self.follow(space);
        }
        self.picked = Some(tile.path.clone());
        Followed::Nothing
    }

    /// Into the selected directory, or out to the selected file.
    pub fn follow(&mut self, space: (i32, i32)) -> Followed {
        let tiles = self.tiles(space);
        let Some(tile) = tiles
            .iter()
            .find(|tile| self.picked.as_deref() == Some(tile.path.as_str()))
        else {
            return Followed::Nothing;
        };
        let step = match tile.kind {
            TileKind::File => return Followed::Open(tile.path.clone()),
            TileKind::Directory { .. } => Step {
                directory: tile.path.clone(),
                from: 0,
            },
            TileKind::Folded { from } => Step {
                directory: tile.path.trim_end_matches(FOLDED).to_owned(),
                from,
            },
        };
        self.zoom.push(step);
        self.picked = None;
        Followed::Entered
    }

    /// Back to `depth` steps in, with what was entered from there selected.
    pub fn back_to(&mut self, depth: usize) {
        if let Some(left) = self.zoom.get(depth).cloned() {
            self.zoom.truncate(depth);
            self.picked = Some(match left.from {
                0 => left.directory,
                _ => format!("{}{FOLDED}", left.directory),
            });
        }
    }

    pub fn is_zoomed(&self) -> bool {
        !self.zoom.is_empty()
    }

    /// Selects the tile that lies `direction` of the selected one, or the
    /// one nearest the middle when nothing is. An opened directory stands
    /// where its name is, since its middle belongs to what is in it.
    pub fn pick_toward(&mut self, direction: PanDirection, space: (i32, i32)) {
        let tiles = self.tiles(space);
        let stands_at = |tile: &Tile| match tile.kind {
            TileKind::Directory { opened: true } => (tile.frame.x + 2, tile.frame.y),
            _ => tile.frame.center(),
        };
        let picked = tiles
            .iter()
            .find(|tile| self.picked.as_deref() == Some(tile.path.as_str()));
        let from = picked.map_or((space.0 / 2, space.1 / 2), stands_at);
        let nearest = tiles
            .iter()
            .filter(|tile| Some(*tile) != picked)
            .filter_map(|tile| {
                let at = stands_at(tile);
                let (dx, dy) = (at.0 - from.0, (at.1 - from.1) * 2);
                let (along, across) = match direction {
                    PanDirection::Left => (-dx, dy),
                    PanDirection::Right => (dx, dy),
                    PanDirection::Up => (-dy, dx),
                    PanDirection::Down => (dy, dx),
                };
                (picked.is_none() || along > 0).then(|| (along.abs() + across.abs() * 2, tile))
            })
            .min_by_key(|&(distance, _)| distance)
            .map(|(_, tile)| tile.path.clone());
        if nearest.is_some() {
            self.picked = nearest;
        }
    }

    /// What the footer says: about the selection, or about the map.
    pub fn caption(&self, space: (i32, i32)) -> String {
        let tiles = self.tiles(space);
        let picked = tiles
            .iter()
            .find(|tile| self.picked.as_deref() == Some(tile.path.as_str()));
        let Some(tile) = picked else {
            let (entry, from) = self.showing();
            let shown = &entry.children[from.min(entry.children.len())..];
            return format!(
                "{} files · {} lines",
                grouped(shown.iter().map(|child| child.files as u64).sum()),
                grouped(shown.iter().map(|child| child.lines).sum()),
            );
        };
        let changed = if tile.changed { " · changed" } else { "" };
        match tile.kind {
            TileKind::File => format!(
                "{} · {} lines · {} this year{changed} · enter opens it",
                tile.path,
                grouped(tile.lines),
                commits(tile.commits),
            ),
            TileKind::Directory { .. } => format!(
                "{}/ · {} files · {} lines{changed} · enter goes inside",
                tile.path,
                grouped(tile.files as u64),
                grouped(tile.lines),
            ),
            TileKind::Folded { .. } => format!(
                "{} · {} lines{changed} · enter shows them",
                tile.name,
                grouped(tile.lines),
            ),
        }
    }

    /// The map as the table it is a picture of: every file, largest first.
    pub fn ranking(&self) -> String {
        let mut files = Vec::new();
        collect_files(&self.whole, &mut files);
        files.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.path.cmp(&b.path)));
        let mut table = format!("{:>9}  {:>7}  file\n", "lines", "commits");
        for file in files {
            let mark = if file.changed { " *" } else { "" };
            table.push_str(&format!(
                "{:>9}  {:>7}  {}{mark}\n",
                grouped(file.lines),
                file.commits,
                file.path
            ));
        }
        table
    }
}

fn collect_files<'a>(entry: &'a Entry, files: &mut Vec<&'a Entry>) {
    match entry.directory {
        true => entry
            .children
            .iter()
            .for_each(|child| collect_files(child, files)),
        false => files.push(entry),
    }
}

fn folded_name(folded: &[Entry]) -> String {
    let files: usize = folded.iter().map(|entry| entry.files).sum();
    format!("+{} files", grouped(files as u64))
}

fn commits(count: u32) -> String {
    match count {
        0 => "no commit".to_owned(),
        1 => "1 commit".to_owned(),
        count => format!("{count} commits"),
    }
}

/// `1234567` as `1,234,567`.
fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Lays `entry`'s children, from `from` on, into `within`.
fn lay(
    entry: &Entry,
    from: usize,
    within: Frame,
    steps: Steps,
    nested: usize,
    tiles: &mut Vec<Tile>,
) {
    let children = &entry.children[from.min(entry.children.len())..];
    if children.is_empty() || within.w < 1 || within.h < 1 {
        return;
    }
    let cells = f64::from(within.w * within.h);
    let total: u64 = children.iter().map(|child| child.lines).sum();
    let named = f64::from(NAMED.0 * NAMED.1);
    let share = |lines: u64| lines as f64 / total as f64 * cells;

    // What is too small to name is always the tail, children being largest
    // first. Where that is all of them, as many are kept as half the space
    // can name: a directory of nothing but small files would otherwise
    // fold into one tile that opens onto itself.
    let nameable = children
        .iter()
        .take_while(|child| share(child.lines) >= named)
        .count();
    let mut kept = match nameable {
        0 => ((cells / named / 2.0) as usize).clamp(1, children.len()),
        nameable => nameable,
    };
    if children.len() - kept < 2 {
        kept = children.len();
    }
    let (kept, folded) = children.split_at(kept);

    let mut weights: Vec<u64> = kept.iter().map(|child| child.lines).collect();
    if !folded.is_empty() {
        let lines: u64 = folded.iter().map(|child| child.lines).sum();
        // Large enough to say what it is, whatever it weighs.
        let legible = (named * 1.5 / cells * total as f64) as u64;
        weights.push(lines.max(legible));
    }
    let mut frames = treemap::squarified(&weights, within);
    // A share of the area is no promise of a shape, and the smallest
    // weight is the one that ends up alone in a sliver. The folded tile
    // has to be readable to be of any use, so it is given more until it is
    // — the one place the map trades proportion for legibility.
    for _ in 0..8 {
        let legible = frames
            .get(kept.len())
            .is_none_or(|frame| frame.w >= NAMED.0 && frame.h >= NAMED.1);
        if legible {
            break;
        }
        if let Some(weight) = weights.last_mut() {
            *weight = *weight * 3 / 2 + 1;
        }
        frames = treemap::squarified(&weights, within);
    }

    for (child, &frame) in kept.iter().zip(&frames) {
        let opened = child.directory && nested > 0 && frame.w >= OPENED.0 && frame.h >= OPENED.1;
        tiles.push(Tile {
            frame,
            kind: match child.directory {
                true => TileKind::Directory { opened },
                false => TileKind::File,
            },
            path: child.path.clone(),
            name: child.name.clone(),
            lines: child.lines,
            files: child.files,
            commits: child.commits,
            changed: child.changed,
            heat: child.heat,
        });
        if opened {
            let inside = Frame {
                x: frame.x + 1,
                y: frame.y + 1,
                w: frame.w - 2,
                h: frame.h - 2,
            };
            lay(child, 0, inside, steps, nested - 1, tiles);
        }
    }
    if let Some(&frame) = frames.get(kept.len()) {
        let (commits, heat) = steps.together(folded);
        tiles.push(Tile {
            frame,
            kind: TileKind::Folded {
                from: from + kept.len(),
            },
            path: format!("{}{FOLDED}", entry.path),
            name: folded_name(folded),
            lines: folded.iter().map(|child| child.lines).sum(),
            files: folded.iter().map(|child| child.files).sum(),
            commits,
            changed: folded.iter().any(|child| child.changed),
            heat,
        });
    }
}

fn paint_tile(canvas: &mut Canvas, tile: &Tile, picked: bool) {
    let Frame { x, y, w, h } = tile.frame;
    if w < 1 || h < 1 {
        return;
    }
    let glyphs = canvas.glyphs;
    let opened = tile.kind == TileKind::Directory { opened: true };
    let border = match (picked, opened) {
        (true, _) => Role::Accent,
        (_, true) => Role::Faint,
        _ => tile.heat.role(),
    };

    // Too thin for a frame: a line of the tile's own colour, so its area
    // is still on the map even though nothing can be said in it.
    if w < 2 || h < 2 {
        let sides = if h < 2 { EAST | WEST } else { NORTH | SOUTH };
        for row in y..y + h {
            for column in x..x + w {
                canvas.line(column, row, sides, Stroke::Solid, border);
            }
        }
        return;
    }

    if !opened {
        let shade = canvas::shade_glyph(tile.heat.shade(), glyphs);
        for row in y + 1..y + h - 1 {
            for column in x + 1..x + w - 1 {
                canvas.put(column, row, shade, tile.heat.role(), false);
            }
        }
    }
    let corners = match tile.kind {
        TileKind::File => Corners::Rounded,
        _ => Corners::Square,
    };
    canvas.frame(tile.frame, corners, border);

    let name = match tile.kind {
        TileKind::Directory { .. } => format!("{}/", tile.name),
        _ => tile.name.clone(),
    };
    let mark = match tile.changed {
        true => format!(" {}", canvas::changed_glyph(glyphs)),
        false => String::new(),
    };
    if opened {
        let room = w - 6 - text_width(&mark);
        if room >= 2 {
            let title = format!(" {}{mark} ", canvas::fitted(&name, room, glyphs));
            let role = if picked {
                Role::Accent
            } else {
                Role::Secondary
            };
            canvas.text(x + 2, y, &title, role, true);
        }
        return;
    }
    let room = w - 4;
    if room < 3 || h < NAMED.1 {
        return;
    }
    let (role, bold) = match (picked, tile.kind, tile.heat) {
        (true, ..) => (Role::Accent, true),
        (_, TileKind::Folded { .. }, _) => (Role::Dim, false),
        (_, _, Heat::Untouched) => (Role::Muted, false),
        (_, _, Heat::Touched) => (Role::Bright, false),
        (_, _, heat) => (heat.role(), true),
    };
    let mark = if text_width(&name) + text_width(&mark) <= room {
        mark
    } else {
        String::new()
    };
    let name = canvas::fitted(&name, room - text_width(&mark), glyphs);
    canvas.text(x + 1, y + 1, &format!(" {name}{mark} "), role, bold);
    if h >= 4 {
        let lines = canvas::fitted(&grouped(tile.lines), room, glyphs);
        canvas.text(x + 1, y + 2, &format!(" {lines} "), Role::Dim, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, lines: u32, commits: u32) -> FileMeasure {
        FileMeasure {
            path: path.to_owned(),
            lines,
            commits,
            changed: false,
        }
    }

    fn map(files: Vec<FileMeasure>) -> CodeMap {
        CodeMap::of(Measure {
            root: PathBuf::from("/project"),
            files,
        })
    }

    const SPACE: (i32, i32) = (120, 36);

    fn tile<'a>(tiles: &'a [Tile], path: &str) -> &'a Tile {
        tiles
            .iter()
            .find(|tile| tile.path == path)
            .unwrap_or_else(|| panic!("no tile for {path}"))
    }

    #[test]
    fn a_large_file_is_a_large_tile_and_a_hot_one_a_hot_tile() {
        let mut files = vec![file("src/big.rs", 5000, 0), file("src/busy.rs", 200, 90)];
        files.extend((0..20).map(|n| file(&format!("src/f{n}.rs"), 300, 1 + n % 3)));
        let tiles = map(files).tiles(SPACE);
        let (big, busy) = (tile(&tiles, "src/big.rs"), tile(&tiles, "src/busy.rs"));
        assert!(big.frame.w * big.frame.h > busy.frame.w * busy.frame.h * 5);
        assert_eq!((big.heat, busy.heat), (Heat::Untouched, Heat::Hot));
    }

    #[test]
    fn a_chain_of_single_directories_is_one_frame() {
        let map = map(vec![
            file("crates/core/src/lib.rs", 400, 1),
            file("crates/core/src/store.rs", 300, 1),
            file("README.md", 300, 1),
        ]);
        let tiles = map.tiles(SPACE);
        let chain = tile(&tiles, "crates/core/src");
        assert_eq!(chain.name, "crates/core/src");
        assert!(tiles.iter().all(|tile| tile.path != "crates"));
        assert_eq!(
            map.entry("crates/core/src/lib.rs").map(|e| e.lines),
            Some(400)
        );
    }

    #[test]
    fn what_is_too_small_to_name_is_folded_into_one_tile_that_can_be_entered() {
        let mut files = vec![
            file("a.rs", 4000, 1),
            file("b.rs", 3000, 1),
            file("c.rs", 2000, 1),
        ];
        files.extend((0..40).map(|n| file(&format!("tiny{n:02}.rs"), 1, 0)));
        let mut map = map(files);
        let tiles = map.tiles((60, 16));
        let folded = tiles
            .iter()
            .find(|tile| matches!(tile.kind, TileKind::Folded { .. }))
            .expect("the forty are one tile");
        assert_eq!((folded.name.as_str(), folded.files), ("+40 files", 40));
        assert!(folded.frame.w >= NAMED.0 && folded.frame.h >= NAMED.1);
        assert_eq!(tiles.len(), 4);

        map.picked = Some(folded.path.clone());
        assert!(matches!(map.follow((60, 16)), Followed::Entered));
        assert_eq!(map.crumbs(), ["+40 files"]);
        let inside = map.tiles((60, 16));
        assert!(inside.iter().all(|tile| tile.name != "a.rs"));
        assert!(inside.iter().any(|tile| tile.name == "tiny00.rs"));

        map.back_to(0);
        assert_eq!(map.picked, Some(folded.path.clone()), "back where it stood");
    }

    #[test]
    fn a_directory_of_nothing_but_small_files_does_not_fold_onto_itself() {
        let mut map = map((0..400)
            .map(|n| file(&format!("f{n:03}.rs"), 10, 0))
            .collect());
        let mut shown = usize::MAX;
        for _ in 0..40 {
            let tiles = map.tiles((60, 16));
            let Some(folded) = tiles
                .iter()
                .find(|tile| matches!(tile.kind, TileKind::Folded { .. }))
            else {
                return;
            };
            assert!(folded.files < shown, "entering it has to get somewhere");
            shown = folded.files;
            map.picked = Some(folded.path.clone());
            map.follow((60, 16));
        }
        panic!("forty levels in and still folding");
    }

    #[test]
    fn the_map_fills_whatever_space_it_is_given() {
        let map = map(vec![
            file("src/a.rs", 900, 1),
            file("src/b.rs", 500, 1),
            file("docs/c.md", 400, 1),
        ]);
        for space in [(120, 36), (71, 19)] {
            let tiles = map.tiles(space);
            let top: i32 = tiles
                .iter()
                .filter(|tile| tile.path == "src" || tile.path == "docs")
                .map(|tile| tile.frame.w * tile.frame.h)
                .sum();
            assert_eq!(top, space.0 * space.1, "at {space:?}");
        }
    }

    #[test]
    fn entering_a_directory_shows_it_alone_and_a_file_is_opened() {
        let mut map = map(vec![
            file("src/ui/render.rs", 900, 1),
            file("src/ui/input.rs", 500, 1),
            file("src/main.rs", 300, 1),
            file("docs/guide.md", 800, 1),
        ]);
        map.picked = Some("src".to_owned());
        assert!(matches!(map.follow(SPACE), Followed::Entered));
        assert_eq!(map.crumbs(), ["src"]);
        let tiles = map.tiles(SPACE);
        assert!(tiles.iter().all(|tile| tile.path.starts_with("src/")));

        map.picked = Some("src/main.rs".to_owned());
        assert!(matches!(map.follow(SPACE), Followed::Open(path) if path == "src/main.rs"));

        map.back_to(0);
        assert!(!map.is_zoomed());
        assert_eq!(map.picked.as_deref(), Some("src"));
    }

    #[test]
    fn a_second_click_follows_and_a_click_on_nothing_lets_go() {
        let mut map = map(vec![file("a.rs", 500, 1), file("b.rs", 400, 1)]);
        let frame = tile(&map.tiles(SPACE), "a.rs").frame;
        let (x, y) = frame.center();
        assert!(matches!(map.click(x, y, SPACE), Followed::Nothing));
        assert_eq!(map.picked.as_deref(), Some("a.rs"));
        assert!(matches!(map.click(x, y, SPACE), Followed::Open(_)));
        map.click(-1, -1, SPACE);
        assert_eq!(map.picked, None);
    }

    #[test]
    fn the_arrows_walk_the_tiles() {
        let mut map = map(vec![file("a.rs", 500, 1), file("b.rs", 500, 1)]);
        map.pick_toward(PanDirection::Right, SPACE);
        let first = map.picked.clone().expect("the one nearest the middle");
        map.pick_toward(PanDirection::Right, SPACE);
        map.pick_toward(PanDirection::Left, SPACE);
        let tiles = map.tiles(SPACE);
        let left = tiles
            .iter()
            .min_by_key(|tile| tile.frame.x)
            .map(|t| &t.path);
        assert_eq!(map.picked.as_ref(), left);
        assert!(first == "a.rs" || first == "b.rs");
    }

    #[test]
    fn the_ascii_rendering_is_ascii() {
        let mut files = vec![file("src/busy.rs", 900, 50)];
        files.extend((0..12).map(|n| FileMeasure {
            changed: n == 0,
            ..file(&format!("src/a-rather-long-file-name-{n}.rs"), 300, n)
        }));
        let text = map(files).paint(SPACE, Glyphs::Ascii).to_text();
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains('#'), "the hottest tile is shaded:\n{text}");
    }

    #[test]
    fn the_ranking_lists_every_file_largest_first() {
        let ranking = map(vec![
            file("small.rs", 10, 0),
            file("src/large.rs", 12345, 7),
        ])
        .ranking();
        let rows: Vec<&str> = ranking.lines().collect();
        assert!(rows[1].contains("12,345") && rows[1].ends_with("src/large.rs"));
        assert!(rows[2].ends_with("small.rs"));
    }

    struct Repository;

    impl Host for Repository {
        fn git(&self, _: &Path, args: &[&str], _: &[i32]) -> Result<String, String> {
            Ok(match args[0] {
                "grep" => "src/main.rs\x00120\nsrc/new name.rs\x0040\nnotes.md\x007\n",
                "log" => "src/main.rs\0notes.md\0\nsrc/main.rs\0",
                _ => "R  src/new name.rs\0src/old.rs\0?? notes.md\0",
            }
            .to_owned())
        }
        fn repository_root(&self, _: &Path) -> Result<PathBuf, String> {
            Ok(PathBuf::from("/project"))
        }
        fn read_file(&self, _: &Path) -> Result<String, String> {
            Err("not asked".to_owned())
        }
        fn list_dir(&self, _: &Path) -> Result<Vec<crate::DirEntry>, String> {
            Err("not asked".to_owned())
        }
        fn write_file(&self, _: &Path, _: &str) -> Result<(), String> {
            Err("not asked".to_owned())
        }
        fn delete_file(&self, _: &Path) -> Result<(), String> {
            Err("not asked".to_owned())
        }
        fn syntax_theme(&self) -> String {
            String::new()
        }
    }

    #[test]
    fn a_checkout_is_measured_from_what_git_says() {
        let measure = measure(&Repository, Path::new("/project/src")).expect("a repository");
        assert_eq!(measure.root, PathBuf::from("/project"));
        assert_eq!(
            measure.files,
            [
                FileMeasure {
                    path: "src/main.rs".to_owned(),
                    lines: 120,
                    commits: 2,
                    changed: false
                },
                FileMeasure {
                    path: "src/new name.rs".to_owned(),
                    lines: 40,
                    commits: 0,
                    changed: true
                },
                FileMeasure {
                    path: "notes.md".to_owned(),
                    lines: 7,
                    commits: 1,
                    changed: true
                },
            ],
            "a rename's origin is not a changed path"
        );
    }
}
