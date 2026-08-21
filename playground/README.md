# UZE playground

This is the deliberately small, evolving real-world test package for UZE.
It is not a second fixture system and it is not part of UZE's product core.

`make playground-lab` builds the current release binary and its local MCP
server, then deploys both plus `default-plugin/` into the WSL distro named
`Lab`:

```bash
make playground-lab

# In Lab
uze setup opencode
uze add ~/uze-playground/default-plugin
```

The deployed path is intentionally owned by this helper. On subsequent
deployments it is refreshed only when its `.uze-playground-managed` marker is
present; an unrelated directory at that path is preserved and causes the
deployment to stop.

## Default plugin

`default-plugin/` is one portable Agent Plugin containing:

- `uze-playground-plan`: short, explicit implementation planning;
- `uze-playground-review`: focused repository review;
- `uze-playground-release`: a safe local release checklist;
- `playground`: a stdio MCP server with deterministic `echo`, `add`, and
  `status` tools.

The `plugin.json` and `mcp.json` files are the portable representation. This
first playground package deliberately has no vendor envelope, so a clean Lab
can prove capability fallback without requiring every harness to be installed.

The MCP command is `uze-playground-mcp`, which the deployment installs in
`~/.local/bin`. Ensure that directory is on `PATH` before invoking a harness:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

For an explicit skill test in a harness, ask for the named skill, for example:

```text
Use the uze-playground-plan skill to create a three-step plan for adding a
small CLI command. Start with the skill's activation marker.
```

For MCP:

```text
Use the playground MCP tool `add` to calculate 19 plus 23. Include the tool
result in your answer.
```
