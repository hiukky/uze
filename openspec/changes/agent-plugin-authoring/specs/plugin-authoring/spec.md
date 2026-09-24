# plugin-authoring Specification

## Purpose
The deterministic authoring surface UZE exposes to an agent: scaffolding a marketplace as a registered, linked Git repository, scaffolding a plugin and its capabilities inside a chosen marketplace, and validating authored plugins and marketplaces offline — each a single-purpose verb under `uze agent …`, hidden from the person-facing `--help` and documented in the projected `AGENTS.md` region.

## ADDED Requirements

### Requirement: Authoring verbs are deterministic, agent-facing, and unaliased for people
The system SHALL expose plugin authoring only as deterministic verbs under `uze agent …`: each verb takes its full input on the command line (or its defaults), performs exactly one artifact-producing step, prints a machine-readable answer, and never prompts interactively. The authoring verbs SHALL be hidden from `uze --help` like the rest of the `agent` grammar, documented in the region UZE projects into `AGENTS.md`, and SHALL NOT gain person-facing aliases: the person's CLI surface carries only what a person performs by hand, and browsing marketplaces and their plugins remains the `uze market` surface's answer.

#### Scenario: An agent drives the full authoring loop without a person's input
- **WHEN** an agent runs `uze agent market create`, then `uze agent plugin create`, then `uze agent plugin check`, providing all arguments up front
- **THEN** each command performs its one step and exits zero with a summary an agent can parse, and no command ever waits for interactive input

#### Scenario: Authoring verbs stay off the person's help surface
- **WHEN** a person runs `uze --help` or `uze agent --help`
- **THEN** the authoring verbs are not listed; they are reachable only by following the `AGENTS.md` region, and no person-facing alias of them exists — browsing what a marketplace offers is `uze market`'s answer, not a second authoring spelling

### Requirement: Marketplace scaffold creates a Git repository and registers it linked
The system SHALL, for `uze agent market create <name> [--at <dir>] [--description <text>]`, create a new marketplace at the named directory: a `marketplace.json` with the given name, description and owner, an empty `plugins/` tree, a Git repository initialized with an initial commit (through the `uze-git` transport), and the marketplace registered in the machine registry with its source pointing at that directory **and linked to it**, so installs read the author's working tree — including not-yet-committed files — from the first moment.

#### Scenario: Create a marketplace in one step
- **WHEN** an agent runs `uze agent market create tools --at ~/dev/tool-market --description "My tools"`
- **THEN** `~/dev/tool-market` is a Git repository containing `marketplace.json` and `plugins/`, an initial commit exists, and `uze market list` shows `tools` registered from that path and linked to it

#### Scenario: The scaffolded marketplace delivers uncommitted work
- **WHEN** the agent scaffolds a plugin into the linked marketplace and installs `<plugin>@tools` from that marketplace (machine scope) before any second commit
- **THEN** the install acquires the plugin's working-tree content, including files not yet committed

#### Scenario: Create refuses to collide with an existing name or directory
- **WHEN** the requested name is already registered, or the target directory exists and is not an empty directory
- **THEN** the command fails with a reason naming which precondition failed, and writes nothing

#### Scenario: A standalone marketplace is selected by naming its directory
- **WHEN** the agent already has a marketplace and passes `--market <name>` to a plugin-authoring verb
- **THEN** the verb resolves the name against the machine registry's own answer and fails with a clear reason when the name is unknown; no new verb exists to "select" a marketplace

### Requirement: A local marketplace is the project itself
With `--local`, the marketplace the verb creates is the project the command runs in: `marketplace.json` at the project root and the plugins in `--plugins-dir` (default `plugins/`), exactly the layout this repository itself is one in. The project's own repository SHALL be the marketplace's repository — the scaffold SHALL NOT initialize a repository, make a commit, or write any Git state; the project's own commit flow carries the bytes. A project that already carries a `marketplace.json` SHALL be refused with its name and the direction to add the plugin directly. `--plugins-dir` SHALL be confined to the project (same name rule as a package id). Running `--local` outside any project SHALL fail naming the frontier choice.

#### Scenario: A project becomes its own marketplace
- **WHEN** an agent runs `uze agent market create tools --local` inside a project
- **THEN** the project root carries `marketplace.json` (named `tools`) and `plugins/`, no commit was made by the scaffold, and `uze market list` answers for `tools`, linked to the project root

#### Scenario: The local marketplace delivers the project's working tree
- **WHEN** a plugin is scaffolded into the local marketplace and installed before the project commits anything
- **THEN** the install acquires the working-tree content, because the link reads the checkout, not history

