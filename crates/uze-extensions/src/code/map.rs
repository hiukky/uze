//! The checkout as a map: where its lines are, and which of them are hot.
//!
//! The one artifact nobody writes. It is measured rather than read — a
//! file's lines, how many commits touched it this year, whether it differs
//! from what was committed — and drawn as a treemap, which is the diagram
//! a grid of cells is best at: rectangles, and no edge to route.
//!
//! **One level at a time.** A treemap that opens directories inside
//! directories spends most of its ink on borders and most of its names on
//! three letters and a cut. So a level draws its own children and nothing
//! below them — each tile large enough to carry a whole name and what it
//! measures — and a directory is *entered* rather than unfolded. How many
//! tiles are on the screen is then a property of the directory being
//! looked at, never of the repository's size, which is what makes the map
//! read the same in a project of ten files and one of ten thousand.
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

use crate::shared::canvas::{
    self, Canvas, Corners, EAST, Frame, Glyphs, NORTH, SOUTH, Stroke, WEST, text_width,
};

use super::treemap;

/// The smallest tile that can carry a name: a border, a row, a border,
/// and enough columns for a name rather than three letters and a cut.
const NAMED: (i32, i32) = (13, 3);
/// What a tile gives back on each side where a neighbour begins, so that
/// no two of them draw on one cell. See [`bordered`].
const GAP: i32 = 1;
/// From this height on a tile says how many lines it holds, and from the
/// next one what it is made of — a row per measure, under the name.
const COUNTED: i32 = 4;
const DETAILED: i32 = 5;
/// How many tiles one level is drawn as, at most. A map is read by
/// comparing what is on it, and past a couple of dozen rectangles nobody
/// compares; the tail is one tile, and entered.
const MOST: usize = 20;
/// How many times a level is laid out again with one tile fewer, looking
/// for a split where every tile kept can carry its name.
const NARROWINGS: usize = 16;
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
    /// A directory, drawn as a tile whatever is in it: what is inside is
    /// seen by going inside.
    Directory,
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

pub struct Map {
    whole: Entry,
    steps: Steps,
    zoom: Vec<Step>,
    picked: Option<String>,
}

impl Map {
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
            whole: top.entry(String::new(), String::new(), steps),
            steps,
            zoom: Vec::new(),
            picked: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.whole.children.is_empty()
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

    /// The tiles of the level on show, in a space. One per child, none
    /// inside another, so a cell belongs to exactly one of them.
    pub fn tiles(&self, space: (i32, i32)) -> Vec<Tile> {
        let mut tiles = Vec::new();
        let (entry, from) = self.showing();
        let within = Frame {
            x: 0,
            y: 0,
            w: space.0,
            h: space.1,
        };
        lay(entry, from, within, self.steps, &mut tiles);
        tiles
    }

    /// The borders are gathered into one grid before any of them is
    /// drawn, so two tiles side by side share a single line and meet the
    /// ones above and below at a junction. Drawn a box at a time they
    /// would stand one beside the other, and a dozen of them read as a
    /// stack of boxes rather than as a map.
    pub fn paint(&self, space: (i32, i32), glyphs: Glyphs) -> Canvas {
        let mut canvas = Canvas::new(space.0, space.1, glyphs);
        let tiles = self.tiles(space);
        let picked = |tile: &Tile| self.picked.as_deref() == Some(tile.path.as_str());
        let mut grid = Grid::new(space);
        for tile in &tiles {
            let role = match picked(tile) {
                true => Role::Accent,
                false => tile.heat.role(),
            };
            grid.outline(bordered(tile.frame, space), role, picked(tile));
        }
        grid.draw(&mut canvas);
        for tile in &tiles {
            label(&mut canvas, tile, bordered(tile.frame, space), picked(tile));
        }
        canvas
    }

    /// Lets go of the selected tile, answering whether there was one.
    pub fn let_go(&mut self) -> bool {
        self.picked.take().is_some()
    }

