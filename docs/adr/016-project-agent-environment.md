# Project Agent Environment

Status: Accepted
Status note: partially superseded by docs/adr/019-explicit-project-machine-boundary-in-cli-command-grammar.md,
narrowly — only the `remove` disambiguation behavior described in
`openspec/changes/project-agent-environment/design.md`'s Decision #8 ("remove disambiguated by context:
lock present + plugin in lock → project; else → global"). `uze remove` is now strictly project-scoped with
no fallback to global removal. Every other decision in this ADR remains in effect, reaffirmed by ADR-019.

## Context

UZE has global machine state (`~/.uze/store`, `marketplaces.json`, `attachments.json`, harness provisioning) and project-scoped context (`AGENTS.md`, `.agents/`, harness bridges). However, projects lacked a reproducible declaration of which plugins compose their agent environment.

The problem: a contributor cloning a repo had no way to reconstruct the author's agent environment without manually running `uze marketplace add` and `uze plugin install` for every dependency. Global state is not versionable; project state must be.

The North Star experience:
```bash
# author
uze flow@ai
git add agents.lock
git commit

# contributor
git clone <repo>
cd <repo>
uze install
```

Result: Same project. Same agent environment. Any supported harness.

## Decision

Formalize three project-scoped artifacts with distinct responsibilities:

- **`AGENTS.md`** — portable instructions baseline (already exists, unchanged)
- **`.agents/`** — portable agent resources (already exists, unchanged)
- **`agents.lock`** — resolved external agent dependencies (NEW)

Separate **Machine Actual State** (global, `~/.uze/*`) from **Project Desired State** (portable, `agents.lock`).

Key invariants:
1. `uze marketplace add` and `uze plugin install` (global admin) NEVER write `agents.lock`
2. `uze <plugin>@<marketplace>` (project shorthand) writes `agents.lock`
3. `uze install` consumes `agents.lock` without silently re-resolving
4. `agents.lock` contains reproducible source identity (Git commit, not branch alias)
5. `agents.lock` NEVER grants trust; `authorize` is always called
6. Store/Engine/Integration remain lock-neutral (vendor neutrality preserved)

Project root resolution: walk upward from `cwd` looking for `agents.lock` (priority), then `AGENTS.md`, then `.git`. First found is root. Fallback: `cwd` itself. One predictable rule, no git assumption.

## Consequences

**Positive:**
- Fresh-machine reproducibility: `git clone + uze install` reconstructs environment from lock alone
- Global/project separation: admin commands never implicitly modify project state
- Vendor-neutral: lock contains no harness-specific paths or config
- Deterministic: `BTreeMap` + `write_atomic` ensures idempotent bytes

**Negative:**
- New file to manage (`agents.lock`)
- New dependency (`noyalib` for YAML serialization, replacing deprecated `serde_yaml`)
- New use cases in Application layer (`add_project_plugin`, `install_project_environment`)

**Neutral:**
- `agents.lock` is desired state; `desired ≠ actual` is valid and diagnosticable (not an error)
- Lock version `1` with explicit `UnsupportedLockVersion` error for future evolution
- `integrity: sha256:...` field reserved but not implemented (commit is identity today)

Source change: openspec/changes/project-agent-environment/
