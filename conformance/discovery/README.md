# Discovery tooling (not runtime test infrastructure)

This directory holds the **observation-only** tooling used to learn how the real
Antigravity CLI talks to the world. It is **not** part of the permanent
conformance test. The permanent test (`../runtime/`) depends only on the real
AGY binary, the API-key mode, and a minimal fake provider — none of this.

Raw discovery evidence (authenticated captures, verbatim provider/system
payloads) must stay **outside the repository**. Anything persisted here is
sanitized to protocol shape with synthetic identity only.

## `mitmproxy/`

- `sanitizing_addon.py` — records traffic **sanitized at capture time**
  (masks Authorization/cookie/api-key headers, token-shaped query params and
  JSON fields, long opaque strings). Used to observe the authenticated host
  session without persisting identity.
- `replay_addon.py` — deterministic replay addon with three modes:
  `passthrough` (forward), `replay` (serve fixtures, forward the rest),
  `offline` (serve fixtures, **FAIL LOUDLY + record** anything unmatched).
  Used to map which external dependencies are actually required.

## `host_obs/`

- `run_agy_obs.sh` — run a real authenticated AGY command on the host through
  mitmproxy with `SSL_CERT_FILE` scoped to the mitmproxy CA (no host CA
  change). Read-only observation.

## How to re-run discovery

```bash
python3 -m venv /tmp/mitm-venv && /tmp/mitm-venv/bin/pip install mitmproxy pexpect
/tmp/mitm-venv/bin/mitmdump --mode regular@8082 \
  -s conformance/agy-isolation/discovery/mitmproxy/sanitizing_addon.py
# then run the real agy with:
#   HTTPS_PROXY=http://127.0.0.1:8082 SSL_CERT_FILE=$HOME/.mitmproxy/mitmproxy-ca-cert.pem agy ...
```

Never commit raw `.mitm`/`.flows` captures or any file containing tokens,
cookies, Authorization headers, email addresses, or account identifiers.

## What discovery established (so the permanent test does not need it)

- The OAuth/PA/Unleash/userinfo/quota/experiments/telemetry/update/Playwright
  endpoints are **not required** for the UZE-relevant surface: the
  vendor-supported **API-key mode** (`modelProvider: "gemini"` in
  `~/.gemini/antigravity-cli/settings.json` + `GEMINI_API_KEY` +
  `GOOGLE_GEMINI_BASE_URL`) bypasses Google OAuth entirely, and the TUI reaches
  its prompt + skills on a network with **zero external egress** (only the
  local fake provider).
- The model request AGY builds (API-key mode) is
  `POST {GOOGLE_GEMINI_BASE_URL}/v1beta/models/{model}:streamGenerateContent?alt=sse`
  (SSE), and the provider request is the model-facing observation boundary.
