---
# Two lines deciding who finds this skill: the long text is what the model
# matches an invocation against, so it names the moment the skill answers —
# "create a plugin for me" — not what a scaffold is.
description: Guides creating a uze plugin end to end — marketplace create or select, plugin scaffold, capability flags, check before install, install from the linked marketplace, iterate, publish by pushing. Use when someone asks to create, author or scaffold a plugin, or to set up their own marketplace of plugins.
invoke:
  model: true
  user: true
slash: true
---

Creating a plugin with uze is a loop of four deterministic verbs. Run them
in this order; every one is non-interactive and fails with a reason, so a
failure is an answer, not a dead end.

## 1. The marketplace — create or select

Ask `uze market list` first. If a marketplace already fits, use its name in
every later step; nothing new is created.

Otherwise create one:

```bash
uze agent market create <name> --at <directory> [--description "…"]
```

Choose `--at` outside every checkout — the operator's home (say
`~/marketplace-<name>`) is the natural place. A marketplace is machine
state, shared by every project on it; one created inside a worktree slot
dies with the slot, and one created inside any repository is a nested
repository that dirties that checkout's status. Inside a project only when
the marketplace is deliberately that repository's own, versioned with it.

This scaffolds the directory as a Git repository (`marketplace.json`,
`plugins/`, an initial commit), registers it with the machine, and links
it — the link is the point: installs read the working tree, including
files not committed yet, so authoring needs no publish step. If Git has no
identity configured the command says so with the two `git config` lines to
set; have the person run them (or run them with their consent), then
repeat the command.

## 2. The plugin

```bash
uze agent plugin create <name> --market <market> [--description "…"] \
    [--hook] [--mcp] [--instructions]
```

The default is a skill plugin: `plugin.json` plus
`skills/<name>/SKILL.md` — edit the skill body, and choose the
`invoke:` policy deliberately (who may trigger it). `--hook` adds a
portable `hooks.json` and a handler stub obeying the `HOOK_*`/exit-code
contract; `--mcp` adds an `mcp.json` with one server stub; `--instructions`
adds a prose contribution the project's `AGENTS.md` composes when it
reconciles. Every generated file carries commented field documentation.

## 3. Check, always before install

```bash
uze agent plugin check <plugins/<name>>
uze agent market check <market directory>
```

This runs the same parsers an install runs, offline. A clean check is the
licence to install; a finding names the file and the reason — fix the file,
check again. Never skip it: the feedback an install would have surfaced
arrives here, before anything is delivered.

## 4. Install and iterate

```bash
uze install -m <name>@<market>
```

Because the marketplace was born linked, editing the plugin's files is what
a re-install reads — no commit needed to test. Loop: edit → check → install.

## Publishing

The marketplace is a normal Git repository the moment it is born. When it
is worth sharing, `git push` it to a host and `uze market add
<url>` on another machine — nothing about the workflow changes.
