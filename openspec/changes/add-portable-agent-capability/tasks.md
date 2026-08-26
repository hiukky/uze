## 1. Canonical capability and resource model

- [x] 1.1 Discover `agents/<name>.md` from package and project inputs, compose
  one `CapabilityKind::Agent` resource per definition, and preserve bytes.
- [x] 1.2 Define Agent resource identity, display name, inspection payload,
  and capability routing tests without adding vendor knowledge to Core.
- [ ] 1.3 Extend route/identity/conformance fixtures for supported, adapted,
  unsupported, collision, and byte-preservation cases.

## 2. Native delivery

- [ ] 2.1 Implement Claude Code native Agent exposure, receipts, inspection,
  safe detach, drift behavior, and focused integration tests.
- [ ] 2.2 Implement OpenCode native Agent exposure, receipts, inspection,
  safe detach, drift behavior, and focused integration tests.
- [ ] 2.3 Implement Antigravity CLI native Agent exposure, receipts,
  inspection, safe detach, drift behavior, and focused integration tests.
- [ ] 2.4 Implement Codex's generated native TOML Agent projection, receipts,
  inspection, safe detach, and focused integration tests.
- [ ] 2.5 Reuse qualified naming and detect cross-harness physical-root
  conflicts before mutating any Agent artifact.

## 3. Product reporting and documentation

- [x] 3.1 Add the Agents capability to TUI compatibility rows and verify
  native/adapted status rendering.
- [x] 3.2 Update the harness-matrix generator and regenerate README.md so all
  four harnesses are Native for Agents.
- [x] 3.3 Confirm `docs/adr/031-adopt-a-canonical-portable-agent-capability.md`
  remains present and linked from the ADR index.

## 4. Conformance and verification

- [ ] 4.1 Add a small canonical Agent fixture and deterministic integration /
  acceptance coverage for composed multi-capability packages.
- [ ] 4.2 Add isolated Agent discovery and negative-isolation scenarios to
  each Claude, Codex, OpenCode, and Antigravity conformance vertical.
- [ ] 4.3 Run each conformance vertical against its real harness and fix all
  failures before promoting matrix evidence.
- [x] 4.4 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --no-fail-fast`, and `openspec validate add-portable-agent-capability --strict`.
