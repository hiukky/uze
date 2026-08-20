# Establish isolated local real-harness conformance

Status: Accepted

## Context

Existing UZE conformance probes use real harnesses but depend on opt-in vendor
credentials and host-installed state. Product lifecycle tests intentionally do
not execute models. We need repeatable behavioral wiring evidence without
turning the product into a runtime proxy or embedding test backends in its
Core.

## Decision

UZE conformance is classified into L0 unit, L1 product contract/integration,
L2 local real-harness E2E, and L3 opt-in vendor conformance. L2 lives under
`tooling/conformance`, uses disposable Docker environments, and invokes the
actual UZE and selected harness CLIs. Its runner owns only process
configuration and evidence classification. Harness selection follows the
ecosystem/provider matrix, rather than the original tracer bullets.

The L2 inference reference is a pinned llama.cpp service. OpenCode has the
first ready direct Chat Completions candidate; Codex needs a Responses protocol
spike; Claude Code is an explicitly experimental local gateway route because
Anthropic does not support non-Claude upstreams. A gateway is not part of the
default path; LiteLLM may be enabled later only as isolated test infrastructure
when direct protocol compatibility is demonstrably insufficient. Gemini CLI is
not selected for zero-vendor L2 without an officially supported local route.

No product-domain type references Docker, local inference, or the test runner.
Model quality is not a compatibility claim: attachment, discovery, and local
behavioral proof remain independent evidence.

## Consequences

The lab can reproduce a test from an empty machine state and preserve version,
model, package, and exposure evidence. It adds Docker/image/model operational
maintenance, and L2 remains opt-in because it requires a supplied GGUF and is
not suitable for ordinary `cargo test`.
