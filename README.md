# UZE

**One agent environment. Any harness.**

UZE is a local compatibility and distribution layer for the agent-plugin
ecosystem. It installs an external package once, preserves its original
representation, identifies the capabilities it can safely understand, and
delivers them through the best surface each harness supports.

```text
External package
       |
       v
UZE Store
       |
       v
Package + capability planning
       |
       v
Claude Code · Codex · OpenCode · future peer integrations
```

UZE is not a vendor-to-vendor converter, a filesystem synchronizer, a runtime
proxy, a launcher, or a new plugin standard.

## Current v0

The current Rust implementation proves a package-first vertical slice using
one external Agent Plugin with a Skill and an MCP server. The package is stored
once and planned across Claude Code, Codex and OpenCode.

Its architectural baseline is:

- **Plugin First** — package is the distribution unit.
- **Capability Aware** — resources are compatibility units.
- **Native Plugin First** — a supported source envelope wins; safe capability
  fallback remains available when it does not.
- **Install Once** — Store is the local source of truth for package bytes.
- **Safe managed lifecycle** — typed receipts and live inspection prevent UZE
  from deleting harness state it cannot positively identify as managed.

See [ADR-008](docs/adr/008-adopt-plugin-first-capability-aware-delivery.md)
and [ADR-009](docs/adr/009-manage-harness-attachments-with-receipts-and-safe-reconciliation.md).

## Local workflow

For local development and a real machine installation, the repository also
ships a small Makefile:

```bash
make build
make test
make check
make install
```

`make install` invokes `cargo install --path . --bin uze --locked --force` and
therefore installs only the product binary into Cargo's configured binary
directory (usually `~/.cargo/bin`). It does not run `uze setup` or mutate any
harness configuration. Set `CARGO_INSTALL_ROOT` to select another install
location.

```bash
uze setup
uze add ./my-agent-plugin
uze list
uze inspect my-agent-plugin
uze doctor
uze remove my-agent-plugin
```

Running `uze` in an interactive terminal opens the minimal TUI. Explicit
subcommands remain the scriptable interface.

Packages are currently local-path Agent Plugins with root `plugin.json`. UZE
preserves the full source tree, including vendor-native envelopes; it does not
write a UZE plugin manifest. Remote registries and marketplaces are not part
of v0.

## Delivery support today

These are delivery facts, not a compatibility score.

| Harness | Package | Skill | stdio MCP |
|---|---|---|---|
| Claude Code | capability fallback | managed user-scope reference | managed Claude config |
| Codex | native when source has compatible Codex envelope | provided by package | provided by package |
| OpenCode | decomposed | native user/global discovery | managed OpenCode config |

Native planning consumes the package-provided resource identities, preventing
a duplicate Skill or MCP attachment. Hooks, agents, commands, remote
marketplaces, cloud state and runtime proxying are deliberately out of scope.

## Ecosystem watchlist

UZE is not limited to its first tracer bullets. The following labels express
research maturity, not implemented product integrations.

| Harness | Delivery research | Local conformance research | Next safe direction |
|---|---|---|---|
| Claude Code | native plugin | possible | Native Claude envelope when supplied; otherwise fallback. |
| Codex | native plugin | possible | Existing marketplace path; Responses spike required. |
| OpenCode | capability adapter | ready | Existing OpenAI-compatible local path. |
| Cursor | native plugin | possible | Agent Plugins native path; local provider still needs proof. |
| Windsurf | IDE extension required | contract only | Do not force IDE automation into Docker L2. |
| Gemini CLI | native extension | not currently testable | Native package adversary; zero-vendor model route is not established. |
| GitHub Copilot CLI | native plugin | ready | Strong next peer candidate with official local BYOK. |
| Cline | capability adapter | possible | Skills/MCP first; preserve executable plugin code. |
| Roo Code | capability adapter | contract only | Wait for maintained CLI evidence. |

The full sources, capability semantics, provider routes and Core impact are in
[the ecosystem research](openspec/changes/establish-local-real-harness-conformance/research-notes.md).

## Confidence tiers

| Tier | Evidence | Requirements |
|---|---|---|
| L0 — Unit | Pure Rust domain behavior | `cargo test`; no harness, model or network. |
| L1 — Contract | Store, planning, receipts and vendor config contracts | Isolated filesystem; no LLM. |
| L2 — Isolated real-harness E2E | Real CLI receives and exercises UZE-managed capability against local or routed test inference | Opt-in Docker/provider tooling; under research. |
| L3 — Vendor conformance | Real harness against official provider/model | Opt-in manual or release evidence. |

L2 is test infrastructure, not a UZE product dependency. A local or routed
model failure must never be reported as an attachment incompatibility.

## Architecture

```text
CLI ─┐
     ├── UzeApplication ── UZE Engine/Core ── peer integrations ── harnesses
TUI ─┘            |               |
                  |               └── EffectiveEnvironment + PackageExposurePlan
                  └── Store + ownership ledger/reconciliation
```

| Concept | Responsibility |
|---|---|
| Package | Distribution unit; preserved external bytes and identity. |
| Resource / capability | Compatibility unit; never a parallel package system. |
| Integration | Harness delivery, inspection and safe detach authority. |
| Store | Authoritative local installation/provenance, never harness artifacts. |
| Ledger | Expected ownership receipts, never live vendor state. |
| Application | Package-centric API shared by CLI and TUI. |

## Safety

Every managed side effect carries an attachment receipt. Before removal, UZE
asks the owning integration to inspect real state:

```text
MATCHED                       -> safe detach
MISSING                       -> nothing to delete
DRIFTED / CONFLICT / BLOCKED  -> preserve external state
```

UZE preserves plugin content during installation; it does not automatically
execute arbitrary plugin scripts or vendor extension code.

## Documentation

- [Architecture invariants](docs/architecture/invariants.md)
- [Testing](docs/testing.md)
- [Architecture decisions](docs/adr/README.md)
- [Plugin-first research](openspec/changes/reframe-plugin-first-portable-environment/research-notes.md)
- [Lifecycle consolidation](openspec/changes/consolidate-plugin-first-v0-experience/README.md)
- [Local real-harness conformance research](openspec/changes/establish-local-real-harness-conformance/research-notes.md)

## License

MIT. See [Cargo.toml](Cargo.toml).
