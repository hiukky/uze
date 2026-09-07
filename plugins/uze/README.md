# UZE — the official UZE Skill

This is a normal Agent Plugins 1.0 package. It carries no special treatment in
the Store, the router, or any integration — it is installed, discovered and
delivered exactly the way any other Skill-only package is.

It ships two Skills:

- `skills/init/SKILL.md` — an agentic orchestrator that calls UZE's own
  deterministic `uze context inspect|plan|reconcile` CLI to make a project's
  instructions context portable.
- `skills/worktree/SKILL.md` — coordinates isolated worktrees for concurrent
  agent work and safe integration, honouring the `worktrees:` policy the
  project declares in `agents.yaml`.

See [`docs/capabilities/context-manager.md`](../../docs/capabilities/context-manager.md)
for the architecture these sit on top of, and
[`docs/capabilities/uze-skill.md`](../../docs/capabilities/uze-skill.md) for how
they are invoked per harness, the no-special-treatment proof, and what is not
tested.
