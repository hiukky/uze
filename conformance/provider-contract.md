# Routed provider contract (L4 only)

The L4 level uses a real harness CLI in an isolated container, an internal
LiteLLM gateway, and a separately authenticated provider. This is not a
local-model result and must be reported as **routed L4 evidence**. L2 never
touches this gateway, any provider, or any credential.

The initial route is deliberately narrow:

```text
Claude Code ─┐
Codex       ─┼── internal LiteLLM ── OpenAI
OpenCode    ─┘
```

The first gateway image is
`docker.litellm.ai/berriai/litellm@sha256:468c25f35f3e5ec4e414974f00deab93337b1b4d9953cabcfd3722e59415f834`
(observed LiteLLM `1.97.0`). The model alias exposed to harnesses is
`uze-conformance`; the actual provider model is a runtime environment value.
The current documented default is `gpt-4o-mini`; the runner records the
concrete model selected rather than inferring it from this default.

## Credential and network rules

- `OPENAI_API_KEY` is injected into the `gateway` service only, from a shell
  environment or CI secret.
- Harnesses receive no provider key and have no direct egress network.
- The gateway has no published host port, database, callbacks or telemetry
  configuration.
- Provider requests declare the stable `uze-conformance-lab/0.1` User-Agent.
  It identifies this non-browser API client to edge protections and must not be
  replaced with a browser fingerprint.
- Compose waits for LiteLLM's liveness endpoint before starting the harness;
  `depends_on` alone is not treated as provider readiness.
- Do not write keys, prompts, tool payloads, or provider responses to the
  ledger, repository, or ordinary test logs.

## Evidence contract

Every L4 result records:

```text
gateway image digest and LiteLLM version
provider and concrete model identifier
harness version and protocol route
scenario, proof claim, timestamp, elapsed time, and failure classification
```

A rate limit, credential failure, provider outage, or model failure is
`PROVIDER_FAILURE`/`MODEL_FAILURE`, never product incompatibility, and never
downgrades an L2 record.

## Provider expansion

OpenRouter and Gemini are future gateway routes, not implicit fallbacks. Each
needs its own pinned model alias, secret name, protocol smoke test, and
documented failure classification before being enabled. LiteLLM remains test
infrastructure only; UZE product crates never depend on it.
