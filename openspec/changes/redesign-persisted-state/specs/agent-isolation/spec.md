## ADDED Requirements

### Requirement: Preserved work is found without the space it was left in
The preserved-work list SHALL answer from UZE's records of every project it
has recorded an agent for, not from the projects the current session
happens to have opened. Work held by an agent that no live pane carries
SHALL be listed whether or not a space is open on its project, and SHALL
remain listed until it is resumed, finished or discarded.

Because the list crosses projects, each entry SHALL name the project its
work belongs to. Two agents carrying the same branch name in two projects
SHALL be distinguishable from the list alone.

#### Scenario: The space the work was left in was closed
- **WHEN** an agent holding work has no live pane and no space is open on its project
- **THEN** its work is listed, naming the project it belongs to

#### Scenario: A project the session never opened
- **WHEN** the operator opens the list in a session that has opened no space on a project holding preserved work
- **THEN** that project's preserved work is listed, without the operator having opened it first

#### Scenario: Work that is delivered or discarded
- **WHEN** an agent's work has been integrated, closed or discarded
- **THEN** it is not listed

#### Scenario: Two projects, one branch name
- **WHEN** two projects each hold preserved work on a branch of the same name
- **THEN** both are listed, and each entry names its own project

#### Scenario: The list stays quick as projects accumulate
- **WHEN** the operator opens the list on a machine holding many projects
- **THEN** it is drawn from what UZE recorded, without asking Git about any
  repository, and the operator waits on nothing

### Requirement: A resumed agent returns to its own project's space
Resuming preserved work SHALL place the agent in a space rooted at the
project the work belongs to, never in whichever space the operator was
looking at. A space is matched by its canonical root alone: its name, its
identity and when it was opened SHALL NOT affect the match, because work is
bound to a project and never to a space.

When no space is rooted at that project, resuming SHALL open one and place
the agent there. The operator SHALL be left looking at the resumed agent.

#### Scenario: A space on the project is already open
- **WHEN** the operator resumes work whose project already has a space open
- **THEN** the agent opens in that space, and no second space is created for the same root

#### Scenario: The space was closed and a new one opened on the same directory
- **WHEN** the space the work was left in was closed, and a different space was later opened on the same project directory
- **THEN** the agent opens in that space, however it is named and whenever it was opened

#### Scenario: No space on the project is open
- **WHEN** the operator resumes work whose project has no space open
- **THEN** a space rooted at that project is opened and the agent opens in it

#### Scenario: A space rooted above the project
- **WHEN** an open space's root contains the project but no space is rooted at the project itself
- **THEN** a space rooted at the project is opened, and the space above it receives no tab

#### Scenario: Resuming from a different project
- **WHEN** the operator resumes work belonging to a project other than the one they were looking at
- **THEN** the space they were looking at receives no tab

#### Scenario: Several spaces on one project
- **WHEN** more than one open space is rooted at the work's project
- **THEN** the agent opens in the selected one when it is among them, and otherwise in the first, and no further space is created

#### Scenario: The checkout was removed by hand
- **WHEN** the operator resumes work whose checkout no longer exists
- **THEN** the agent is given a checkout again on its own branch, in a space rooted at its project

#### Scenario: The project itself is gone
- **WHEN** the operator resumes work whose project directory no longer exists
- **THEN** nothing is opened, the reason is said, and the entry stays in the list
