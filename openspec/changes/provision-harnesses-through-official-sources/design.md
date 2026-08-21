## Design

This change adds an explicit **harness bootstrap layer** above the existing
package delivery path. It does not turn UZE into a universal runtime/version
manager: it covers only registered peer CLI integrations and invokes only
their official vendor routes.

```text
CLI / TUI
    │
UzeApplication::setup
    │
Integration-owned provision → detect/verify → existing prepare
    │                                               │
    └────────── secret-free provisioning state      └─ package exposure + receipts
```

`IntegrationPort` remains the vendor boundary. It receives additive
provisioning operations (or delegates them to a small integration-owned
provisioner) that produce typed facts: absent/present, action attempted,
verified version, supported/blocked, and an actionable reason. The Application
coordinates selection, calls the existing `install` preparation only after a
verified executable, and replays normal package delivery only for the selected
integration. Core types never contain a vendor URL, shell snippet, or config
schema.

`setup` chooses the official latest-stable route. On Unix, Claude Code,
Codex, and OpenCode have documented first-party install scripts; Windows uses
their documented PowerShell or official package-manager routes where a script
is not supplied. Gemini uses its documented npm route. For an existing
executable, the integration uses its own documented update command where that
is reliable; if UZE cannot safely establish the method/platform, it reports a
blocked result rather than guessing.

The command runner is injectable. Production invokes a process without a
shell unless an official installer necessarily requires one; tests assert the
exact approved command specification through a fake runner. Timeouts, output
redaction, version verification, and nonzero exits become structured results.
Normal tests never run a network installer.

`$UZE_HOME/state/provisioning.json` records only UZE-initiated action,
integration id, platform/method, executable identity when observed, version,
and time/outcome. It is distinct from `attachments.json`: a provision record
does not grant detach/removal authority. This change adds no public harness
uninstall command. That needs a future explicit ownership and vendor-uninstall
decision.

`uze add` retains the DX fix already on `main`: it prepares detected
integrations so immediate attachment works, but it never invokes provision or
network operations. This keeps scripted plugin installation predictable.

### Architectural decision

An ADR is required: vendor executable provisioning is an enduring new product
responsibility, but it must remain integration-owned and separate from both
package provenance and managed attachment ownership. The LikeC4 model must
show the Application coordinating the optional official vendor provision path.
