## Purpose

Defines the root-vs-namespace grammar of the `uze` CLI itself: which commands are project-scoped versus
machine-scoped, how `<plugin>@<market>` is resolved against built-in commands, and what the CLI does in
every ambiguous or invalid input case — the cross-cutting contract every other CLI capability composes with.

## ADDED Requirements

### Requirement: Root-level commands are project-scoped, namespaced commands are machine-scoped
The system SHALL group every CLI command into exactly one of two scopes, communicated by its position in
the grammar: a **root-level command** (no namespace prefix) operates on the current project environment
(`agents.lock`, `AGENTS.md`, `.agents/`); a command under the `market`, `plugin`, or `harness` namespace
operates on machine-level resources (`~/.uze/store`, the marketplace registry, harness provisioning). No
command SHALL exist at the root that mutates machine-level state without also being reachable, under the
same semantics, from its machine namespace.

#### Scenario: Root commands never require a namespace prefix
- **WHEN** the user runs `uze install`, `uze remove <plugin>`, `uze status`, `uze context ...`, or
  `uze <plugin>@<market>`
- **THEN** only the current project's `agents.lock`/`AGENTS.md`/`.agents/` are read or written; no
  machine-level file outside the Store's read path is mutated

#### Scenario: Machine commands always require a namespace prefix
- **WHEN** the user wants to install, remove, update, list, or inspect a plugin independently of any
  project, or manage marketplace sources, or manage harness integrations
- **THEN** the command is spelled `uze plugin <verb>`, `uze market <verb>`, or `uze harness <verb>` — none
  of these operations exist at the CLI root

### Requirement: `<plugin>@<market>` project shorthand mutates the project, acquiring machine-side only as a consequence
`uze <plugin>@<market>` SHALL mean "make the current project use `<plugin>` from marketplace `<market>`":
it SHALL add or update the corresponding entry in `agents.lock`, acquiring the plugin into the machine
Store when not already present as a necessary side effect of resolution — never as its primary intent. It
SHALL NOT be a synonym for `uze plugin install <plugin>@<market>`.

#### Scenario: Shorthand on a project with no lock yet
- **WHEN** the user runs `uze flow@ai` in a directory with no `agents.lock`
- **THEN** the system resolves the project root (per existing walk-up rule: `agents.lock` > `AGENTS.md` >
  `.git` > cwd), creates `agents.lock` there with `flow`'s entry (marketplace `ai`), and ensures `flow` is
  present in the Store

#### Scenario: Shorthand on a project with an existing lock
- **WHEN** the user runs `uze flow@ai` in a project whose `agents.lock` already declares other plugins
- **THEN** `flow`'s entry is added to the existing `agents.lock` without disturbing the other entries

#### Scenario: Plugin already declared with the same marketplace
- **WHEN** the user runs `uze flow@ai` and `agents.lock` already declares `flow` from marketplace `ai`
- **THEN** the command is idempotent: the lock entry is rewritten with the same values, no duplicate Store
  install occurs, and the command succeeds

#### Scenario: Plugin already declared with a different marketplace
- **WHEN** the user runs `uze flow@ai` and `agents.lock` already declares `flow` from a different
  marketplace (e.g. `beta`)
- **THEN** the command fails with an explicit marketplace-mismatch error naming both the existing and
  requested marketplace; `agents.lock` is not modified

#### Scenario: Marketplace source conflicts with the lock's own record
- **WHEN** the user runs `uze flow@ai` and `agents.lock` already records marketplace `ai` with a source
  that differs from the machine's registered `ai` marketplace
- **THEN** the command fails with an explicit source-conflict error; `agents.lock` is not modified

#### Scenario: Marketplace is not known
- **WHEN** the user runs `uze flow@ai` and `ai` is neither the embedded `uze-official` marketplace nor
  present in the machine's marketplace registry
- **THEN** the command fails with an error naming the missing marketplace and suggesting
  `uze market add <source>`; `agents.lock` is not modified

