## 1. The clean sweep

- [ ] 1.1 `make install` from this branch (fixes: executable MCP stub, `${CLAUDE_PLUGIN_ROOT}` in the claude envelope, resolved path in the opencode entry)
- [ ] 1.2 Clean the test state: `uze remove hello -m` (any drift block, `uze doctor` names it); leftover `~/uze/boo-market`, `~/boo`, vendor entries
- [ ] 1.3 The 4 Lab verticals with real harness binaries, synthetic world, zero internet: `python3 conformance/lab.py --harness <claude|codex|opencode|antigravity>` — skills, agents, MCP and removal, one named check per outcome
- [ ] 1.4 Failures become fixes through the `--sandbox` loop, guided by the `conformance-debug` skill; `verdict.json` is the evidence
- [x] 1.5 `${PLUGIN_ROOT}` in a canonical `mcp.json` is resolved to the Store path by one resolver (`shared::mcp::resolve_package_root`) for every harness and every route — the generated envelopes (claude, codex, antigravity) and the managed config entries (opencode, and every capability-level fallback). The earlier per-vendor rewrites (`${CLAUDE_PLUGIN_ROOT}`, codex's raw-text replace, the `scripts/` symlinks) are gone; opencode and antigravity had never been fixed
- [x] 1.6 antigravity: plugin MCP servers are not listed by `agy mcp list` (global config only) — `agy plugin list` shows the `mcpServers` component; the staged `mcp_config.json` now carries the resolved path
- [x] 1.6a The scaffolded MCP stub is a working stdio server (initialize, tools/list, tools/call); the previous one printed a line and exited, which every harness reports as a failed connection
- [x] 1.6b The Lab's `mcp-plugin` fixture ships its server under `${PLUGIN_ROOT}/scripts/server`, so every MCP check in all four verticals exercises the resolution
- [ ] 1.7 Deferred from this session, in order: S1 (description as a double-quoted YAML scalar + check refusing a truncated block), detach residue (orphaned generated trees, `.git/uze-write.lock`), `update` output truncation, empty `Delivery` section on install

- [ ] 2.1 SELF-HEAL — re-add links: `market add <local path>` links to the path by default (a local checkout is the origin of its own truth); `agent market create` stays the one-command create
- [ ] 2.2 SELF-HEAL — the mirror never answers for a local source: a marketplace registered from a local repo refreshes its mirror from that repo before any read/install — stale mirror "unknown package" becomes impossible; the catalog and the install can never disagree
- [ ] 2.3 SELF-HEAL — `uze doctor` resolves known-intervention drift in one command: reconcile, clean what the receipts prove stale, reinstall — and reports what it did; blocks remain only for drift no one can attribute

## Notes

- The PR review (20 findings, file:line) was delivered this session; the user flow that surfaced the projection bugs is in the transcript (author-persona subagent, world `/tmp/uze-user-test`).
- `make lab-replay` covers replay; `conformance-stability.yml` is the nightly promotion gate.
