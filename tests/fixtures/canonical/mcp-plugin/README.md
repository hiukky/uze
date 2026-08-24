# Agent Plugin MCP Package Fixture

A clean external Agent Plugins 1.0 package (see ADR-007) declaring exactly
one MCP server, no Agent Skill. Kept separate from
`tests/fixtures/canonical/skill-plugin/` deliberately: extending that
package in place would change which resource several existing tests'
`.first()`/unqualified assumptions resolve to, since resources sort by
identity string and `"mcp.json"` sorts before `"skills/..."`.

`mcp.json`'s `command` field is a placeholder
(`__UZE_MCP_FIXTURE_BINARY__`) — tests that need to actually spawn the
server rewrite it to the real, test-build-resolved path of the
`uze-mcp-conformance-fixture` binary (`env!("CARGO_BIN_EXE_
uze-mcp-conformance-fixture")`) before installing this fixture into a
store. Tests that only exercise resource discovery/composition can use the
placeholder as-is.

The fixture server itself
(`e2e/fixtures/bin/mcp_conformance_fixture.rs`) is a minimal, real stdio
MCP server built with the official `rmcp` Rust SDK. Its one tool,
`uze_conformance`, returns whatever `UZE_MCP_CONFORMANCE_PROOF` was set to
at server-launch time — a value the *test* controls, not a hardcoded
constant, and deliberately independent of the Agent Skills `PROOF` token so
the two conformance suites stay separately verifiable. No network,
database, external API, credentials, or LLM is involved.
