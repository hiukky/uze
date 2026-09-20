# Conformance is a disposable lab outside the product, and evidence is layered

Status: Accepted

## Context

UZE's central claim is that a capability declared once reaches every
harness. Before this change, the only evidence for it came from two places
that could not carry it. The deterministic Rust suites drive fake harness
binaries, so they prove what UZE *wrote*, never what a vendor *did* with
it. The opt-in vendor probes did run a real CLI, but from the developer's
own machine: a populated `HOME`, a real account, a real model, and a real
bill. A probe like that cannot be rerun from a clean state, cannot run in
CI, and cannot distinguish "UZE attached it wrong" from "the model chose
not to use it".

Three distinct claims were also being scored as one. That the package
landed, that the harness discovered it, and that a model exercised it are
independent facts with independent failure modes, and collapsing them means
a model having a bad day reads as a product incompatibility.

## Decision

Real-harness conformance is a **disposable lab that lives outside the
product**, and it is a distinct evidence tier from the deterministic suites.

- **Tiers are explicit and the boundary is credentials and network.**
  L0/L1 (unit, component contract) and L3 (acceptance, the real `uze`
  binary against controlled harness CLIs) run under plain `cargo test`:
  no Docker daemon, no container, no provider account. L2 is the Lab —
  the real vendor binary — and L4 is model-invocation behavior, which is
  manual and never gates CI. `tests/README.md` owns the table.
- **The Lab's world is synthetic and sealed.** Each run is a container on
  an `--internal` Docker network with an empty `HOME`, `UZE_HOME` and
  project directory, no host `HOME` mount and no Docker socket. The
  harness is real; the provider it talks to is a per-vendor synthetic
  server speaking that vendor's real wire protocol. **Zero Internet, zero
  tokens, zero credentials** — so a run is reproducible from nothing, and
  a deterministic response is the only response a harness can get.
- **Evidence is layered, and model quality is not a compatibility claim.**
  Attachment (package identity, stored paths, exposure strategy),
  discovery (the harness lists it), and behavior (the turn produces the
  result) are recorded independently, alongside run provenance. A missing
  executable, a timeout, a vendor limitation and a model's choice are each
  reported at their own layer, and none of them silently becomes a product
  incompatibility.
- **The lab is not a fourth integration.** No product-domain type names
  Docker, a provider, or the runner, and the Lab reuses no
  `IntegrationPort` for process orchestration. It is never linked into the
  deterministic suite, and deleting it costs no product behavior.
- **Harness selection follows evidence, not the original tracer
  bullets.** A harness enters the Lab when it has an honest headless path
  and a protocol a synthetic provider can serve.

## Consequences

Conformance becomes a thing CI can run: free, offline, reproducible from
an empty machine, and specific about which layer failed. The deterministic
suites stay fast and dependency-free, because nothing about the Lab is on
their path.

The cost is a second toolchain and a second set of operational concerns —
image builds, per-vendor synthetic providers, PTY driving — maintained
beside the Rust workspace rather than inside it, and one synthetic provider
per vendor to keep current as wire protocols move. That is the price of
running the real binary; mocking the harness would remove the cost and the
evidence together.

## More Information

The routed-provider design this change started from — a pinned LiteLLM
gateway holding a free-tier provider credential, with the harness
containers reaching only the gateway — was implemented, then abandoned
during the same change. Routing to a real model made every run
non-deterministic, cost-bearing and credential-bearing, and produced
behavioral evidence that varied by provider mood. The synthetic provider
replaced it and made the stronger guarantee possible: not "credentials are
confined to the gateway" but *there are no credentials*. Real-model
behavior did not disappear — it became L4, run deliberately and never as a
gate.

The runner was likewise built in Rust under `e2e/` and rewritten in Python
under `conformance/`, once it was clear the work was process orchestration
and terminal driving rather than anything the workspace's types belonged
in.

Later changes refined this record rather than reversing it:
[043](043-conformance-asserts-one-outcome-contract-per-capability.md) made
the Lab assert one outcome contract per capability instead of per-vendor
notions of correct, and
[035](035-adaptive-result-registry-and-version-provenance.md) closed the
gate's false-positive sources.

Source change: openspec/changes/archive/2026-09-20-establish-local-real-harness-conformance/