    /// A click: selects, or — on what is already selected — follows.
    pub fn click(&mut self, x: i32, y: i32, space: (i32, i32)) -> Followed {
        let tiles = self.tiles(space);
        let Some(tile) = tiles.iter().find(|tile| tile.frame.contains(x, y)) else {
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
            TileKind::Directory => Step {
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

    /// Selects the tile that lies `direction` of the selected one, or the
    /// one nearest the middle when nothing is. An opened directory stands
    /// where its name is, since its middle belongs to what is in it.
    pub fn pick_toward(&mut self, direction: PanDirection, space: (i32, i32)) {
        let tiles = self.tiles(space);
        let stands_at = |tile: &Tile| tile.frame.center();
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
            TileKind::Directory => format!(
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

/// Lays `entry`'s children, from `from` on, into `within` — and stops
/// there. What is inside a child is not drawn inside its tile; it is what
/// the next level shows.
fn lay(entry: &Entry, from: usize, within: Frame, steps: Steps, tiles: &mut Vec<Tile>) {
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
    // fold into one tile that opens onto itself. Either way no more than
    // a screenful of tiles, so the map is the same size in every project.
    let nameable = children
        .iter()
        .take_while(|child| share(child.lines) >= named)
        .count();
    let mut keeping = match nameable {
        0 => ((cells / named / 2.0) as usize).clamp(1, children.len()),
        nameable => nameable,
    }
    .min(MOST);
    if children.len() - keeping < 2 {
        keeping = children.len();
    }

    // A share of the area is no promise of a shape: the smallest weights
    // are the ones that end up in a sliver. A tile nobody can read the
    // name of says less than one more name under "the rest", so the split
    // is tried, looked at, and tried again one tile smaller until every
    // tile that was kept can say what it is.
    for _ in 0..NARROWINGS {
        let frames = frames_for(children, keeping, within, cells, total);
        let cramped = frames.iter().take(keeping).position(|f| !fits_a_name(f));
        let Some(index) = cramped.filter(|&index| index > 0) else {
            break;
        };
        keeping = index;
        // Leaving exactly one child out would fold it into a tile reading
        // "+1 files", which is a worse name than the one it already has.
        if children.len() - keeping == 1 && keeping > 1 {
            keeping -= 1;
        }
    }
    let (kept, folded) = children.split_at(keeping);
    let frames = frames_for(children, keeping, within, cells, total);

    for (child, &frame) in kept.iter().zip(&frames) {
        tiles.push(Tile {
            frame,
            kind: match child.directory {
                true => TileKind::Directory,
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

/// Room for a name, a measure under it, and a border either side.
///
/// Measured on what the tile is *drawn* as, not on what it was laid out
/// as: a tile gives a cell back on each side where it has a neighbour to
/// clear (see [`bordered`]), and a threshold that did not know it was
/// promising room the name would not get.
fn fits_a_name(frame: &Frame) -> bool {
    frame.w - GAP >= NAMED.0 && frame.h - GAP >= NAMED.1
}

/// The frames for a split at `keeping`: what is kept, and — unless
/// nothing was left out — the tile the rest is folded into. That last one
/// is given more area until it is readable, whatever it weighs: it is of
/// no use unless it can say how many files it stands for, and it is the
/// one place the map trades proportion for legibility.
fn frames_for(
    children: &[Entry],
    keeping: usize,
    within: Frame,
    cells: f64,
    total: u64,
) -> Vec<Frame> {
    let (kept, folded) = children.split_at(keeping.min(children.len()));
    let mut weights: Vec<u64> = kept.iter().map(|child| child.lines).collect();
    if !folded.is_empty() {
        let named = f64::from(NAMED.0 * NAMED.1);
        let lines: u64 = folded.iter().map(|child| child.lines).sum();
        let legible = (named * 1.5 / cells * total as f64) as u64;
        weights.push(lines.max(legible));
    }
    let mut frames = treemap::squarified(&weights, within);
    for _ in 0..8 {
        if frames.get(kept.len()).is_none_or(fits_a_name) {
            break;
        }
        match weights.last_mut() {
            Some(weight) => *weight = *weight * 3 / 2 + 1,
            None => break,
        }
        frames = treemap::squarified(&weights, within);
    }
    frames
}

/// The rectangle a tile's border is drawn on: its own, one cell short of
/// wherever a neighbour begins — except at the map's own edge, where
/// there is none to clear and the tile runs flush to it.
///
/// Sharing the line instead put two tiles' borders on one cell, and a
/// cell has one colour: a run they shared came out in whichever of the
/// two claimed it, so a long edge between a red tile and a grey one was
/// red for part of its length and grey for the rest — read as a
/// rendering fault, which is the one thing a map of a checkout must not
/// look like. A tile still answers for every cell it was laid on, so the
/// gap is the tile's to click, not a seam between two.
fn bordered(frame: Frame, space: (i32, i32)) -> Frame {
    let clears_right = i32::from(frame.x + frame.w < space.0) * GAP;
    let clears_bottom = i32::from(frame.y + frame.h < space.1) * GAP;
    Frame {
        x: frame.x,
        y: frame.y,
        w: (frame.w - clears_right).max(1),
        h: (frame.h - clears_bottom).max(1),
    }
}

/// Every border on the map, by cell: which sides meet there, and in
/// whose colour. The selected tile claims the cells it shares, so its
/// outline is unbroken however many neighbours it has.
struct Grid {
    width: i32,
    height: i32,
    cells: Vec<(u8, Role, bool)>,
}

impl Grid {
    fn new((width, height): (i32, i32)) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        Self {
            width,
            height,
            cells: vec![(0, Role::Default, false); (width * height) as usize],
        }
    }

    fn add(&mut self, x: i32, y: i32, sides: u8, role: Role, picked: bool) {
        if sides == 0 || x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let cell = &mut self.cells[(y * self.width + x) as usize];
        cell.0 |= sides;
        if picked || !cell.2 {
            cell.1 = role;
            cell.2 |= picked;
        }
    }

    fn outline(&mut self, frame: Frame, role: Role, picked: bool) {
        let Frame { x, y, w, h } = frame;
        if w < 1 || h < 1 {
            return;
        }
        let (right, bottom) = (x + w - 1, y + h - 1);
        for column in x..=right {
            let mut along = 0;
            if column > x {
                along |= WEST;
            }
            if column < right {
                along |= EAST;
            }
            let corner = column == x || column == right;
            let down = if corner && bottom > y { SOUTH } else { 0 };
            let up = if corner { NORTH } else { 0 };
            self.add(column, y, along | down, role, picked);
            if bottom > y {
                self.add(column, bottom, along | up, role, picked);
            }
        }
        for row in y + 1..bottom {
            self.add(x, row, NORTH | SOUTH, role, picked);
            self.add(right, row, NORTH | SOUTH, role, picked);
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        for y in 0..self.height {
            for x in 0..self.width {
                let (sides, role, _) = self.cells[(y * self.width + x) as usize];
                if sides != 0 {
                    let glyph =
                        canvas::line_glyph(sides, Stroke::Solid, canvas.glyphs, Corners::Square);
                    canvas.put(x, y, glyph, role, false);
                }
            }
        }
    }
}

/// What a tile says inside its border: its name, and under it a measure
/// per row as far down as there is room.
fn label(canvas: &mut Canvas, tile: &Tile, frame: Frame, picked: bool) {
    let Frame { x, y, w, h } = frame;
    let glyphs = canvas.glyphs;
    let room = w - 4;
    if room < 3 || h < NAMED.1 {
        return;
    }
    let name = match tile.kind {
        TileKind::Directory => format!("{}/", tile.name),
        _ => tile.name.clone(),
    };
    let (role, bold) = match (picked, tile.kind, tile.heat) {
        (true, ..) => (Role::Accent, true),
        (_, TileKind::Folded { .. }, _) => (Role::Dim, false),
        (_, _, Heat::Untouched) => (Role::Muted, false),
        (_, _, Heat::Touched) => (Role::Bright, false),
        (_, _, heat) => (heat.role(), true),
    };
    // The mark outlives the name: a name is cut to make room for it,
    // because what changed is the one thing a reader came for.
    let mark = match tile.changed {
        true => format!(" {}", canvas::changed_glyph(glyphs)),
        false => String::new(),
    };
    let name = canvas::fitted(&name, room - text_width(&mark), glyphs);
    canvas.text(x + 2, y + 1, &format!("{name}{mark}"), role, bold);

    if h >= COUNTED {
        let lines = canvas::fitted(&format!("{} lines", grouped(tile.lines)), room, glyphs);
        canvas.text(x + 2, y + 2, &lines, Role::Dim, false);
    }
    if h >= DETAILED
        && let Some(detail) = detail(tile)
    {
        let detail = canvas::fitted(&detail, room, glyphs);
        canvas.text(x + 2, y + 3, &detail, Role::Faint, false);
    }
}

/// The third row: what the tile is made of. A folded tile has none —
/// its name is already the count.
fn detail(tile: &Tile) -> Option<String> {
    match tile.kind {
        TileKind::Directory => Some(format!("{} files", grouped(tile.files as u64))),
        TileKind::File => {
            (tile.commits > 0).then(|| format!("{} this year", commits(tile.commits)))
        }
        TileKind::Folded { .. } => None,
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

    fn map(files: Vec<FileMeasure>) -> Map {
        Map::of(Measure {
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
        let mut files = vec![file("big.rs", 5000, 0), file("busy.rs", 400, 90)];
        files.extend((0..20).map(|n| file(&format!("f{n}.rs"), 300, 1 + n % 3)));
        let tiles = map(files).tiles(SPACE);
        let (big, busy) = (tile(&tiles, "big.rs"), tile(&tiles, "busy.rs"));
        assert!(big.frame.w * big.frame.h > busy.frame.w * busy.frame.h * 5);
        assert_eq!((big.heat, busy.heat), (Heat::Untouched, Heat::Hot));
    }

    /// No two tiles draw on one cell.
    ///
    /// A cell has one colour, so a border two tiles shared came out in
    /// whichever of them claimed it: a long edge between a red tile and
    /// a grey one was red for part of its length and grey for the rest,
    /// which reads as a rendering fault rather than as two tiles.
    #[test]
    fn two_tiles_never_draw_on_the_same_cell() {
        let mut files = vec![file("Cargo.lock", 3799, 2), file("AGENTS.md", 620, 9)];
        files.extend((0..6).map(|n| file(&format!("crates/m{n}.rs"), 1400 + n * 300, n)));
        files.extend((0..4).map(|n| file(&format!("src/u{n}.rs"), 2400, 20 + n)));
        files.extend((0..9).map(|n| file(&format!("docs/{n:03}.md"), 380, 1)));
        let map = map(files);

        for space in [(70, 20), SPACE, (150, 38)] {
            let drawn: Vec<Frame> = map
                .tiles(space)
                .iter()
                .map(|tile| bordered(tile.frame, space))
                .collect();
            for (index, one) in drawn.iter().enumerate() {
                for other in &drawn[index + 1..] {
                    let apart = one.x + one.w <= other.x
                        || other.x + other.w <= one.x
                        || one.y + one.h <= other.y
                        || other.y + other.h <= one.y;
                    assert!(apart, "{one:?} and {other:?} overlap in {space:?}");
                }
            }
            // And nothing is given up at the map's own edge, where there
            // is no neighbour to clear.
            assert!(
                drawn.iter().any(|frame| frame.x + frame.w == space.0),
                "the map reaches its right edge in {space:?}"
            );
            assert!(
                drawn.iter().any(|frame| frame.y + frame.h == space.1),
                "and its bottom one in {space:?}"
            );
        }
    }

    #[test]
    fn a_level_draws_its_own_children_and_a_directory_is_entered_to_see_inside() {
        let mut files = vec![file("README.md", 4000, 1)];
        files.extend((0..8).map(|n| file(&format!("src/ui/w{n}.rs"), 900, n)));
        files.extend((0..8).map(|n| file(&format!("src/core/c{n}.rs"), 700, n)));
        let mut map = map(files);

        let tiles = map.tiles(SPACE);
        let paths: Vec<&str> = tiles.iter().map(|tile| tile.path.as_str()).collect();
        assert_eq!(paths, ["src", "README.md"], "nothing below them is drawn");
        for (index, tile) in tiles.iter().enumerate() {
            for other in &tiles[index + 1..] {
                let (x, y) = other.frame.center();
                assert!(!tile.frame.contains(x, y), "no tile sits inside another");
            }
        }

        map.picked = Some("src".to_owned());
        assert!(matches!(map.follow(SPACE), Followed::Entered));
        let inside: Vec<String> = map.tiles(SPACE).into_iter().map(|tile| tile.path).collect();
        assert_eq!(inside, ["src/ui", "src/core"], "one level closer");
    }

    #[test]
    fn every_tile_on_a_level_is_large_enough_to_carry_its_name() {
        let mut files = vec![
            file("Cargo.lock", 3799, 2),
            file("AGENTS.md", 620, 9),
            file("README.md", 300, 4),
        ];
        files.extend((0..14).map(|n| file(&format!("crates/core/m{n}.rs"), 1400 + n * 60, n)));
        files.extend((0..7).map(|n| file(&format!("src/ui/u{n}.rs"), 2400, 20 + n)));
        files.extend((0..40).map(|n| file(&format!("docs/adr/{n:03}.md"), 180, 1)));
        for space in [(150, 38), SPACE, (80, 24)] {
            for tile in map(files.clone()).tiles(space) {
                assert!(
                    fits_a_name(&tile.frame),
                    "{} drawn as {:?} in {space:?}",
                    tile.name,
                    tile.frame
                );
            }
        }
    }

    #[test]
    fn a_level_is_never_more_than_a_screenful_of_tiles() {
        let map = map((0..200)
            .map(|n| file(&format!("f{n:03}.rs"), 400 + n, 0))
            .collect());
        let tiles = map.tiles((200, 60));
        assert!(
            tiles.len() <= MOST + 1,
            "{} tiles on a level of 200 files",
            tiles.len()
        );
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
        assert!(map.crumbs().is_empty(), "back at the top");
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
        let mut files = vec![file("busy.rs", 900, 50)];
        files.extend((0..12).map(|n| FileMeasure {
            changed: n == 0,
            ..file(&format!("a-rather-long-file-name-{n}.rs"), 300, n)
        }));
        let text = map(files).paint(SPACE, Glyphs::Ascii).to_text();
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains('*'), "what changed is marked:\n{text}");
        assert!(text.contains("lines"), "a tile says what it holds:\n{text}");
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
