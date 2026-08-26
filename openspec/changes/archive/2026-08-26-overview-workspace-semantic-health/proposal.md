## Why

The TUI Overview answers the wrong question. After the first workspace-
awareness pass its PROJECT/MARKETPLACE blocks read like a file
inspection:

```
PROJECT                       MARKETPLACE
agents.lock    ✓ 1 plugin     agents.json    ✓ valid
Installed      1/1            plugins/       ✓ 1 package
Context        ready          Invalid        0
AGENTS.md      ✓ present      Marketplace    uze
```

Technically correct, product-wise empty: the names `agents.lock`,
`agents.json`, `plugins/`, `Invalid` leak implementation artifacts, and
almost every row carries a green check that only proves "this value
exists". The screen is supposed to answer *"what kind of workspace am I
looking at, and is it ready to work?"* — evidence and implementation
belong to Doctor/Inspect, not to the Overview. It also wastes horizontal
space: two 48% columns on a wide terminal leave a large empty gutter
between them.

This change raises the Overview to semantic states, with the *states* —
not the file facts — produced by the Application layer and rendered
verbatim by the TUI. The TUI never derives `ready` from lock bytes.

## What Changes

- `UzeApplication::overview_workspace` returns semantic projections:
  `ProjectOverview { environment, memory, declared_plugins,
  installed_plugins, missing_plugins }` and
  `OverviewMarketplace { name, package_count, invalid_packages, state }`.
- `Environment` states: `NotConfigured` / `Invalid` / `InstallRequired` /
  `Ready`. `Ready` = valid `agents.lock` with every declared plugin
  installed — provable cheaply; attachment/reconciliation state is
  explicitly NOT claimed (that is Doctor's vendor-inspected territory).
- `Memory` states: `None` / `Ready` (`✓ AGENTS.md`) / `Issue` (bridge
  gap or vendor-only context).
- `Plugins` becomes a quantity (`2 installed`), colored only when it
  diverges (`! 1/2 installed`).
- MARKETPLACE becomes `Name` / `Plugins` / `Status`
  (`✓ valid` / `! 2 invalid` / `× invalid manifest`).
- Indicator semantics fixed: `✓` = verified and healthy, `!` = attention,
  `×` = error/invalid, `—` = absent/not applicable/not configured.
- Layout: the workspace section always reads vertically — one PROJECT
  block, then one MARKETPLACE block — each width-capped (36 cells) so the
  block stays compact on wide terminals; no dead gutter, no columns.
- A plain directory is a first-class state: `Environment — not
  configured`, `Memory — none`, `Plugins — none`, no MARKETPLACE block,
  no verbose onboarding text.
- File-level details removed from the Overview: no `agents.lock`,
  `agents.json`, `plugins/`, `Invalid` count, `.agents/` resource count,
  or package listing rows. Those remain in Doctor/Inspect.

## Impact

`crates/uze-application/src/application/overview.rs` (read model),
`src/ui/view/overview.rs` (render), `src/ui/model.rs` +
`src/ui/worker.rs` (install action driven by the state, not by
re-derived facts). No CLI grammar or Store semantics change.
