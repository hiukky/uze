# Routed provider contract

The default behavioral lab uses a real harness CLI in an isolated container,
an internal LiteLLM gateway, and a separately authenticated provider. This is
not a local-model result and must be reported as **routed L2 evidence**.

The initial route is deliberately narrow:

```text
Claude Code ─┐
Codex       ─┼── internal LiteLLM ── Groq
OpenCode    ─┘
```

The first gateway image is
`docker.litellm.ai/berriai/litellm@sha256:468c25f35f3e5ec4e414974f00deab93337b1b4d9953cabcfd3722e59415f834`
(observed LiteLLM `1.97.0`). The model alias exposed to harnesses is
`uze-conformance`; the actual Groq model is a runtime environment value.

## Credential and network rules

- `GROQ_API_KEY` is injected into the `gateway` service only, from a shell
  environment or CI secret.
- Harnesses receive no provider key and have no direct egress network.
- The gateway has no published host port, database, callbacks or telemetry
  configuration.
- Do not write keys, prompts, tool payloads, or provider responses to the
  ledger, repository, or ordinary test logs.

## Evidence contract

Every routed L2 result records:

```text
gateway image digest and LiteLLM version
provider and concrete model identifier
harness version and protocol route
PackageId, resource identities, store paths, exposure strategy
timestamp, elapsed time, and non-sensitive failure classification
```

Free tiers are useful for a scheduled or manually triggered CI lane, but their
quota, availability, and model behavior are external state. A rate limit,
credential failure, provider outage, or model failure is
`BLOCKED_BY_ENVIRONMENT`/`MODEL_FAILURE`, never product incompatibility.

## Provider expansion

OpenRouter and Gemini are future gateway routes, not implicit fallbacks. Each
needs its own pinned model alias, secret name, protocol smoke test, and
documented failure classification before being enabled. LiteLLM remains test
infrastructure only; UZE product crates never depend on it.
