## Why

Antigravity executes `hooks.json` hooks only when signed in with a Google account: under `GEMINI_API_KEY` they are loaded and never run (vendor bug google-antigravity/antigravity-cli#893, and Google states API-key mode is unsupported in #78). The conformance Lab runs Antigravity in API-key mode, so every hook check on that harness is a declared limitation and UZE's own hook delivery to Antigravity has never been asserted. Measured on 2026-09-02 on a real account: the same global hook fires under OAuth and stays silent under the API key; serving the `json-hooks-enabled` flag alone does not open the gate. The signed-in protocol is JSON over hosts the Lab already intercepts, and every request/response shape needed was captured (sanitized, protocol shape only).

## What Changes

- The Antigravity synthetic world gains a **signed-in (`consumer`) mode**: a synthetic OAuth token file (`~/.gemini/antigravity-cli/antigravity-oauth-token`, `{"auth_method":"consumer","token":{access_token,token_type,refresh_token,expiry}}`, far expiry, synthetic identity), and stubs for `www.googleapis.com/oauth2/v2/userinfo` (synthetic `conformance@uze.invalid` identity), the CloudCode `v1internal` endpoints the CLI consults (`loadCodeAssist`, `fetchUserInfo`, `setUserSettings`, `retrieveUserQuotaSummary`, `listExperiments` with `json-hooks-enabled: true`, `fetchAdminControls`, `fetchAvailableModels`, `recordCodeAssistMetrics`, `writeTrajectoryAcls`) with minimal observed shapes, and a token refresh stub if the CLI asks for one.
- The model path moves to `v1internal:streamGenerateContent`: the existing Gemini provider logic (static, toolcall, `wants_function_call`, variations) unchanged, the request unwrapped from `{project, requestId, model, userAgent, requestType, request:{…}}` and each SSE event wrapped as `data: {"response": <GenerateContentResponse>, "traceId": …, "metadata": {}}`.
- The Antigravity vertical runs signed-in by default; `hooks > vendor` becomes a passing precondition and the UZE hook checks are asserted like on Claude. API-key mode stays covered by one declared check citing #893, so the vendor bug remains visible without gating the vertical.
- Registry: the 10 Antigravity hook declarations are retired only by a passing run; `user-only-skill-adapted` is untouched.
- Docs: the Antigravity integration README and `docs/capabilities/portable-hooks.md` state the mode dependency for users (hooks need a signed-in session; API-key sessions run none — vendor bug #893).

Out of scope: real OAuth, token refresh semantics beyond a stub, quota/tier semantics, and any change to product code.

## Capabilities

### New Capabilities

<!-- none -->

### Modified Capabilities

- `local-real-harness-conformance`: the Antigravity vertical exercises the harness in the mode in which the vendor executes hooks, with a synthetic signed-in identity and CloudCode protocol stubs; API-key mode is declared, not silently dropped.

## Impact

- `conformance/harnesses/antigravity/provider.py` (consumer mode, CloudCode paths, SSE wrapping), `scenarios.py`/`bindings.py` (`agy_setup` writes the token, drops `GEMINI_API_KEY`; hook phases asserted), `fixtures/` (token, identity, endpoint shapes — synthetic only), `shared/common.py` (hosts/SANs), `evidence/expected.json`, `DECISIONS.md`, integration README, capability docs. No Rust changes.
