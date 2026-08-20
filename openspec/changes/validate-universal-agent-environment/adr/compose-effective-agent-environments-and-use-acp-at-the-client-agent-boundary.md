# Compose effective agent environments and use ACP at the Client-Agent boundary

Status: Accepted

## Context

ADR-001 correctly prevents UZE from recreating AGENTS.md, Agent Skills, MCP,
and other standards in a proprietary format. ADR-002 then concentrated the
remaining work in a single internal graph of residual vendor primitives and a
filesystem projector. That turns configuration translation into UZE's central
abstraction even though the durable user need is to make the *agentic project*
portable.

ACP now provides an open Client ↔ Agent runtime interface, including
initialization-time version/capability negotiation, sessions, prompts,
streaming updates, permission requests, tool activity, and diffs. The official
Rust SDK also provides explicit `Proxy` components and a `Conductor` for
running a chain of proxies between a client and an agent. ACP is relevant to
runtime interoperability, but does not standardize project instructions,
skills, MCP resources, project composition, or every target harness.

## Decision

We will treat UZE as a standards-first resolver of an **effective agent
environment**: a portable project core (`AGENTS.md`, Agent Skills, MCP) plus
separately identified optional harness enhancements. UZE will adopt before it
invents: use a direct open standard first, then a native capability that leaves
the portable core intact, then an explicit adapter, and otherwise report the
capability as unsupported.

At the Client ↔ Agent boundary only, UZE will prefer native ACP, then an
official or demonstrably reliable ACP adapter, then a minimal explicit
integration adapter. ACP-negotiated capabilities remain protocol facts; UZE
will not duplicate their discovery or classify them as project capabilities.
ACP is not a universal UZE protocol and no target is required to support it.

An ACP Proxy or the official Rust SDK Conductor may be used only for a named,
explicit runtime-integration concern. They must not silently transform portable
project standards, create semantic equivalence for proprietary features, or
become a mandatory implementation dependency. A2A is recognized as a future
Agent ↔ Agent option and is outside this MVP.

This ADR supersedes ADR-002's single capability-graph boundary. ADR-001
remains accepted: this decision extends its standards-first consequence to the
separate ACP runtime boundary rather than replacing it.

## Consequences

Easier: UZE has a precise responsibility that standards do not already cover:
discovering and explaining how a project-owned portable core and optional
runtime enhancements compose into an effective environment. ACP-capable
clients and agents can communicate through a shared protocol without UZE
writing one-off protocol bridges, and Proxy/Conductor chains offer a bounded
option when a runtime concern must be extended.

Harder: project composition and harness-specific semantics remain unsolved by
current standards. UZE must preserve explicit reporting for unsupported or
unverified enhancements, verify ACP support per selected runtime, and resist
turning adapter convenience into hidden configuration synchronization. A
future implementation in Rust may use the official SDK, but selecting a
language or proxy chain remains a separate implementation decision.
