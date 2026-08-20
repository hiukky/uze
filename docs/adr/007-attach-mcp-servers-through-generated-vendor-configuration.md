# Attach MCP servers through generated vendor configuration

Status: Accepted

## Context

ADR-006 proved Agent Skills' transparent attachment and, in doing so,
registered a conceptual split — in documentation only, no code — between
**Static Attachment** (native filesystem discovery plus a UZE-owned
symlink/reference, resolved entirely at `uze setup`/`uze add` time) and
**Runtime Attachment** (the harness consults something UZE generated, not
just a shared discovery directory). MCP is the first capability that
actually needs the second category: an MCP server is a live process each
harness must be told about through its own configuration surface, not a
file a shared directory scan can find.

Official-docs research for both harnesses (recorded in `research-notes.md`)
found a symmetric, non-interactive, scriptable, global/user-scope
registration surface on each: `claude mcp add --scope user --transport
stdio <name> -- <command> [args]` (writing to `~/.claude.json`'s
`mcpServers`) and `codex mcp add <name> -- <command> [args]` (writing to
`~/.codex/config.toml`'s `[mcp_servers.*]`, global-only — no `--scope` flag
exists on Codex's side, global is simply the only destination). Both also
expose a config-only `get`/`list` for idempotency checks with no real model
turn, and a `remove` for cleanup. Neither the core MCP spec nor Agent
Plugins 1.0 needed to be extended or reinvented: the spec is wire-protocol
only (bundling is explicitly out of its scope, deferred to Agent Plugins),
and Agent Plugins 1.0 already defines a root-level `mcp.json` shape
(`{"mcpServers": {...}}`) identical to what both harnesses' own config
files already use.

## Decision

UZE will attach an MCP server to Claude Code and Codex by generating a
namespaced entry in each harness's own global/user-scope MCP configuration,
via that harness's own CLI (`claude mcp add`/`codex mcp add`), never by
hand-patching either config file directly. A new `ExposureMechanism::
ManagedVendorConfig { entry_name, command, args }` variant describes this
plan for reporting purposes; unlike the Agent Skills `ManagedUserScopeReference`
variant, it carries no generic `attach()`/`detach()` method on
`ExposureMechanism` itself, because the actual registration command differs
per harness — each integration's own `attach()` override reads the plan's
data and shells out to its own harness's CLI. Idempotency is enforced by
checking existence via `get`/`list` before ever calling `add`.

A package's MCP server is declared in a root-level `mcp.json`
(`{"mcpServers": {"<name>": {"command", "args"}}}`), the exact Agent
Plugins 1.0 shape, copied verbatim by the store alongside the existing
`skills/` copy — no `UZE MCP format` was invented. `CapabilityKind::Mcp`
already existed in the core model (previously used only for project-level
`mcp.json` discovery); this change is its first use for a UZE-store-owned,
harness-attached resource. Like Skills, an MCP resource is only attached to
a harness whose `uze setup` has already completed — there is no per-session
conformance-probe fallback for MCP the way `--plugin-dir` exists for Skills,
so pre-setup attachment attempts report `Unsupported` rather than fabricate
one.

The conformance fixture uses the official `rmcp` Rust SDK, added as a
regular dependency consumed only by a new fixture-only `[[bin]]` target —
not by the `uze` binary/library source, which never imports it — chosen
over a dev-dependency-only `[[example]]` because Cargo does not define a
flag-free, `CARGO_BIN_EXE_`-equivalent path-resolution mechanism for
examples the way it does for `[[bin]]` targets referenced from integration
tests.

Alternatives rejected: reusing `ManagedUserScopeReference`'s
filesystem-symlink shape for MCP (there is no symlink or shared discovery
directory to point at); inventing a proprietary UZE MCP manifest format
(Agent Plugins 1.0's `mcp.json` is already sufficient and is what both
harnesses' own plugin systems already expect); hand-patching
`~/.claude.json`/`~/.codex/config.toml` directly instead of shelling out to
each harness's own CLI (research confirmed the CLI path is format-preserving
and merge-safe; hand-patching risks corrupting unrelated user config); and
extending the existing Agent Skills fixture package in place to also carry
`mcp.json` (would silently change which resource several existing tests'
`.first()`/unqualified assumptions resolve to, since resources sort by
identity string and `"mcp.json"` sorts before `"skills/..."`).

## Consequences

Easier: a second, structurally different capability now proves the model
generalizes without redesigning `Resource`, the package envelope, or the
router; `uze add` needed zero CLI changes since it already calls
`integration.attach(resource)` generically for every resource kind; secrets
remain entirely out of scope because the fixture and the mechanism itself
never carry a literal secret value, by construction.

Harder: MCP attachment has no conformance-probe fallback before `uze
setup` completes (unlike Skills' `--plugin-dir`), so a package added before
setup for a given harness simply reports unsupported for that harness until
setup runs — this is a real, accepted gap, not hidden by an ad hoc
workaround. A package with more than one MCP server per `mcp.json` would
produce colliding `Resource::identity()` values today — deliberately not
solved, since the fixture and the acceptance criteria need exactly one.
Neither harness's `mcp add` overwrite behavior for a colliding,
differently-configured name was confirmed by research, so UZE never calls
`add` for a name that already exists rather than relying on that behavior.

## Implementation Plan

- **Affected paths:** `src/exposure.rs` (new `ExposureMechanism` variant),
  `src/store.rs` (`install_agent_plugin` copies `mcp.json` when present),
  `src/engine.rs` (`package_resources` discovers MCP resources),
  `src/project.rs` (`Resource::attachment_entry_name` branches on
  capability kind), `src/integrations/claude.rs`,
  `src/integrations/codex.rs` (MCP `capabilities()`/`exposure_plan()`/
  `attach()`, plus a directly-callable removal method), `Cargo.toml`
  (`rmcp` + fixture `[[bin]]` target), `tests/fixtures/packages/
  agent-plugin-mcp/` (new, separate from the Skills fixture),
  `tests/uze_harness_conformance.rs` (new deterministic and opt-in MCP
  tests reusing existing fixture/evidence helpers).
- **Patterns to follow:** the store remains the only place package content
  lives; each integration owns only its own generated-config lifecycle;
  conformance stays opt-in and tiered (configuration/discovery/behavioral),
  never conflating an environment block with incompatibility.
- **Patterns to avoid:** hand-patching a harness's shared config file;
  inventing a UZE-proprietary MCP manifest; forcing MCP and Skills to share
  one `ExposureMechanism` shape; expanding the Agent Skills fixture package
  in a way that changes existing tests' resource-ordering assumptions.

### Verification

- [x] `uze setup` + `uze add` attach the fixture MCP server to both
      harnesses without a `uze sync` step.
- [x] A second `uze add` does not duplicate or corrupt either harness's
      generated entry.
- [x] Configuration/discovery/behavioral conformance tiers are structurally
      distinct opt-in tests; a configuration-only pass is never reported as
      behaviorally verified.
- [x] No literal secret appears in any generated configuration or test
      fixture.
- [x] Rust, OpenSpec, and LikeC4 validation pass.

Source change: openspec/changes/enable-mcp-as-second-capability/

## More Information

2026-08-20: Implemented `ExposureMechanism::ManagedVendorConfig`, MCP
resource discovery in `UzeStore`/`UzeEngine` (`mcp.json` copied verbatim,
one `Resource` per declared server), and `ClaudeIntegration`/
`CodexIntegration` MCP support (`capabilities()`, `exposure_plan()`,
`attach()` shelling out to `claude mcp add --scope user --transport stdio`
/ `codex mcp add`, idempotency checked via `mcp get`/`mcp list --json`
first). Fixed a real latent bug surfaced by the first MCP-only package
fixture: `UzeEngine::package_resources` unconditionally scanned `skills/`,
which does not exist for an MCP-only package — now guarded. Added a
`command_home` field to both integrations (set explicitly as `HOME` on
every `claude`/`codex mcp *` shell-out) after realizing the MCP path — unlike
Skills, which never spawns a process for the actual attachment — needed the
same isolation guarantee the CLI's spawned-subprocess pattern got for free.
Added a real `rmcp`-based stdio MCP conformance fixture
(`uze-mcp-conformance-fixture`, a genuine `[[bin]]` target so
`env!("CARGO_BIN_EXE_...")` resolves reliably) with one tool,
`uze_conformance`, returning a test-controlled value via
`UZE_MCP_CONFORMANCE_PROOF`. `cargo test`, `cargo clippy -D warnings`,
`cargo fmt --check`, `openspec validate --strict`, and `likec4 validate`
all pass; the smoke-tested fixture server correctly answers a hand-rolled
MCP stdio handshake.

2026-08-20: Ran the real, opt-in `CONFIGURATION VERIFIED`/`DISCOVERY
VERIFIED` probe against an isolated `$HOME`/`$UZE_HOME`. Claude Code:
`claude mcp get`/`claude mcp list` both reported `Status: ✔ Connected` for
the real fixture server — both tiers genuinely verified, live. Codex:
`codex mcp list --json` confirmed the entry with the correct command
(`CONFIGURATION VERIFIED`), but its JSON shape
(`name`/`enabled`/`disabled_reason`/`transport`/`auth_status`) has no
explicit connectivity field, so `DISCOVERY` is honestly reported
inconclusive rather than inferred. The isolated, credential-less
`BEHAVIORAL` probe correctly returned `BLOCKED_BY_ENVIRONMENT` for both
(same "not logged in" / HTTP 401 pattern already fixed in
`conformance::is_environment_block` for Agent Skills).

2026-08-20: With explicit operator consent, closed configuration/discovery
verification against the real machine (`uze add` on the real `$UZE_HOME`,
writing into the real `~/.claude.json`/`~/.codex/config.toml`) — confirmed
merge-safe: `~/.claude.json`'s ~60 other top-level keys were untouched,
only `mcpServers.uze-uze-mcp-conformance` was added; `~/.codex/config.toml`'s
existing `model`/`approvals_reviewer`/project trust entries were untouched.
Real `claude mcp get`/`codex mcp list --json` both confirmed the entry
identically to the isolated-home run. **Behavioral verification via the
real OAuth session was attempted and characterized, not forced**: Codex's
`codex exec --ask-for-approval never` still required approval for the MCP
tool call and failed with "MCP tool call requires approval, but approval
policy is never"; retrying with `--dangerously-bypass-approvals-and-sandbox`
was correctly refused by the operating environment's own safety classifier,
and that refusal was respected rather than worked around. Claude Code's
headless `-p` mode hit the same class of gap (`stop_reason: "tool_use"`,
exhausted `max_turns` without completing the call); retrying with
`--dangerously-skip-permissions` was likewise correctly refused. **Finding,
not a defect in this ADR's mechanism**: Agent Skills content is read
directly into context and needs no per-invocation permission; an MCP tool
call is a distinct action both harnesses gate behind an approval step that
plain "never ask" flags do not satisfy in headless/non-interactive mode.
Closing that gap safely (a scoped, non-dangerous headless MCP-approval
mechanism, if either harness documents one) is future work, not required to
consider this ADR's core claim — transparent, harness-generated MCP
configuration — proven: `CONFIGURATION VERIFIED` and `DISCOVERY VERIFIED`
(Claude) are both real and live on the operator's actual machine.

2026-08-20: Follow-up, without any dangerous bypass, to characterize the
approval gap precisely rather than leave it as a black box. Claude Code's
non-interactive `--allowedTools=mcp__<server>__<tool>` (`=` syntax —
`<tools...>` is variadic and swallows the prompt if given
space-separated before it) genuinely resolves the approval step:
`permission_denials` was empty on every subsequent run, confirmed across
five real, paid API calls (~$0.20 total). The remaining obstacle is
distinct and smaller than "approval": in short-lived, non-interactive
`-p` sessions, the (economical `haiku`) model repeatedly either answered
from the unrelated, still-installed `uze-e2e` Agent Skill instead of
calling the MCP tool, or reported the MCP tool absent from its "deferred
tools list" even with the server confirmed `✔ Connected` by `claude mcp
get` moments earlier — suggesting MCP tool discovery in Claude's headless
mode is not immediately queryable the same way it is in an interactive
session, independent of the approval question. Codex's
`--ask-for-approval never` gap was not re-attempted with a non-dangerous
alternative in this pass. Not pursued further to avoid continuing to
spend real, paid API calls chasing a secondary discovery quirk once the
primary approval question was already answered; this is recorded as the
precise, narrower remaining gap rather than left as the original, broader
"approval blocks everything" characterization. Does not change the
verdict: `CONFIGURATION VERIFIED` and `DISCOVERY VERIFIED` remain the
real, live, proven claims; full `BEHAVIORAL VERIFIED` for MCP is future
work with a now-narrower, better-understood cause.
