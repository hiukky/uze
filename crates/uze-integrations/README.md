# uze-integrations

One vertical per harness, each implementing `uze_core::IntegrationPort` — the
only contract Core knows. The single place in the workspace allowed to name a
vendor.

- [Claude Code](src/claude/README.md)
- [Codex](src/codex/README.md)
- [OpenCode](src/opencode/README.md)
- [Antigravity CLI](src/antigravity/README.md)
- `shared/` — cross-vendor process and path helpers

`registry::IntegrationRegistry` is the single composition root: `builtin` for
the real environment, `isolated` for tooling and tests. No integration imports
another. A new harness means a vertical, a registry entry, conformance and
docs — nothing in core, application, CLI or TUI.

```bash
cargo test -p uze-integrations
python3 conformance/lab.py --harness claude    # real-binary evidence
```

Delivery precedence per capability: Native > Generated Native > Safe
Adaptation > Unsupported. The current matrix is generated — see
`cargo run --bin uze-harness-matrix`.
