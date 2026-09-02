## Why

Antigravity executes `hooks.json` hooks only when signed in with a Google account: under `GEMINI_API_KEY` they are loaded and never run (vendor bug google-antigravity/antigravity-cli#893, and Google states API-key mode is unsupported in #78). The conformance Lab runs Antigravity in API-key mode, so every hook check on that harness is a declared limitation and UZE's own hook delivery to Antigravity has never been asserted. Measured on 2026-09-02 on a real account: the same global hook fires under OAuth and stays silent under the API key; serving the `json-hooks-enabled` flag alone does not open the gate. The signed-in protocol is JSON over hosts the Lab already intercepts, and every request/response shape needed was captured (sanitized, protocol shape only).

## What Changes

- The Antigravity synthetic world gains a **signed-in (`consumer`) mode**: a synthetic OAuth token file (`~/.gemini/antigravity-cli/antigravity-oauth-token`, `{"auth_method":"consumer","token":{access_token,token_type,refresh_token,expiry}}`, far expiry, synthetic identity), and stubs for `www.googleapis.com/oauth2/v2/userinfo` (synthetic `conformance@uze.invalid` identity), the CloudCode `v1internal` endpoints the CLI consults (`loadCodeAssist`, `fetchUserInfo`, `setUserSettings`, `retrieveUserQuotaSummary`, `listExperiments` with `json-hooks-enabled: true`, `fetchAdminControls`, `fetchAvailableModels`, `recordCodeAssistMetrics`, `writeTrajectoryAcls`) with minimal observed shapes, and a token refresh stub if the CLI asks for one.
- The model path moves to `v1internal:streamGenerateContent`: the existing Gemini provider logic (static, toolcall, `wants_function_call`, variations) unchanged, the request unwrapped from `{project, requestId, model, userAgent, requestType, request:{…}}` and each SSE event wrapped as `data: {"response": <GenerateContentResponse>, "traceId": …, "metadata": {}}`.
- The Antigravity vertical runs signed-in by default and `hooks > vendor` becomes a passing precondition: the vendor's own deny hook fires in the session. API-key mode stays covered by one declared check citing #893, so the vendor bug remains visible without gating the vertical.
- Registry: the hook declarations are retired only by a passing run. **The run did not retire them.** With the signed-in gate open, a second one was measured and found shut: on 1.1.24 the harness reads no `hooks.json` from a plugin directory, which is where UZE delivers Antigravity's hooks (`agy plugin validate` counts them; the session reports `loaded 0 named hooks from 0 hooks.json file(s)`), contradicting the vendor's own shipped plugin guide. The vertical therefore grows a second live precondition, `hooks > delivery`, and the UZE hook checks stay declared against *that* measurement instead of a now-false #893 reason. `user-only-skill-adapted` is untouched.
- Docs: the Antigravity integration README and `docs/capabilities/portable-hooks.md` state the mode dependency for users (hooks need a signed-in session; API-key sessions run none — vendor bug #893).

Out of scope: real OAuth, token refresh semantics beyond a stub, quota/tier semantics, and any change to product code.

## Capabilities

### New Capabilities

<!-- none -->

### Modified Capabilities

- `local-real-harness-conformance`: the Antigravity vertical exercises the harness in the mode in which the vendor executes hooks, with a synthetic signed-in identity and CloudCode protocol stubs; API-key mode and the vendor's unread plugin-scoped `hooks.json` are each measured live and declared, never silently dropped.

## Impact

- `conformance/harnesses/antigravity/provider.py` (consumer mode, CloudCode paths, SSE wrapping, the model catalogue the signed-in CLI has no built-in for), `scenarios.py` (`agy_setup` writes the token and drops `GEMINI_API_KEY`; the two hook preconditions; the API-key declared check), `fixtures/` (token, API-key settings variant — synthetic only), `shared/common.py` (hosts/SANs, `render_screen`, an opt-in accumulating waiter), `shared/capture.py` (chunked request bodies), `experiments/antigravity/` (a signed-in probe; the two older ones pin API-key mode), `evidence/expected.json`, `DECISIONS.md`, integration README, capability docs. No Rust changes.
