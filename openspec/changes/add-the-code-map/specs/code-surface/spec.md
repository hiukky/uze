## ADDED Requirements

### Requirement: The checkout is drawn as a map nobody has to write
The code surface SHALL offer, beside its file tree, a map of the active
checkout: a treemap in which a file's area is proportional to its line
count, and a directory with a single directory in it is drawn as one tile
named for both. The map SHALL cover the text files Git tracks and the
untracked ones it does not ignore, and SHALL leave out binary files. It
SHALL require no file, no manifest key and no declared artifact. Where
the checkout has not been measured — outside a Git repository, or before
the measurement arrives — the map SHALL NOT be offered.

The map SHALL be offered only where the file tree is what the surface is
navigating. The changes half SHALL NOT offer it, by chip or by key: a
map of the whole checkout answers a question nobody reviewing a change is
asking.

#### Scenario: Beside the tree, and not beside the diff
- **WHEN** the surface is showing the changes of a measured checkout
- **THEN** the map SHALL NOT be among the ways of showing it
- **AND THEN** the key that toggles it SHALL leave the changes on show

#### Scenario: An ignored directory
- **WHEN** the repository ignores `target/`
- **THEN** no tile SHALL stand for anything under `target/`

#### Scenario: A chain of single directories
- **WHEN** `crates/` holds only `core/`, which holds only `src/`
- **THEN** one frame titled `crates/core/src` SHALL be drawn, not three

#### Scenario: Not a repository
- **WHEN** the surface is opened outside a Git repository
- **THEN** no `Code` area SHALL be listed

### Requirement: The map fits the view, and is entered rather than moved
The map SHALL take the whole frame while it is on show — it is a picture
of the whole checkout, and the tree it replaces is what leaving it goes
back to. It SHALL be laid out for the space it has, and laid out again
when that space changes; it SHALL NOT be panned. Tiles SHALL be laid out
so they stay close to square as seen, a cell being about twice as tall as
it is wide.

**One level at a time.** A level SHALL draw its own children and nothing
below them: no directory is drawn around its contents, so how many tiles
are on the screen is a property of the directory being looked at and
never of the repository's size. Every tile SHALL be large enough to carry
its own name; where the split would leave one that is not, it SHALL be
made again one tile smaller until every tile kept can. What is left out
SHALL be folded into one tile that says how many files it stands for.
Borders SHALL be drawn as one grid, so two tiles side by side share a
line rather than standing one beside the other.

Activating a selected directory, or a folded tile, SHALL enter it: the
map is laid out again for that directory alone, and the trail SHALL show
the way in, beginning with the checkout. A click on a step of the trail
SHALL go back to that level, with the directory that was entered
selected. The key that closes SHALL let go of the selection, then leave
the level, and only with neither left SHALL it leave the map.

#### Scenario: The terminal is resized
- **WHEN** the view becomes narrower while the map is shown
- **THEN** the map SHALL be laid out again to fill exactly the new space

#### Scenario: Many small files
- **WHEN** a directory holds three large files and forty one-line files
- **THEN** the three SHALL be drawn and named
- **AND THEN** one tile SHALL read that it stands for the other forty

#### Scenario: A tile too thin for its name
- **WHEN** a child's share of the area would be drawn as a sliver
- **THEN** it SHALL be folded rather than drawn without a name

#### Scenario: Entering a directory
- **WHEN** the directory `crates` is selected and activated
- **THEN** the map SHALL show the contents of `crates` alone, filling the
  view
- **AND THEN** the trail SHALL end in `crates`

#### Scenario: A directory holding another directory
- **WHEN** `src/` holds `ui/`, which holds seven files
- **THEN** `src/` SHALL be one tile, with nothing of `ui/` drawn inside it

#### Scenario: Entering a folded tile
- **WHEN** a folded tile is activated
- **THEN** the map SHALL show only the files it stood for

#### Scenario: Coming back
- **WHEN** `level-up` is triggered inside `crates`
- **THEN** the whole map SHALL be shown with `crates` selected

#### Scenario: The arrows
- **WHEN** a pan action is triggered on the code map
- **THEN** the selection SHALL move to the nearest tile in that direction

### Requirement: The map says which files are large and which are hot
A file's tile SHALL be coloured by how many commits touched it in the last
year, ranked against the other files of the map in four steps from none to
hottest, using semantic roles only. A directory drawn as one tile SHALL
take the step weighted by its files' lines. A file with uncommitted
changes SHALL be marked, and that mark SHALL survive a name too long to
fit. A tile SHALL say its own name and what it holds — its lines, and the
files under it or the commits that touched it — as far down as there is
room. Selecting a tile SHALL name its path, its lines, its commits and
whether it is changed in the heading; with nothing selected the heading
SHALL give the level's files and lines. The map SHALL be offered in three
renderings on one control — the map, the same map in ASCII, and the
ranking itself: every file by lines, largest first, with its commits.

#### Scenario: A large file nobody touches and a small one everybody does
- **WHEN** the map holds a 5,000-line file with no commit this year and a
  200-line file with the most commits of any
- **THEN** the first SHALL be the larger tile in the coldest step
- **AND THEN** the second SHALL be the smaller tile in the hottest step

#### Scenario: A tile is selected
- **WHEN** the tile of `src/main.rs` is clicked
- **THEN** the footer SHALL read its path, its line count and its commits

#### Scenario: The ranking
- **WHEN** the `Ranking` rendering is chosen
- **THEN** the files SHALL be listed by lines, largest first

#### Scenario: The ASCII rendering
- **WHEN** the map is shown in the ASCII rendering
- **THEN** every glyph it draws SHALL be an ASCII character

### Requirement: A file on the map is the surface's selection
Activating a selected file's tile, or clicking it a second time, SHALL
select that file and leave the map for whichever half the map was opened
over. The map is a fourth way of asking about the same path, so the
selection SHALL survive the switch the way it does between the other
three.

#### Scenario: Down to the file
- **WHEN** the tile of `src/main.rs` is selected and activated
- **THEN** `src/main.rs` SHALL be the surface's selection
- **AND THEN** the half the map was opened over SHALL be on show again

### Requirement: Measuring the checkout never blocks the workspace
The measurement SHALL be made off the thread that draws, and asked for
once per checkout the surface is opened on — a `git grep` over every file
is not a read to repeat, and the answer outside a repository is nothing,
which SHALL NOT be asked for again. Its answer SHALL carry the checkout
it was asked for, and one that arrives after the surface was closed or
reopened elsewhere SHALL be dropped. A measurement arriving SHALL NOT
change what is on show: the map is a mode somebody asks for.

#### Scenario: The measurement arrives while the diff is being read
- **WHEN** the surface is showing a diff and the measurement arrives
- **THEN** the diff SHALL stay on show
- **AND THEN** the map SHALL be on offer from the file tree

#### Scenario: The surface was closed before the answer
- **WHEN** the surface is closed while the measurement is outstanding
- **THEN** the late answer SHALL be dropped