#### Scenario: Outside any recognized project
- **WHEN** the user runs `uze flow@ai` in a directory with no `agents.lock`, `AGENTS.md`, or `.git`
  anywhere in its ancestry
- **THEN** the project root resolves to the current directory itself and a new `agents.lock` is created
  there — this is the deliberate bootstrap path for a brand-new project, not an error

### Requirement: The shorthand requires an explicit `@market` segment; no bare-name form exists
A first argument containing no literal `@` SHALL NOT be treated as project shorthand, regardless of
whether it happens to match a known marketplace plugin name.

#### Scenario: Bare name is not shorthand
- **WHEN** the user runs `uze flow` (no `@`)
- **THEN** the system does not create or modify `agents.lock`; it attempts ordinary command dispatch,
  which fails because `flow` matches no built-in command, and the error names the missing `@market` as the
  likely cause

### Requirement: Built-in commands take unconditional precedence over shorthand, by construction
No built-in command name (at any nesting level: root, `market`, `plugin`, `harness`, and their verbs)
SHALL ever contain the `@` character. Because the shorthand grammar requires `@` and no built-in name can
contain it, a single first-argument token SHALL be classified as shorthand if and only if it contains `@`
and does not start with `-` — this classification SHALL need no priority ordering or special-casing beyond
that one lexical fact, and SHALL be enforced by a test that programmatically inspects the registered
command tree for a `@` in any name.

#### Scenario: No built-in command can ever collide with a shorthand token
- **WHEN** any current or future built-in command or subcommand is named
- **THEN** a test fails at build/test time if that name contains `@`, preventing the ambiguity from ever
  being introduced

#### Scenario: A plugin or marketplace name matching a built-in verb is still unambiguous
- **WHEN** the user runs `uze install@ai` (a marketplace plugin literally named `install`)
- **THEN** the token contains `@`, so it is classified as shorthand — not as the `install` built-in —
  requesting plugin `install` from marketplace `ai`

### Requirement: The shorthand's trailing flags are parsed by the same argument grammar as every other command
Flags following `<plugin>@<market>` (`--trust`, `--format`) SHALL be parsed through the same declarative,
validated argument grammar used by every other command — not a hand-rolled loop that can silently accept
or ignore unrecognized flags.

#### Scenario: An unrecognized flag after the shorthand is rejected, not ignored
- **WHEN** the user runs `uze flow@ai --trsut` (a typo)
- **THEN** the command fails with an "unrecognized argument" error naming `--trsut` — it does not silently
  proceed without granting trust

### Requirement: Local filesystem paths are not expressible as project-shorthand input
`uze <path>@<market>` and any other attempt to express a local, unpublished plugin directory as a
project-shorthand argument SHALL be rejected by the same charset validation the shorthand's plugin-name
segment already applies, consistent with `agents.lock` only ever recording marketplace- or Git-sourced
plugins (never a bare local path) for reproducibility.

#### Scenario: A path-like plugin segment is rejected
- **WHEN** the user runs `uze ./my-plugin@ai`
- **THEN** the command fails with an invalid-plugin-name error; no lock is written, and the error does not
  suggest this is a supported form

### Requirement: `uze --help` and every namespace's `--help` communicate the Project/Machine split
The top-level `uze --help` output SHALL group commands under headings that name the Project/Machine split
explicitly (e.g. a "Project" heading and a "Machine" heading), list the `<plugin>@<market>` shorthand under
Usage, and list `market`, `plugin`, and `harness` under the Machine heading. `uze market --help` and
`uze plugin --help` SHALL each list only that namespace's verbs.

#### Scenario: Top-level help names both scopes
- **WHEN** the user runs `uze --help`
- **THEN** the output contains a heading naming project-scoped commands and a separate heading naming
  machine-scoped commands, and the `<plugin>@<market>` form appears in the Usage line

#### Scenario: Namespace help is self-contained
- **WHEN** the user runs `uze market --help`
- **THEN** the output lists exactly `market`'s verbs (`add`, `remove`, `list`, `inspect`) with no
  unrelated commands mixed in
