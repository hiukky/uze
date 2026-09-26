## Why

The user-flow test of the authoring surface ran in a sandbox with no harness
detected, so the four real harness projections were exercised for the first
time on the operator's own machine — where the two delivery bugs of this
branch (`${PLUGIN_ROOT}` vs `${CLAUDE_PLUGIN_ROOT}`, the MCP stub the scaffold
never wrote) surfaced as live Errors in opencode and Claude. A clean, complete
real-harness verification across claude, codex, opencode and antigravity is
the pending gate. The tasks are in `tasks.md`.
