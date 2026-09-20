## ADDED Requirements

### Requirement: A project can be told whether its artifacts draw
UZE SHALL offer a command that reads every artifact the project declares
and draws it, reporting for each one whether it drew and exiting non-zero
when any did not. It SHALL use the same catalog, parser and layout the
surface uses, and SHALL NOT restate the accepted syntax anywhere: a reason
it reports SHALL be the one the surface would show. The reader is the
agent that wrote the file, so the command SHALL sit in the `agent`
namespace and SHALL offer both text and JSON.

An artifact SHALL be reported in one of three states: drawn; drawn with a
stated number of relations that found no path; or not drawn, with the
reason quoted. The second SHALL fail the check like the third, because a
diagram missing a relation still draws every box.

A project's *declaration* SHALL be judged apart from what its directory
holds. Declaring no artifacts, and declaring a directory that holds none,
SHALL both pass. A declaration the host will not follow, and a declared
directory that cannot be read, SHALL fail.

#### Scenario: A diagram type the surface does not draw
- **WHEN** the declared directory holds a file beginning with `gantt`
- **THEN** the check SHALL report it not drawn, with the surface's own
  reason
- **AND THEN** the command SHALL exit non-zero

#### Scenario: A diagram whose relations have nowhere to go
- **WHEN** a flowchart joins every node to every other, and the layout
  leaves edges unrouted
- **THEN** the check SHALL report how many found no path
- **AND THEN** the command SHALL exit non-zero, as for one that does not
  draw at all

#### Scenario: A statement that cannot be read
- **WHEN** a C4 relation names only one end
- **THEN** the check SHALL quote the statement back
- **AND THEN** every other artifact SHALL still be reported

#### Scenario: A project with nothing declared
- **WHEN** `agents.yaml` declares no `artifacts:`
- **THEN** the check SHALL say so and SHALL exit zero

#### Scenario: A directory declared before anything is drawn in it
- **WHEN** the declared directory holds no Mermaid file
- **THEN** the check SHALL report nothing checked and SHALL exit zero

#### Scenario: A declaration the host will not follow
- **WHEN** `artifacts.path` leaves the project
- **THEN** the check SHALL report the refusal and SHALL exit non-zero
