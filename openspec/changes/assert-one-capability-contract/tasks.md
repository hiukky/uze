## 1. The seam

- [ ] 1.1 Add `conformance/contract/` with a runner that takes a harness's bindings and executes every capability contract against it.
- [ ] 1.2 Add a `Tui` driver wrapping the pexpect child: launch, wait for ready, type, accumulate, snapshot — the mechanics every vertical currently repeats.
- [ ] 1.3 Add `Unsupported` as a declared, recorded outcome rather than an omitted check.

## 2. The Skill contract

- [ ] 2.1 State it in outcome terms: model discovery and explicit invocation both produce the same result; model-only is discovered but not user-invocable; user-only is the inverse.
- [ ] 2.2 Make every absence assertion conditional on a presence assertion that proves the surface was populated.
- [ ] 2.3 Write bindings for the four harnesses and run the contract against each.

## 3. The remaining capabilities

- [ ] 3.1 MCP: server connected, tool invocable, same result everywhere.
- [ ] 3.2 Hook: deny blocks and relays a reason, allow executes, first deny wins. Written against what the Claude and Antigravity investigations establish.
- [ ] 3.3 Agent: visible, selectable, answers.

## 4. Reduce the verticals

- [ ] 4.1 Delete from each `scenarios.py` what the contract now covers.
- [ ] 4.2 Keep only what is genuinely unique to that vendor, and say in each why it cannot be a contract.

## 5. A vertical for UZE

- [ ] 5.1 Cover what only shows up integrated: launching an agent, isolating a checkout, surviving a reattach, delivering context.
- [ ] 5.2 Move what does not need a container into `View` snapshots in the deterministic suite.

## 6. Record it

- [ ] 6.1 Invariants for the properties the contract now holds.
- [ ] 6.2 State the contract/bindings split in `AGENTS.md`, including that a contract names no vendor and a binding carries no assertion.
