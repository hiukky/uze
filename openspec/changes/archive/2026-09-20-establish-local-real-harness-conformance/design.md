## Design

The conformance lab is test infrastructure, not a fourth UZE integration.
Its runner owns a narrow process contract — executable, arguments,
environment, HOME, UZE_HOME, working directory, stdin, timeout, output and
exit status — plus terminal driving and evidence classification. It invokes
the real harness executables; it does not reuse `IntegrationPort` for
process orchestration, and it does not infer product compatibility from
model output.

Each run starts in a disposable container with a fresh HOME, UZE_HOME and
project. UZE installs the portable multi-capability fixture once, plans and
attaches it through its real peer integrations, then the harness runs
against the vertical's synthetic provider. The runner emits independent
attachment, discovery and behavioral evidence. A missing executable, a
declared vendor limitation, a timeout or a model's own choice is reported at
its own layer, never as a false product incompatibility.

**The provider is synthetic, per vendor.** It speaks the vendor's real wire
protocol — the harness is not told it is in a lab — and returns scripted,
deterministic responses:

```text
real uze -> real harness -> real TUI -> synthetic provider -> deterministic result
```

The alternative considered and implemented first was a pinned LiteLLM
gateway holding a free-tier provider credential, with each harness container
reaching only the gateway. It was abandoned: routing to a real model made
runs non-deterministic and cost-bearing, and confined credentials rather
than removing them. The synthetic provider gives the stronger property —
there is no credential to confine — and makes a run reproducible from an
empty machine. Real-model behavior moved to the opt-in L4 tier.

The image is built with explicit harness version arguments and no host HOME
mount, on an internal-only Docker network with no external egress.

**Structure is vertical by harness.** One directory per vendor holds its
provider, its driving bindings and its scenarios, because a maintainer
debugging one harness should find everything in one place. Shared code stays
small and vendor-neutral.
