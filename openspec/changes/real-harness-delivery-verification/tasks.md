## 1. The clean sweep

- [ ] 1.1 `make install` from this branch (fixes: executable MCP stub, `${CLAUDE_PLUGIN_ROOT}` in the claude envelope, resolved path in the opencode entry)
- [ ] 1.2 Clean the test state: `uze remove hello -m` (any drift block, `uze doctor` names it); leftover `~/uze/boo-market`, `~/boo`, vendor entries
- [ ] 1.3 The 4 Lab verticals with real harness binaries, synthetic world, zero internet: `python3 conformance/lab.py --harness <claude|codex|opencode|antigravity>` — skills, agents, MCP and removal, one named check per outcome
- [ ] 1.4 Failures become fixes through the `--sandbox` loop, guided by the `conformance-debug` skill; `verdict.json` is the evidence
- [ ] 1.5 Deferred from this session, in order: S1 (description as a double-quoted YAML scalar + check refusing a truncated block), detach residue (orphaned generated trees, `.git/uze-write.lock`), `update` output truncation, empty `Delivery` section on install

## Notes

- The PR review (20 findings, file:line) was delivered this session; the user flow that surfaced the projection bugs is in the transcript (author-persona subagent, world `/tmp/uze-user-test`).
- `make lab-replay` covers replay; `conformance-stability.yml` is the nightly promotion gate.