#### Scenario: A project that is already a marketplace is refused, not extended
- **WHEN** `uze agent market create <name> --local` runs where `marketplace.json` already exists
- **THEN** the verb fails naming the existing marketplace and directing to `uze agent plugin create` instead

### Requirement: Plugin scaffold produces an installable plugin inside a marketplace
The system SHALL, for `uze agent plugin create <name> --market <name> [--description <text>] [--hook] [--mcp] [--instructions]`, create `plugins/<plugin-name>/` inside the marketplace's checkout with: a `plugin.json` (name, description), a `skills/<skill-name>/SKILL.md` whose frontmatter carries the canonical `invoke: {model, user}` policy block, the optional capability files for each flag (`hooks.json`, `mcp.json`, an instruction resource), and the matching `plugins[]` entry added to the marketplace's `marketplace.json`. Every scaffolded file SHALL carry commented documentation explaining its fields, and the scaffold SHALL refuse to overwrite an existing plugin of the same name.

#### Scenario: Default scaffold is a skill plugin
- **WHEN** an agent runs `uze agent plugin create greet --market tools --description "Says hello"`
- **THEN** `plugins/greet/` exists with `plugin.json` and `skills/greet/SKILL.md`, the marketplace manifest names `greet → ./plugins/greet`, and the plugin is installable

#### Scenario: Capability flags extend the scaffold
- **WHEN** the agent passes `--hook --mcp`
- **THEN** the scaffolded plugin additionally carries a `hooks.json` with one handler stub obeying the `HOOK_*`/exit-code contract and an `mcp.json` with one server stub, and `check` passes

#### Scenario: Scaffold never overwrites
- **WHEN** `plugins/<name>/` already exists in the marketplace
- **THEN** the command fails naming the existing plugin and writes nothing into it

### Requirement: Check validates authored plugins and marketplaces offline
The system SHALL, for `uze agent plugin check <path>` and `uze agent market check <path>`, validate the authored artifact without touching the Store, any harness or the network, and report every finding: manifest parse errors, a package name outside the valid charset, absolute or `..`-shaped path references, `SKILL.md` files missing required frontmatter or a malformed invocation policy, a `hooks.json` violating the handler contract (timeout bounds, deny exit code semantics), a malformed `mcp.json`, and — for marketplace checks — `plugins[]` entries whose `source` does not resolve inside the marketplace or whose plugin fails its own check. A clean artifact SHALL exit zero; any finding SHALL exit non-zero with the reasons on the answer.

#### Scenario: A plugin that would fail at install fails check first
- **WHEN** an authored plugin's `plugin.json` names it `-starts-with-dash` and the agent runs `uze agent plugin check` on its directory
- **THEN** the command exits non-zero and names the invalid package name, before any install was attempted

#### Scenario: A clean authored plugin passes
- **WHEN** the agent scaffolds a plugin with `--hook --mcp --instructions` and runs `uze agent plugin check` on it
- **THEN** the command exits zero and reports what the plugin would deliver (one skill, one hook group, one MCP server)

#### Scenario: A marketplace check covers its plugins
- **WHEN** the agent runs `uze agent market check` on a scaffolded marketplace whose one plugin has an invalid `hooks.json`
- **THEN** the command exits non-zero and the answer locates the finding in that plugin

### Requirement: The authoring loop ships as a Skill
The system SHALL ship the guided authoring script as a Skill in the official plugin (`plugins/uze/skills/author/SKILL.md`), whose frontmatter carries the canonical invocation policy permitting both model and user invocation, and whose body teaches the loop: marketplace create or select, plugin scaffold, capability flags, `check` before `install`, installing from the linked marketplace, iterating, and publishing by pushing the marketplace repository. The Skill SHALL be delivered by the official plugin's normal Skill delivery — no integration change.

#### Scenario: An agent asked to create a plugin finds the guided script
- **WHEN** a person asks an agent with the official plugin installed to create a plugin for them
- **THEN** the agent has the `uze:author` Skill available and can invoke it (by policy, both model-triggered and user-triggered invocation are permitted)

#### Scenario: The Skill passes its own validation
- **WHEN** the official plugin is checked as any authored plugin
- **THEN** the `author` Skill's frontmatter (name, description, invocation policy) validates against the canonical Skill requirements

### Requirement: Authoring verbs are classified and cheap
The system SHALL classify every new authoring leaf command in `command_performance.rs` as `Budgeted` (scaffold verbs are bounded file writes plus at most one Git commit; check verbs are read-only) or `JustifiedSlow` with a stated reason.

#### Scenario: An unclassified authoring command fails the suite
- **WHEN** an authoring leaf command is added without a performance classification
- **THEN** `cargo test` fails naming the command, as with every other leaf
