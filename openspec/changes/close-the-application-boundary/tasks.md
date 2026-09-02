## 1. A readable domain

- [x] 1.1 Group `uze-core`'s modules into `package`, `capability`, `delivery`, `project`, `machine`, each documenting what belongs in it.
- [x] 1.2 Keep every public path flat through re-exports, so no call site changes.
- [x] 1.3 Repair the two structural guards that hard-coded paths into the moved files — including the one that skipped a file it could not read, and so passed while checking nothing.

## 2. A scopeable surface

- [x] 2.1 Add capability-scoped borrowed views and move each concern's operations onto its own.
- [x] 2.2 Return shared helpers to the type that owns the state; compose sibling services through the handle.
- [x] 2.3 Audit every accessor hop for its receiver rather than trusting a textual rename.

## 3. Presentation stops reaching past it

- [x] 3.1 Re-export from `uze-application` the vocabulary its read models are made of.
- [x] 3.2 Move prompt history, workspace-root resolution and harness descriptors behind `workspace()`.
- [x] 3.3 Move the seat rule, splitting it: the TUI reports which panes are agents, the application decides what that means.
- [x] 3.4 Move hook dispatch, leaving the CLI only stdin and stdout.
- [x] 3.5 Delete the compatibility facade and move its 184 references onto the crates that own them.
- [x] 3.6 Take the layering budget to zero; state the sanctioned exceptions and why the compiler cannot replace the test.

## 4. Two Git spawns

- [x] 4.1 Name both owners in the architecture suite, with the reason each exists, and fail on a third.
- [x] 4.2 Record the invariant.

## 5. One preference procedure

- [x] 5.1 Extract the procedure the four verticals shared; each declares only its mapping.
- [x] 5.2 Derive `changed_keys` from the writes instead of maintaining a parallel list.
- [x] 5.3 Measure the result and record what it implies for the cost of a fifth harness.
