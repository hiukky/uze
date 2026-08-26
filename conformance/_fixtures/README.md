# Conformance fixtures

`marketplace/` is the complete, isolated marketplace installed by the Harness
Conformance Lab. It intentionally represents final user-facing resources,
instead of reusing the small single-shape inputs in `tests/_fixtures/`.

Every vertical receives a fresh copy of this marketplace and installs the
resources it exercises. The four shared resources cover the portable surface:

- `flow:commit` — default model and user invocation.
- `flow:review` — user-only invocation.
- `flow:analyze` — model-only invocation.
- `uze-mcp-conformance` — one real stdio MCP server.

`mcp.json` contains two runtime placeholders. The Lab replaces them in its
disposable copy with the fixture-server path and per-run proof before UZE
reads the marketplace. Do not point production or deterministic tests here;
their independent fixture tree remains `tests/_fixtures/`.
