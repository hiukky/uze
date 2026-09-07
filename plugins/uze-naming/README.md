# uze-naming

Enforcement for the branch vocabulary a project declares in
`agents.yaml`'s `worktrees.branch`.

One `PreToolUse` hook, effect `deny`: an agent's commit is refused while
its task still carries the name UZE generated for it, and the refusal names
the command that clears it (`uze agent task name <type>/<subject>`) and the
types this project accepts.

## Why this is a plugin of its own

A hook is an executable capability, and installing one is a trust
decision. The `uze` plugin is installed on every machine by UZE's own
bootstrap; making *that* package carry an executable capability would mean
every machine silently authorizing one. Enforcement is a thing a project
asks for:

```bash
uze uze-naming@uze-official
```

## What it does not do

It is inert in a project that declares no `worktrees.branch` vocabulary,
and it never fires outside an isolated checkout — the operator's own
commits in the primary checkout are not its business.

Where a harness cannot express a denial (OpenCode claims `observe` and
`allow` only), delivery degrades to the instruction UZE projects into
`AGENTS.md` and the downgrade is recorded rather than presented as
enforcement.
