## Layering as data, with debt made visible

A rule is a struct, not a hand-written assertion:

```rust
struct Rule {
    name: &'static str,
    scope: &'static str,       // directory scanned
    forbidden: &'static str,   // path prefix that must not appear
    reason: &'static str,      // printed on failure
    sanctioned: &[(&str, &str)], // permanently allowed, with why
    budget: &[(&str, usize)],    // debt, exact count, may only shrink
}
```

Two lists rather than one, because they mean opposite things. `sanctioned`
is architecture — the runtime shim and the harness-matrix tool consume
`IntegrationRegistry` because that is what a composition root is for, and
nobody should ever "fix" them. `budget` is debt with a number on it.

The budget is an **exact** count, not a ceiling. Removing a violation fails
the test until the number is lowered, which puts every improvement in a diff
and stops a budget from silently over-permitting. Test-only code
(`#[cfg(test)]` modules and the files they gate) is out of scope: a fixture
legitimately builds domain values, and forcing it through the facade would be
worse code, not better architecture.

The end state is not a passing test. It is the deletion of `uze-core` from
the binary's `[dependencies]`, after which `rustc` enforces the rule and the
test is redundant. The suite is scaffolding that reports how far away that
is.

## One transport for Git

`uze-git` owns the process contract and nothing else — no notion of a
worktree, a checkout, or a diff view. Exit-code classification belongs to the
command, not to the caller: `diff` returning `1` means "there are
differences", `rebase` returning `1` means "conflict", and both are states
rather than failures. Encoding that once is the only way a future write lock
over refs can be complete, because a lock is worthless if a second module
can spawn Git around it.

Reads and writes are separate entry points from the start. A read must never
take the lock — a status view that blocks behind a rebase is a worse product
— and that asymmetry has to be visible in the signature rather than
remembered.

`uze-extensions` regains a dependency it lost when the worktree tree was
removed from the diff view. That removal was correct and this addition is
unrelated: a transport crate is not the domain crate, and the architecture
suite forbids only the latter.

## The extension contract

Today an extension receives `&mut ratatui::Frame` and pushes `(Rect, Hit)`
pairs. It therefore owns geometry, colour, and layout, and duplicates the
host's palette by hand — the crate doc asks for the two to be kept in sync
"by eye".

The extension will instead answer with a view model: what it has, not how it
looks. Syntax highlighting survives as a *role* per span (`Keyword`,
`Added`, `Removed`) that the host maps to its own palette, which is strictly
better than a copied colour table. Geometry moves to the host: it rendered
the list, so it knows a click on row three is row three, and hands back the
semantic hit the extension already accepts today on `handle_mouse`.

The vocabulary starts as the smallest set `git_diff` actually needs and grows
only when a **second** extension needs the same primitive. One extension
wanting a widget is a special case; two are evidence.

The input half of the contract (`handle_key`, `handle_mouse`, `handle_scroll`
returning an outcome) is already data-in, data-out and does not change.

## What "extension never touches the machine" actually means

The extension registry claims extensions are "pure compiled-in code that
never touches the machine". Taken literally this is already false: the diff
view spawns Git and reads files. The true and useful property is narrower —
an extension never touches **UZE's own state**: no `UzeHome`, no Store, no
receipts, no `uze-application`. That is what makes it safe to render and
what makes the trust question tractable later, and it is what the suite
locks.
