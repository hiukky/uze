## Why

UZE's thesis is one sentence: a capability declared once is delivered
natively to every harness. The Harness Conformance Lab is the only place
that thesis meets a real binary — and it does not assert it.

Measured across the four verticals: **42 distinct checks, and exactly three
appear in all four** — `tui-reached-prompt`, `provider-request-captured`,
`deterministic-response-rendered`. All three are liveness. None says
whether a Skill works. **28 of 42 (67%) exist in exactly one harness**, and
the counts are uneven for no product reason: Claude 10, Codex 16, OpenCode
15, Antigravity 24.

The cause is not neglect. Each vertical asserts the **mechanism** a vendor
happens to use — `policy-sidecar-delivered` (a Codex file),
`mcp-surface-in-tui` (a string on a screen), `model-only-skill-hidden-from-tui`
(Antigravity only). Mechanism diverges *by design*: that is the whole
`Native > Generated Native > Safe Adaptation` precedence. Assert mechanism
and the suites must diverge.

The deterministic suite already knows better. `tests/integrations/identity.rs`
states that every assertion there is "phrased as an *outcome* invariant …
never as 'the JSON must look like X.'" The Lab violates the principle its
own sibling follows.

What that costs was just demonstrated. Every `hooks-*-denial-blocks-tool`
asserted the absence of the *allow* scenario's marker — a string the deny
scenario can never print — so a turn where no hook ran passed every deny
and order check, on three harnesses, for months. A check that exists in one
vertical has nobody to disagree with it.

## What Changes

- Add `conformance/contract/`: one module per canonical capability
  (`skill`, `mcp`, `hook`, `agent`) stating what **every** harness must
  prove, in outcome terms — invoke this Skill by model discovery and by
  explicit user invocation, and get the same result; connect this MCP
  server and call this tool, and get the same result.
- Add `conformance/harnesses/<vendor>/bindings.py`: how *this* harness is
  driven — how a user invokes a Skill, where a catalog is read, how a tool
  result is recognised. The contract runs identically; only bindings differ.
- A harness that genuinely cannot express part of the contract declares it
  `Unsupported` with a reason. That becomes recorded evidence rather than a
  check nobody wrote.
- Reduce each `scenarios.py` to what is genuinely unique to that vendor.
- Add a `uze` vertical: the Lab tests four harness TUIs and never tests
  UZE's own. Cover what only shows up integrated — launching an agent,
  isolating a checkout, surviving a reattach, delivering project context.

## Capabilities

### New Capabilities

None. This changes how the Lab asserts, not what the product does.

### Modified Capabilities

None.

## Impact

- `conformance/contract/` (new), `conformance/harnesses/*/bindings.py`
  (new), `conformance/harnesses/*/scenarios.py` (reduced),
  `conformance/lab.py` (runs the contract before the vendor-specific
  phases), `conformance/harnesses/uze/` (new vertical).
- `docs/architecture/invariants.md`, `AGENTS.md`.

## Non-goals

- **Adopting pytest.** What is missing is a contract, not a runner: the
  same 42 divergent checks under `@pytest.mark.parametrize` are still four
  scripts. The hand-rolled `check`/`check_absence` also already encodes the
  settled-turn contract (ADR-035) that a generic runner does not.
  Parametrisation earns its keep once a `(capability × harness)` matrix
  exists — after this change, not as part of it.
- Fixing the Claude and Antigravity hook phases. Each needs its own change,
  and the knowledge they produce — how each harness actually accepts a
  scripted tool call — is what the `hook` contract will be written against.
- Replacing the deterministic suite. The Lab proves what only a real binary
  can; `tests/` keeps proving everything that does not need one.
