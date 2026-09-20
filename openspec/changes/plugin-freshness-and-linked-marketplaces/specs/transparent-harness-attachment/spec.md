## ADDED Requirements

### Requirement: A managed reference that resolves to nothing is adopted, not preserved

When UZE attaches a capability and finds a reference already occupying the
name it needs, it SHALL distinguish by whether that reference resolves.

A reference that resolves to nothing carries no capability into the harness,
so preserving it protects no work and blocks the attachment permanently.
UZE SHALL replace it with the reference it meant to create. This is the
shape a UZE-owned target that moved leaves behind on every machine that had
one: the content is rebuilt at its new path while the reference into it
stays where the harness looks.

A reference that still resolves MAY be somebody's own — a managed-looking
name is not ownership proof — and SHALL continue to be preserved, with the
attachment refused and the reason reported.

Absence is the only ground for adoption. A reference whose target cannot be
read for any other reason — a directory this user may not traverse, a volume
not mounted, a dead network path — SHALL be preserved: UZE cannot tell it
apart from one that resolves, and what it cannot tell about it does not
touch.

#### Scenario: A reference pointing at a path that no longer exists is replaced
- **WHEN** attaching a capability whose discovery name is occupied by a
  reference whose target does not exist
- **THEN** the reference is replaced with the one being attached, the
  attachment succeeds, and the operation it is part of continues

#### Scenario: A reference somebody repointed at their own content is preserved
- **WHEN** attaching a capability whose discovery name is occupied by a
  reference that resolves to content UZE was not attaching
- **THEN** the reference is left exactly as it is, the attachment is refused,
  and the reason names the path

#### Scenario: A reference whose target cannot be read is preserved
- **WHEN** attaching a capability whose discovery name is occupied by a
  reference whose target cannot be read for a reason other than its absence
- **THEN** the reference is left exactly as it is and the attachment is
  refused

A name held by something UZE does not own SHALL stop that capability and no
other. The package is installed before delivery begins, so raising it as the
command's failure reports total failure over partial work — bytes already in
the Store and other capabilities already attached. It SHALL be reported the
way a derived view that failed to refresh already is: named, warned about,
and not the installation's verdict.

The refusal SHALL be explicit in every case. A silent skip, and an automatic
retry under a different name, are both forbidden.

#### Scenario: One blocked reference does not decide the whole operation's fate
- **WHEN** an install delivers several capabilities and one discovery name is
  occupied by a reference that still resolves
- **THEN** the refusal names that capability, the capabilities that could be
  delivered are delivered, and the occupant is untouched

#### Scenario: A blocked capability is never silent
- **WHEN** any capability is not delivered because its name is held
- **THEN** the command says which capability, for which harness, and why

### Requirement: A dangling reference UZE wrote is detectable and removable

A reference in a harness's user-scope discovery location that resolves to
nothing, points inside `$UZE_HOME`, and is claimed by no receipt SHALL be
reported by `uze doctor` and SHALL be removable through it. Nothing but UZE
writes inside `$UZE_HOME`, so such a reference can only be UZE's own — left
by a capability that was renamed or removed while its receipt was lost.

A reference that resolves, or that points anywhere outside `$UZE_HOME`,
SHALL NOT be reported or touched by this sweep.

#### Scenario: A renamed capability's leftover is found
- **WHEN** a discovery location holds a reference into `$UZE_HOME` that
  resolves to nothing and that no receipt claims
- **THEN** `uze doctor` reports it, names its path, and offers to remove it

#### Scenario: Somebody else's entry is never swept
- **WHEN** a discovery location holds a reference that resolves, or one
  pointing outside `$UZE_HOME`
- **THEN** the sweep neither reports nor touches it

#### Scenario: A healthy machine reports no leftovers
- **WHEN** every reference in every discovery location resolves and is
  claimed by a receipt
- **THEN** `uze doctor` reports no leftovers
