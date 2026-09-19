## ADDED Requirements

### Requirement: The checkout's code is drawn as a map nobody has to write
The surface SHALL list, under an area of its own placed after every
declared one, a code map of the active checkout: a treemap in which a
file's area is proportional to its line count, a directory is a titled
frame around what it holds, and a directory with a single directory in it
is drawn as one frame named for both. The map SHALL cover the text files
Git tracks and the untracked ones it does not ignore, and SHALL leave out
binary files. It SHALL require no file, no manifest key and no declared
artifact. Outside a Git repository the area SHALL be absent.

#### Scenario: A project with declared artifacts
- **WHEN** the surface is opened in a repository that declares artifacts
- **THEN** the area selector SHALL list `Code` after the declared areas
- **AND THEN** the first declared artifact SHALL still be what opens

#### Scenario: An ignored directory
- **WHEN** the repository ignores `target/`
- **THEN** no tile SHALL stand for anything under `target/`

#### Scenario: A chain of single directories
- **WHEN** `crates/` holds only `core/`, which holds only `src/`
- **THEN** one frame titled `crates/core/src` SHALL be drawn, not three

#### Scenario: Not a repository
- **WHEN** the surface is opened outside a Git repository
- **THEN** no `Code` area SHALL be listed

### Requirement: The code map fits the view, and is entered rather than moved
The code map SHALL be laid out for the space the board has, and laid out
again when that space changes; it SHALL NOT be panned, and SHALL show no
minimap and no dot grid. Tiles SHALL be laid out so they stay close to
square as seen, a cell being about twice as tall as it is wide. Files too
small to carry a name SHALL be folded, per directory, into one tile that
says how many files it stands for. A directory too small to show its
contents SHALL be drawn as a single tile. Activating a selected directory,
or a folded tile, SHALL enter it: the map is laid out again for that
directory alone, and the menu SHALL show the trail to it. `level-up` and a
click on a trail entry SHALL go back, with the directory that was entered
selected.

#### Scenario: The terminal is resized
- **WHEN** the view becomes narrower while the map is shown
- **THEN** the map SHALL be laid out again to fill exactly the new space

#### Scenario: Many small files
- **WHEN** a directory holds three large files and forty one-line files
- **THEN** the three SHALL be drawn and named
- **AND THEN** one tile SHALL read that it stands for the other forty

#### Scenario: Entering a directory
- **WHEN** the directory `crates` is selected and activated
- **THEN** the map SHALL show the contents of `crates` alone, filling the
  view
- **AND THEN** the menu SHALL show a trail ending in `crates`

#### Scenario: Entering a folded tile
- **WHEN** a folded tile is activated
- **THEN** the map SHALL show only the files it stood for

#### Scenario: Coming back
- **WHEN** `level-up` is triggered inside `crates`
- **THEN** the whole map SHALL be shown with `crates` selected

#### Scenario: The arrows
- **WHEN** a pan action is triggered on the code map
- **THEN** the selection SHALL move to the nearest tile in that direction

### Requirement: The code map says which files are large and which are hot
A file's tile SHALL be coloured by how many commits touched it in the last
year, ranked against the other files of the map in four steps from none to
hottest, using semantic roles only; the hotter two steps SHALL also be
shaded, so the ranking survives a palette that separates roles poorly. A
directory drawn as one tile SHALL take the step of its hottest file. A
file with uncommitted changes SHALL be marked. Selecting a tile SHALL name
its path, its lines, its commits and whether it is changed in the footer;
with nothing selected the footer SHALL give the map's files and lines. The
`Source` rendering of the code map SHALL be the ranking itself: every file
by lines, largest first, with its commits.

#### Scenario: A large file nobody touches and a small one everybody does
- **WHEN** the map holds a 5,000-line file with no commit this year and a
  200-line file with the most commits of any
- **THEN** the first SHALL be the larger tile in the coldest step
- **AND THEN** the second SHALL be the smaller tile in the hottest step

#### Scenario: A tile is selected
- **WHEN** the tile of `src/main.rs` is clicked
- **THEN** the footer SHALL read its path, its line count and its commits

#### Scenario: The ranking
- **WHEN** the next-rendering action reaches `Source` on the code map
- **THEN** the files SHALL be listed by lines, largest first

#### Scenario: The ASCII rendering
- **WHEN** the code map is shown in the ASCII rendering
- **THEN** every frame and every shade SHALL be an ASCII character

### Requirement: A file on the code map opens in the code surface
Activating a selected file's tile, or clicking it a second time, SHALL
open the code surface on that file, the way a box that leads to the code
does.

#### Scenario: Down to the file
- **WHEN** the tile of `src/main.rs` is selected and activated
- **THEN** the code surface SHALL open showing that file's contents

### Requirement: Measuring the code never blocks the workspace
The code map SHALL be measured off the thread that draws, independently of
the declared artifacts, so neither waits for the other. Its answer SHALL
carry the checkout it was asked for, and one that arrives after the surface
was closed or reopened elsewhere SHALL be dropped. A measurement that
arrives after the artifacts SHALL NOT change which artifact is on show.

#### Scenario: The measurement is slower than the artifacts
- **WHEN** the declared artifacts arrive first and the first is on show
- **THEN** the `Code` area SHALL appear when the measurement arrives
- **AND THEN** the artifact on show SHALL stay on show

#### Scenario: The surface was closed before the answer
- **WHEN** the surface is closed while the measurement is outstanding
- **THEN** the late answer SHALL be dropped

## MODIFIED Requirements

### Requirement: A project declares where its architecture artifacts live
A project SHALL declare its architecture artifacts by naming one directory
in `agents.yaml`, as `artifacts.path`, relative to the project root. The
declaration SHALL be optional. A path that is absolute, or that leaves the
project, SHALL be refused rather than followed. The scaffolded
`agents.yaml` SHALL show the key, commented out. Where there are no
declared artifacts to show but there is a code map, the code map SHALL be
shown, and what is the matter with the declaration SHALL be said beside it
rather than in place of it.

#### Scenario: The project declares a directory
- **WHEN** `agents.yaml` holds `artifacts: { path: docs/architecture }`
- **THEN** the artifacts are read from `docs/architecture` under the
  project root

#### Scenario: The project declares nothing
- **WHEN** the surface is opened, outside a Git repository, in a project
  whose `agents.yaml` has no `artifacts` key
- **THEN** the surface SHALL say the project declares no artifacts yet
- **AND THEN** it SHALL show how to declare them, and SHALL NOT present
  this as an error

#### Scenario: The project declares nothing, in a repository
- **WHEN** the surface is opened in a Git repository whose `agents.yaml`
  has no `artifacts` key
- **THEN** the code map SHALL be shown
- **AND THEN** the footer SHALL say the project declares no artifacts yet

#### Scenario: The declared path leaves the project
- **WHEN** `artifacts.path` is `../elsewhere` or `/etc`
- **THEN** nothing SHALL be read from that path
- **AND THEN** the surface SHALL say that `artifacts` in `agents.yaml`
  needs fixing

#### Scenario: The declared directory holds no diagram
- **WHEN** the declared directory exists and holds no Mermaid file
- **THEN** the surface SHALL name the declared path and say it holds no
  Mermaid files yet

#### Scenario: An unknown key under artifacts
- **WHEN** `artifacts` holds a key other than `path`
- **THEN** the manifest SHALL be rejected, as it is for any unknown key
