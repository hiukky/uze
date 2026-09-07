## Purpose

How the work an agent does acquires the two names people read — the branch
a reviewer meets on a pull request, and the label an operator reads beside
its siblings — so that neither is a generated identifier, and neither
overwrites a name a person chose.

## ADDED Requirements

### Requirement: The work is named through a surface any harness can reach
The system SHALL provide a command an agent runs to name the work it is
doing, resolved from the working directory alone. It SHALL NOT depend on a
harness event, a launch prompt, or a vendor-specific capability, because a
name every harness can supply is the only name always available.

#### Scenario: The task is resolved from the checkout the command runs in
- **WHEN** an agent runs the naming command from anywhere inside its
  isolated checkout, including a nested directory
- **THEN** the task that owns that checkout is the one named

#### Scenario: A slot reused by successive tasks names its current owner
- **WHEN** the naming command runs in a checkout that earlier tasks also
  used
- **THEN** the task named is the one that owns the slot now, never a task
  that used to

#### Scenario: There is no task to name
- **WHEN** the command runs in the primary checkout, or outside any
  repository
- **THEN** it fails saying so, and names nothing

#### Scenario: A task cannot be named from outside itself
- **WHEN** the command is invoked with the intent of naming a task other
  than the one owning the working directory
- **THEN** there is no argument that expresses it: one agent can never
  rename another's branch

### Requirement: A name has two halves and each is read by someone different
A name SHALL be authored once as a type and a subject. The branch SHALL
carry both; the visible label SHALL carry the subject alone. The subject
SHALL be short — an intention in one or two words, not a description of
the task.

#### Scenario: One name reaches both surfaces
- **WHEN** an agent names its work `fix/branch-naming`
- **THEN** its branch is `fix/branch-naming` and its label reads `branch
  naming`

#### Scenario: The label is never the identifier once a name exists
- **WHEN** a task has been named
- **THEN** no surface shows its generated identifier in place of its name

### Requirement: The project declares the vocabulary and every name is validated against it
A project SHALL declare which branch types its names may use, as a named
preset or as its own list. A proposed name SHALL be accepted only when its
type is in that vocabulary and its subject is a well-formed single path
segment. A refusal SHALL say which half was wrong, so the next attempt is
informed rather than guessed.

#### Scenario: A name outside the vocabulary is refused
- **WHEN** an agent proposes a type the project does not declare
- **THEN** the name is refused, naming the declared vocabulary, and the
  branch is unchanged

#### Scenario: A malformed subject is refused
- **WHEN** a proposed subject is empty, carries a path separator, or is
  longer than the declared limit
- **THEN** the name is refused with that reason

#### Scenario: A name already taken is refused
- **WHEN** the proposed branch name already exists in the repository
- **THEN** the name is refused rather than silently disambiguated

#### Scenario: An undeclared vocabulary keeps today's behavior
- **WHEN** a project declares no branch vocabulary
- **THEN** branches are named from the identifier exactly as before

### Requirement: A name nobody generated is never overwritten
A task's branch and label SHALL be replaced automatically only while they
are still the generated defaults. Once any name has been written — by the
agent, by the operator, or by an earlier automatic step — it SHALL be
final until a person changes it again.

#### Scenario: The generated name is replaced once
- **WHEN** a task whose branch is still the generated one is named
- **THEN** the branch and label take the new name

#### Scenario: A chosen name survives every later mechanism
- **WHEN** a task that already carries a chosen name reaches any later
  naming step — a second naming call from the agent, the publish-time
  fallback
- **THEN** the existing name stands and nothing is renamed

#### Scenario: Renaming does not disturb identity
- **WHEN** a task is renamed
- **THEN** its identifier, its checkout directory and its persisted state
  are unchanged

### Requirement: The checkout's HEAD is the truth about a task's branch
The branch recorded for a task SHALL be a cache of the branch its checkout
is on, re-read whenever the task is evaluated. A branch renamed outside
UZE SHALL therefore reach every surface that reads it, and SHALL NOT leave
UZE asking Git about a branch that no longer exists.

#### Scenario: A manual rename reaches the operator's view
- **WHEN** the operator renames a task's branch inside its checkout and the
  task is evaluated
- **THEN** the sidebar, the delivery target and the sync count all read the
  new name

#### Scenario: A manual rename does not break readiness
- **WHEN** a task whose branch was renamed by hand has commits ahead of its
  base on a clean tree
- **THEN** it is reported ready and delivery is offered, rather than
  counting zero commits against a branch that no longer exists

#### Scenario: A detached head changes nothing
- **WHEN** a task's checkout is mid-rebase and on no branch
- **THEN** the recorded branch is left as it was

### Requirement: Naming is enforced where the harness can enforce it
The system SHALL refuse an agent's first commit while its task is still
unnamed, through the portable Hook capability, and the refusal SHALL carry
the command that resolves it. Where a harness cannot express a denial, the
delivery SHALL degrade to the projected instruction and the degradation
SHALL be recorded, never presented as enforcement.

#### Scenario: An unnamed task's commit is refused
- **WHEN** an agent attempts its first commit with its task still unnamed,
  on a harness that honors a denial
- **THEN** the commit is refused and the reason names the command and the
  project's vocabulary

#### Scenario: A named task commits freely
- **WHEN** the same agent commits after naming its work
- **THEN** nothing is refused

#### Scenario: A harness without denial is honest about it
- **WHEN** the hook is delivered to a harness that claims no denying effect
- **THEN** the route is recorded as degraded with the reason, and the
  projected instruction is what carries the expectation

### Requirement: A published branch never carries a generated identifier
When a task's branch is published and it was never named, the system SHALL
derive a readable name from the first commit on that branch rather than
publishing the generated identifier.

#### Scenario: An unnamed task is published readably
- **WHEN** a task that was never named is delivered by publishing its
  branch
- **THEN** the published branch is named from its first commit's subject

#### Scenario: A named task is published under its own name
- **WHEN** a named task is published
- **THEN** the published branch carries the name it already had, and no
  second name is invented

#### Scenario: A published name is frozen
- **WHEN** a task's branch has already been published
- **THEN** no automatic mechanism renames it again

### Requirement: The agent surface is documented where agents read
Commands whose audience is the agent SHALL be excluded from the help a
person reads and SHALL be documented in the instruction text projected
into the project, which is the surface an agent actually reads. The
projected text SHALL name the project's own vocabulary, so the instruction
an agent reads is the one its project will accept.

#### Scenario: The human help does not carry the agent surface
- **WHEN** a person asks for help
- **THEN** the agent-facing commands are not listed among the commands a
  person is offered

#### Scenario: The projected instruction carries the vocabulary in force
- **WHEN** a project declares its branch vocabulary
- **THEN** the projected instruction names that vocabulary and the naming
  command, and it changes when the declaration changes
