## Decision and research

- [x] Record ADR-010 for official, integration-owned harness provisioning.
- [x] Record the official install/update/verification routes and platform
      restrictions for Claude Code, Codex, OpenCode, and Gemini CLI; include
      source links and do not infer missing Windows routes.

## Provisioning boundary

- [x] Add typed, integration-owned provision/detect/verify results and an
      injectable process runner; keep vendor commands out of Core and the
      Application.
- [x] Add atomic, secret-free provisioning state under `$UZE_HOME/state`,
      separate from `attachments.json` and integration setup state.
- [x] Evolve application setup orchestration: provision selected harness,
      verify, prepare, republish, then deliver already stored packages to only
      that integration.
- [x] Preserve `add` as an offline/no-provision operation that only prepares
      detected harnesses.

## Peer implementations

- [ ] Implement and contract-test supported Unix/WSL routes for Claude Code,
      Codex, OpenCode, and Gemini CLI.
- [ ] Implement documented Windows/macOS routes only where the vendor exposes
      a safe official automation path; return actionable unsupported results
      otherwise.
- [ ] Capture version and provision provenance without credentials or complete
      command output.

## Product presentation and safety

- [ ] Surface install/update/verify/prepare outcomes in CLI and the existing
      TUI using Application read models only.
- [ ] Ensure provision failure never records a prepared integration or
      attachment receipt, and does not remove packages or external artifacts.
- [ ] Do not implement harness removal; document the future ownership gate.

## Verification

- [ ] Test missing → official install → verify → prepare in isolated HOME and
      UZE_HOME with a fake process runner.
- [ ] Test present → official update → verify → prepare and update failure →
      blocked/no preparation.
- [ ] Test package added before harness provision is delivered by later setup
      without duplicate native/capability attachments.
- [ ] Test `add` never invokes a provision command, including when all
      harnesses are absent.
- [ ] Run cargo test, cargo clippy -- -D warnings, cargo fmt --check,
      openspec validate --all --strict, likec4 validate, and git diff --check.
