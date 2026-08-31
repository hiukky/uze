# UZE — the official UZE Skill

This is a normal Agent Plugins 1.0 package. It carries no special treatment
in the Store, the router, or any integration — it is installed, discovered,
and delivered exactly the way any other Skill-only package is
(`uze add ./plugins/uze`).

It ships two Skills: `skills/init/SKILL.md`, an agentic orchestrator that
calls UZE's own deterministic `uze context inspect|plan|reconcile` CLI to
make a project's instructions context portable; and
`skills/worktree/SKILL.md`, which coordinates isolated worktrees for
concurrent agent implementation and safe primary-branch integration, honoring
the project's `agents.lock` `worktrees_dir` when configured. See
[`docs/capabilities/context-manager.md`](../../docs/capabilities/context-manager.md)
for the architecture this Skill sits on top of, and
[`docs/capabilities/uze-skill.md`](../../docs/capabilities/uze-skill.md) for
how it's invoked per harness (`/uze:init`), its self-hosting proof, and its test coverage.
