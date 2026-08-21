# Gemini native conformance fixture

One preserved external package with a **portable core** and an **optional
vendor enhancement**:

```
plugin.json             portable Agent Plugin manifest
mcp.json                portable MCP declaration
skills/                 portable Agent Skill
gemini-extension.json   Gemini vendor enhancement (optional)
```

`gemini-extension.json` is a *source-provided* envelope, exactly like the
Codex fixture's `.codex-plugin/plugin.json`. UZE never synthesizes it: a
package without one has no native route into Gemini and is decomposed
capability by capability instead. That is the whole point of keeping both
halves in one fixture — removing the vendor half must change the delivery
route and nothing else.

Gemini declares MCP servers *inside* its manifest rather than in a sibling
`mcp.json`, so the same server appears in both. That duplication is the
vendor's shape, not a UZE format, and the Store preserves both files byte for
byte.
