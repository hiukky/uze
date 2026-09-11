# An extension may write only what it was shown

Status: Accepted

## Context

ADR-041 made extension code a trust class of its own and fixed how it
reaches the machine: through the `Host` trait the TUI implements, never on
its own. Until the code surface, everything `Host` granted was a read —
run Git, read a file, list a directory — so the boundary only ever had to
answer what an extension could *see*.

The code surface edits the files it shows and deletes them. That is the
first time extension code can change the operator's machine, and the grant
has to be decided before it exists: once a surface depends on a broad
write, narrowing it is a regression someone notices.

"Write a file" and "delete a path" are each a family of powers. Writing
covers creating any path a string can name; deleting covers a directory,
which is recursive by nature. Neither is a gesture the surface makes.

## Decision

`Host` grants two writes, each as narrow as the gesture behind it:

- `write_file` replaces the contents of a file that already exists. It
  refuses a path that is not an existing file. Creating a file from a
  typed string is a separate grant that nothing asks for yet.
- `delete_file` removes a file. It refuses a directory, so "delete this"
  can never mean "delete these four hundred".

The host enforces both refusals; the extension cannot widen them. The
confirmation before a delete is the surface's, and the refusal behind it
is the host's, so a bug in one does not remove the other.

## Consequences

The code surface can edit and delete what it lists, and an extension
still reaches nothing it was not handed — ADR-041's property holds, with a
smaller claim: an extension can change a file it was shown, never create
one, and never remove a directory.

A future surface that needs to create files, rename them or remove a tree
has to ask for a new grant, stated as narrowly as these, rather than find
the power already there. A loading mechanism for extensions authored
elsewhere (ADR-041) inherits these as the first capabilities its model has
to express.

Source change: openspec/changes/add-the-code-surface/
