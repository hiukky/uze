## Context

The Overview is a *projection*: the filesystem and Store are evidence;
states are the product. Two constraints shape the design:

1. **No TUI heuristics.** A TUI `if lock_valid && installed == declared
   { ready }` rule re-implements domain logic and will drift. The
   Application already owns `agents.lock`, `agents.json`, Store, and
   context semantics; it must own the verdicts.
2. **No new heavyweight machinery.** The Overview budget is
   millisecond-fast (ADR 018). Per-receipt attachment inspection spawns
   vendor CLIs (measured at seconds); the report path serves those
   verdicts through the inspection cache (ADR 024), so every screen —
   not just the dashboard — sees real attachment state in steady state.

## Decisions

### D1. Semantic states are produced by the Application

`UzeApplication::overview_workspace(cwd)` returns:

- `ProjectOverview { environment, memory, declared_plugins,
  installed_plugins, missing_plugins }` — always present, even when a
  directory has no `agents.lock` (that is `NotConfigured`, a real state).
- `OverviewMarketplace { name, package_count, invalid_packages, state }`
  — present for `Marketplace`/`Hybrid` kinds.
- The old file-oriented fields (`agents.lock`/`agents.json` rows,
  `.agents/` resource count, per-package listing, `Invalid` count) are
  removed from the projection. Evidence still exists in Doctor/Inspect.

### D2. `Environment: ready` is exactly what is provable

`Ready` ⇔ `agents.lock` exists, parses (supported version), and every
declared plugin is installed in the Store. That is fully provable from
the lock + Store index in ~1ms and is the condition the user acts on
(`uze install`). The design explicitly does NOT claim: plugin content
revision match (mostly absent in locks today), projection/
reconciliation state, or vendor-level attachment health — none of that
is cheaply provable, and the Doctor screen exists precisely for it.

### D3. `Memory` comes from `context_inspect`'s portability

Pure truth table, implemented as `derive_memory(agents_md,
portability)`:

| AGENTS.md | Portability            | Memory |
|-----------|------------------------|--------|
| yes       | Portable               | Ready  |
| yes       | PartiallyPortable etc. | Issue  |
| yes       | (inspection failed)    | Ready  |
| no        | VendorLocked           | Issue  |
| no        | NoContext / other      | None   |

### D4. Marketplace status

`Status` is `✓ valid` (manifest parses, all sources exist), `! N
invalid` (valid manifest, N declared sources missing/escaping), or `×
invalid manifest` (unparseable/unreadable). Marketplace health is
independent of whether its packages are installed globally.

### D5. Indicators have one meaning

`✓` verified+healthy · `!` attention/actionable · `×` error/invalid ·
`—` absent/not applicable/not configured. Quantities (`2 installed`,
`8 packages`) carry no check — color only when they diverge.

### D6. Layout: stacked rows, not columns

The workspace section always reads vertically — one PROJECT block, then
one MARKETPLACE block. Each block is width-capped (36 cells) so it stays
compact on wide terminals instead of stretching an empty line; the same
cap applies on narrow terminals, where the blocks naturally take the
full column. A pure consumer shows only PROJECT, a pure marketplace only
MARKETPLACE, a plain directory the three `—` rows and nothing else.

## Consequences

- TUI `overview_install_path` keys off
  `ProjectEnvironmentState::InstallRequired` — the Application's
  verdict — instead of re-deriving from lock/install counts.
- The previous `count_local_resources` core helper and the per-package
  `OverviewPackage` projection are removed with the file-level rows.
