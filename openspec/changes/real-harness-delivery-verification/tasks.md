## 1. The clean sweep

- [ ] 1.1 `make install` from this branch (fixes: executable MCP stub, `${CLAUDE_PLUGIN_ROOT}` in the claude envelope, resolved path in the opencode entry)
- [ ] 1.2 Clean the test state: `uze remove hello -m` (any drift block, `uze doctor` names it); leftover `~/uze/boo-market`, `~/boo`, vendor entries
- [ ] 1.3 The 4 Lab verticals with real harness binaries, synthetic world, zero internet: `python3 conformance/lab.py --harness <claude|codex|opencode|antigravity>` — skills, agents, MCP and removal, one named check per outcome
- [ ] 1.4 Failures become fixes through the `--sandbox` loop, guided by the `conformance-debug` skill; `verdict.json` is the evidence
- [ ] 1.5 NEW — codex: the delivered MCP declares `${PLUGIN_ROOT}` and codex does not resolve it (the same Error opencode/claude had); the codex integration translates the variable to the Store's real path (or codex's own grammar) in the generated `.mcp.json`
- [ ] 1.6 NEW — antigravity: the MCP does not even appear listed in agy; verify the staging `~/.gemini/config/plugins/<name>/mcp_config.json` (the route is `agy plugin install` staging bytes — see `conformance/DECISIONS.md`)
- [ ] 1.7 Deferred from this session, in order: S1 (description as a double-quoted YAML scalar + check refusing a truncated block), detach residue (orphaned generated trees, `.git/uze-write.lock`), `update` output truncation, empty `Delivery` section on install

- [ ] 2.1 SELF-HEAL — re-add links: `market add <local path>` links to the path by default (a local checkout is the origin of its own truth); `agent market create` stays the one-command create
- [ ] 2.2 SELF-HEAL — the mirror never answers for a local source: a marketplace registered from a local repo refreshes its mirror from that repo before any read/install — stale mirror "unknown package" becomes impossible; the catalog and the install can never disagree
- [ ] 2.3 SELF-HEAL — `uze doctor` resolves known-intervention drift in one command: reconcile, clean what the receipts prove stale, reinstall — and reports what it did; blocks remain only for drift no one can attribute

## Notes

- The PR review (20 findings, file:line) was delivered this session; the user flow that surfaced the projection bugs is in the transcript (author-persona subagent, world `/tmp/uze-user-test`).
- `make lab-replay` covers replay; `conformance-stability.yml` is the nightly promotion gate.
