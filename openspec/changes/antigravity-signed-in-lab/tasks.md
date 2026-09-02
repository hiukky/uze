## 1. Provider consumer mode

- [ ] 1.1 Add `consumer` handling to `conformance/harnesses/antigravity/provider.py`: serve `www.googleapis.com/oauth2/v2/userinfo`, `oauth2.googleapis.com/token` (synthetic token), and the CloudCode `v1internal` endpoints with the minimal observed shapes; `listExperiments` carries `json-hooks-enabled: true`; unknown paths answer `{}` and are logged
- [ ] 1.2 Route `v1internal:streamGenerateContent` to the existing Gemini logic: unwrap `request`, wrap each SSE event as `{"response": …, "traceId": …, "metadata": {}}`; static/toolcall/variations unchanged
- [ ] 1.3 Hosts and SANs: add `www.googleapis.com`, `oauth2.googleapis.com` beside the CloudCode/Unleash hosts; provider docstring records the shapes as observed 2026-09-02
- [ ] 1.4 Unit tests for the envelope wrapping/unwrapping and the token/userinfo payloads (synthetic-only assertions)

## 2. Vertical in signed-in mode

- [ ] 2.1 `agy_setup` writes the synthetic `antigravity-oauth-token` and stops exporting `GEMINI_API_KEY`/`GOOGLE_GEMINI_BASE_URL`; fixtures updated (settings without `modelProvider: gemini`)
- [ ] 2.2 Discovery run: confirm the endpoint list and that no refresh or extra call blocks the session; fix stubs accordingly
- [ ] 2.3 `hooks > vendor` passes; UZE hook checks asserted; add one declared check for API-key mode citing google-antigravity/antigravity-cli#893
- [ ] 2.4 Full Antigravity vertical green with the 10 hook declarations retired from `conformance/evidence/expected.json`; other verticals unaffected (shared code touched → rerun)

## 3. Docs

- [ ] 3.1 `conformance/DECISIONS.md`: record the mode decision and the measurement; `crates/uze-integrations/src/antigravity/README.md` and `docs/capabilities/portable-hooks.md`: hooks need a signed-in session (vendor bug #893 for API keys)
- [ ] 3.2 Gates: `ruff format --check`, `ruff check`, conformance unit tests, `openspec validate --all --strict`
