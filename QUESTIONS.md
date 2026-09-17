# Questions

Decisions that are yours. Each carries a recommendation; work continued
on something else meanwhile.

## 1. Should `uze-core` be free of I/O?

The brief asks for a pure-domain core. `AGENTS.md` and the code place
`machine/` (process spawning, PATH, detection cache) and `delivery/persistence`
(atomic writes, flock) inside `uze-core` on purpose, and the architecture
suite enforces vendor neutrality rather than I/O purity.

Moving I/O out means a new crate between core and application, and every
integration's signature changes. **Recommendation:** keep the current
boundary; it is tested and coherent. Revisit only if a second consumer of
the domain appears that cannot do I/O.

## 2. Crate roles in the brief vs the repository

The brief names `uze-extensions` as the harness layer. Harnesses live in
`uze-integrations`; `uze-extensions` holds TUI extensions (`code`). No
action taken; noting it so nothing is moved on that assumption.

## 3. The integration worktrees under `.worktrees/simplify*`

Seven checkouts (`simplify`, `simplify-{core,terminal,crates,cli,workspace,
integrations,application}`) and their `agent/simplify-*` branches were used
to build this refactor; all of their work is merged into `feat/space-kinds`.
The guardrails forbid removing worktrees, so they remain.
**Recommendation:** `git worktree remove` them and delete the branches.

## 4. Install `hyperfine` and `perf` on this machine?

Performance work in the brief is gated on measurement. Neither is
installed; `cargo-llvm-cov` and `cargo-deny` are. **Recommendation:** yes,
both are dev-only tools outside the repository.
