Sequenced **after** `plugin-freshness-and-linked-marketplaces` — see
design.md, "Ordering against the freshness change".

## 1. What a project is

- [ ] 1.1 `resolve_project_root` returns an absence instead of the working
      directory, and its precedence becomes `agents.yaml` → repository root
      → `AGENTS.md` (`crates/uze-core/src/project/project_root.rs`)
- [ ] 1.2 Rewrite `fallback_is_cwd_when_no_markers` (`:81`) to assert
      absence, and `prefers_agents_md_over_git` (`:110`) to assert the new
      precedence; update the module doc, which states the cwd fallback as
      intended design
- [ ] 1.3 Add the case nothing tests today: `AGENTS.md` in a subdirectory of
      a repository does not become the project root. Add a worktree case
      (`.git` as a file) beside it
- [ ] 1.4 Answer absence at each of the five `resolve_project_root(…)?`
      sites in `project_environment.rs` (`:42`, `:143`, `:211`, `:269`,
      `:544`) — machine-only or refuse, one decision each, stated in the
      code's own words

## 2. The verbs

- [ ] 2.1 Remove `PluginAction` and its dispatch; move install, remove,
      update, inspect to the root (`src/main.rs`)
- [ ] 2.2 `-m` / `--machine` on the verbs with two scopes; identical meaning
      on each; accepted with or without a project
- [ ] 2.3 `--alias` / `--replace` move to the root install form, keeping
      ADR-036's collision resolution reachable
- [ ] 2.4 Every verb reports the scopes it changed, including "nothing was
      declared"
- [ ] 2.5 `UzeError::PluginNotUsedByProject` and `NoProjectEnvironment`
      (`error.rs:200-211`) name `-m` instead of a command that no longer
      exists
- [ ] 2.6 Reclassify every leaf in `src/command_performance.rs`;
      `every_cli_command_is_classified` passes and no class is a lie
- [ ] 2.7 `uze --help` states the scope rule once, rather than per command
- [ ] 2.8 `uze status` with no project is a machine read model, not
      `ProjectLockStatus::Absent` reported as a fault; `uze update` with no
      project re-resolves every installed package from its own source

## 3. The rest of the workspace

- [ ] 3.1 `conformance/harnesses/{claude,codex,opencode,antigravity}/scenarios.py`
      use `uze <p>@uze-lab -m`; update `conformance/DECISIONS.md:529` to say
      the guarantee is now explicit rather than positional
- [ ] 3.2 `make lab-replay` and one real vertical pass
- [ ] 3.3 `plugins/uze/skills/**` and `docs/**` carry the new spellings
- [ ] 3.4 Fix the ~18 `ADR-038` citations that mean **ADR-036**
      (`naming.rs`, `store.rs`, `install.rs`, `update.rs`, `read_models.rs`,
      `marketplace.rs`, `application.rs`, `tests.rs`, `fake_harness.rs`) —
      ADR-038 is the terminal runtime server. Leave the ones in
      `uze-terminal`, `src/ui*`, `prompt_history.rs`, which are correct

## 4. Journeys

- [ ] 4.1 `02-packages/01`, `02`, `03` — `03`'s own prose teaches the scope
      split and has to be rewritten, not just its commands
- [ ] 4.2 `03-context/01`, `01-first-run/01`
- [ ] 4.3 `06-recovery/01-drift-blocks-a-destructive-removal.yml` — the
      claim survives as `uze remove <p> -m`; its narration at `:113`
      explains the old semantics and is rewritten
- [ ] 4.4 New: using a plugin outside any project declares nothing and is
      still resolved by every harness — the `then` reads
      `~/.agents/skills/…` and the absence of `agents.yaml` anywhere
- [ ] 4.5 New: `-m` inside a project installs and declares nothing —
      `agents.lock` byte-identical across the command
- [ ] 4.6 `journey validate` passes and `journey list` reads correctly

## 5. Gate

- [ ] 5.1 `docs/architecture/invariants.md` — the scope rule and the anchor
      rule, each naming the test that holds it
- [ ] 5.2 `make check` (fmt, clippy `--all-targets -D warnings`, workspace
      tests, coverage floor, `cargo deny`, `openspec validate --all
      --strict`)
- [ ] 5.3 Verify by hand: adopt UZE in a fresh repository, use a plugin from
      `$HOME`, and confirm neither wrote where the other should have
