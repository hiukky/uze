## 1. Provider consumer mode

- [x] 1.1 Add `consumer` handling to `conformance/harnesses/antigravity/provider.py`: serve `www.googleapis.com/oauth2/v2/userinfo`, `oauth2.googleapis.com/token` (synthetic token), and the CloudCode `v1internal` endpoints with the minimal observed shapes; `listExperiments` carries `json-hooks-enabled: true`; unknown paths answer `{}` and are logged
- [x] 1.2 Route `v1internal:streamGenerateContent` to the existing Gemini logic: unwrap `request`, wrap each SSE event as `{"response": …, "traceId": …, "metadata": {}}`; static/toolcall/variations unchanged
- [x] 1.3 Hosts and SANs: add `www.googleapis.com`, `oauth2.googleapis.com` (and `lh3.googleusercontent.com`, the avatar the identity names) beside the CloudCode/Unleash hosts; provider docstring records the shapes as observed 2026-09-02
- [x] 1.4 Unit tests for the envelope wrapping/unwrapping and the token/userinfo payloads (synthetic-only assertions) — `conformance/tests/test_signed_in.py`, plus the chunked-body reader and the multi-entry `tools` regression

## 2. Vertical in signed-in mode

- [x] 2.1 `agy_setup` writes the synthetic `antigravity-oauth-token` and stops exporting `GEMINI_API_KEY`/`GOOGLE_GEMINI_BASE_URL`; fixtures updated (settings without `modelProvider: gemini`)
- [x] 2.2 Discovery run: confirmed the endpoint list and fixed the stubs it named — experiment `antigravity/signed-in`. Beyond the captured shapes the run needed three things: a `fetchAvailableModels` catalogue (shape from the binary's own descriptor; without a `model` enum every turn dies with "neither PlanModel nor RequestedModel specified"), chunked request-body decoding, and RPC routing that ignores the query (`…:streamGenerateContent?alt=sse`). No token refresh was attempted; the stub stays served
- [x] 2.3 `hooks > vendor` passes (the vendor-format deny hook denies `run_command`; the TUI renders `Tool call denied by pre-tool hook: blocked by protect-env`); `hooks > api-key` re-runs the same control hook on `GEMINI_API_KEY` as one declared check citing google-antigravity/antigravity-cli#893
- [x] 2.4 Full Antigravity vertical run; `common.py`/`capture.py` are shared, so the `uze` and `claude` verticals were re-run
- [x] 2.5 Added `hooks > delivery` as a second live precondition (a headless start reading the harness's own count). It found the plugin route dead on 1.1.24: `agy plugin validate` counts the plugin's three hooks while the session reports `loaded 0 named hooks from 0 hooks.json file(s)` and never opens the file, contradicting the vendor's shipped plugin guide
- [x] 2.6 **Route change (product):** Antigravity's hooks are delivered as receipt-owned named entries merged into the shared `~/.gemini/config/hooks.json`, keyed `<package>:<group-id>`, wrapper at `$UZE_HOME/state/attachments/antigravity/hooks/exec`; the generated plugin writes no `hooks.json` and claims no hook coverage. A denial exits 0 on this harness — measured: a non-zero exit is logged as `pre-tool hook failed` and the permission prompt takes over. With that, `hooks > delivery` passes (`loaded 3 named hooks from 1 hooks.json file(s)`) and every UZE hook check passes for real, so the 9 declarations are retired

## 3. Docs

- [x] 3.1 `conformance/DECISIONS.md`: the mode decision, what the provider serves that no capture could give it, and the second precondition; `crates/uze-integrations/src/antigravity/README.md` and `docs/capabilities/portable-hooks.md`: the two vendor gates a delivered hook must pass — a signed-in session (#893 for API keys), and a harness that reads a plugin's `hooks.json` (shut on 1.1.24)
- [x] 3.2 Gates: `ruff format --check`, `ruff check`, conformance unit tests, `openspec validate --all --strict`
