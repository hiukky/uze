## Context

See proposal.md — Why. Shapes captured on 2026-09-02 (agy 1.1.22 binary, backend UA 1.1.24) through the sanitizing mitmproxy; only structure is kept:

- Token file `antigravity-oauth-token`: `{"auth_method":"consumer","token":{"access_token":str,"token_type":str,"refresh_token":str,"expiry":str}}`.
- `GET www.googleapis.com/oauth2/v2/userinfo` → `{id, email, verified_email, name, given_name, picture}`.
- `POST daily-cloudcode-pa.googleapis.com/v1internal:fetchUserInfo` `{project}` → `{"userSettings":{"telemetryEnabled":bool},"regionCode":str}`; `loadCodeAssist` → tier document (`{"currentTier":{"id":"free-tier",…},"allowedTiers":[…]}`); `listExperiments` `{}` → `{"experimentIds":[],"flags":[{"name":"json-hooks-enabled","boolValue":true}]}`; the rest answer `{}`.
- `POST …/v1internal:streamGenerateContent`: request `{project, requestId, model, userAgent, requestType, request:{contents, systemInstruction, generationConfig, sessionId}}`; response SSE `data: {"response": {"candidates":[…],"usageMetadata":{…},"modelVersion":…,"responseId":…},"traceId":…,"metadata":{}}`.
- Also contacted: `antigravity-unleash.goog` (already served), `play.googleapis.com/log`, `lh3.googleusercontent.com` (avatar — 404 is fine), playwright driver downloads (404 is fine).

## Goals / Non-Goals

**Goals:** the vertical exercises the mode in which the vendor executes hooks; nothing real enters the repo; every other Antigravity check keeps its verdict.

**Non-Goals:** real OAuth, refresh semantics, tiers/quotas, product code changes.

## Decisions

### D1 — Signed-in is the default mode of the vertical
It is the mode users actually run (API-key mode is declared unsupported by the vendor) and the only one where hooks execute. API-key mode keeps one declared check so the vendor bug stays on the report.

### D2 — One provider, two envelopes
The Gemini provider logic is reused untouched; a `consumer` mode adds the CloudCode path, unwraps `request` and wraps each SSE event in `{"response": …}`. `wants_function_call` and the variation/discovery machinery apply unchanged.

### D3 — Synthetic identity, minimal stubs
Token values are literals (`synthetic-access-token`, far `expiry` so no refresh); userinfo is `conformance@uze.invalid`; every endpoint not needed for behaviour answers `{}`. Nothing observed from a real account is kept beyond structure; the sanitizer's e-mail masking guards the capture path.

### D4 — Assert, then retire
The registry entries leave only when the run proves the checks passing; the gate's escalation rule enforces it.

## Risks / Trade-offs

- [The CLI may require a token refresh or validate the access token shape] → stub `oauth2.googleapis.com/token` returning the same synthetic token; discover with `--discovery` before asserting.
- [The signed-in path touches more endpoints on future versions] → the provider logs every unknown path (`[provider:flags]` style) and answers `{}`; version drift stays an explicit event.
- [Two modes to keep honest] → API-key mode is a single declared check, not a second vertical.

## Migration Plan

1. Provider consumer mode + fixtures, discovery run to confirm the endpoint list.
2. Switch `agy_setup` to signed-in; run the vertical; fix shapes until `hooks > vendor` passes.
3. Retire registry entries, update docs.
Rollback: `agy_setup` back to `GEMINI_API_KEY`; the provider keeps both modes.
