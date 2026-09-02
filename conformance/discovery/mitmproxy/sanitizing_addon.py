"""Sanitizing mitmproxy addon for the AGY record phase.

Persists ONLY protocol shape — never personal identity.
Masks: Authorization/cookie/api-key headers, token-like query params,
and token/credential-like JSON fields. Identity also hides *inside* string
values — a response carried an account address in a URL
(`upgradeSubscriptionUri=...?Email=<addr>&...`), which no key-based rule
sees — so every string value is additionally scrubbed of e-mail addresses
and of embedded `email=`/`user=` query values. Raw flows are never written
to disk.
"""

import json
import os
import re
import time

SENSITIVE_HEADERS = {
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-goog-api-key",
    "x-api-key",
    "api-key",
    "x-goog-iam-authorization",
    "x-cloud-trace-context",
    "x-request-id",
    "x-token",
    "x-auth-token",
    "google-oauth-state",
    "x-cc-token",
}
SENSITIVE_PARAM_KEYS = {
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apikey",
    "key",
    "code",
    "client_secret",
    "credential",
    "auth",
    "code_challenge",
    "code_verifier",
    "state",
    "jwt",
    "sig",
    "email",
    "user",
    "username",
    "login",
}
SENSITIVE_JSON_KEYS = {
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apikey",
    "credential",
    "credentials",
    "client_secret",
    "secret",
    "token",
    "authorization",
    "authorization_code",
    "code",
    "idtoken",
    "id_token",
    "oauth",
    "oauth_token",
    "session",
    "session_id",
    "email",
    "user_id",
    "userId",
    "sub",
    "account_id",
    "gaia_id",
    "installation_id",
    "machine_id",
    "device_id",
    "client_id",
    "clientId",
    "state",
    "jwt",
}

TOKEN_RE = re.compile(r"(?i)([A-Za-z0-9._~+/=-]{16,})")
# An address is identity at any length, so it can never ride out on the
# token heuristic (which only fires at 24 characters).
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
# The same identity url-encoded inside a link a JSON value happens to carry
# (`...?Email=user%40example.com&...`): masked by parameter name, before
# any pattern gets a chance to miss it.
EMBEDDED_PARAM_RE = re.compile(r"(?i)\b(email|user|username|login)=([^&\s\"']*)")


def _mask(value, key=None):
    if value is None:
        return None
    if key is not None and key.lower() in SENSITIVE_JSON_KEYS:
        return "<UZE_MASKED:%s>" % key
    s = str(value)
    # Identity first: a `?Email=` inside a link, then any bare address.
    # Both are masked whatever their length — the token heuristic below
    # only fires at 24 characters and would let a short address through.
    s = EMBEDDED_PARAM_RE.sub(lambda m: f"{m.group(1)}=<UZE_MASKED>", s)
    s = EMAIL_RE.sub("<UZE_MASKED_EMAIL>", s)
    # Mask anything that looks like a bearer/JWT/opaque token
    if re.search(r"(?i)bearer\s+", s):
        s = re.sub(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+", r"\1<UZE_TEST_TOKEN>", s)
    # Mask long opaque strings (tokens, ids)
    s = TOKEN_RE.sub(
        lambda m: "<tok%d>" % len(m.group(1)) if len(m.group(1)) >= 24 else m.group(0),
        s,
    )
    return s


def _sanitize_headers(headers):
    out = {}
    for k, v in headers.items():
        lk = k.lower()
        if lk in SENSITIVE_HEADERS:
            out[k] = "<UZE_MASKED_HEADER>"
        elif any(
            part in lk
            for part in ("token", "key", "secret", "cred", "auth", "cookie", "session")
        ):
            out[k] = "<UZE_MASKED_HEADER>"
        else:
            out[k] = _mask(v)
    return out


def _sanitize_query(query_text):
    if not query_text:
        return query_text
    parts = []
    for pair in query_text.split("&"):
        if "=" in pair:
            k, _, v = pair.partition("=")
            if k.lower() in SENSITIVE_PARAM_KEYS:
                parts.append(f"{k}=<UZE_MASKED>")
            else:
                parts.append(f"{k}={_mask(v)}")
        else:
            parts.append(pair)
    return "&".join(parts)


def _sanitize_json_body(body_text):
    try:
        data = json.loads(body_text)
    except Exception:
        return None

    def walk(o):
        if isinstance(o, dict):
            return {
                k: ("<UZE_MASKED>" if k.lower() in SENSITIVE_JSON_KEYS else walk(v))
                for k, v in o.items()
            }
        if isinstance(o, list):
            return [walk(v) for v in o]
        if isinstance(o, str):
            return _mask(o)
        return o

    return walk(data)


class SanitizingLog:
    def __init__(self):
        self.path = os.environ.get("AGY_LOG_PATH", "/tmp/agy-sanitized.jsonl")
        self.buf = []
        self.fh = open(self.path, "a")

    def _emit(self, rec):
        self.fh.write(json.dumps(rec) + "\n")
        self.fh.flush()

    def response(self, flow):
        req = flow.request
        resp = flow.response
        host = req.host
        scheme = req.scheme
        path = req.path
        if "?" in path:
            path, _, qs = path.partition("?")
        else:
            qs = ""
        body = None
        if req.raw_content:
            ct = req.headers.get("content-type", "")
            if "json" in ct:
                body = _sanitize_json_body(req.get_text())
                if body is None:
                    body = _mask(req.raw_content[:2000])
            elif len(req.raw_content) < 20000:
                body = _mask(req.raw_content[:2000])
            else:
                body = f"<body {len(req.raw_content)} bytes>"
        resp_body = None
        if resp.raw_content:
            ct = resp.headers.get("content-type", "")
            if "json" in ct:
                resp_body = _sanitize_json_body(resp.get_text())
                if resp_body is None:
                    resp_body = _mask(resp.raw_content[:2000])
            elif len(resp.raw_content) < 20000:
                resp_body = _mask(resp.raw_content[:2000])
            else:
                resp_body = f"<body {len(resp.raw_content)} bytes>"
        rec = {
            "ts": time.time(),
            "seq": len(self.buf) + 1,
            "method": req.method,
            "scheme": scheme,
            "host": host,
            "path": path,
            "query_sanitized": _sanitize_query(qs) if qs else None,
            "req_headers": _sanitize_headers(dict(req.headers)),
            "req_body": body,
            "status": resp.status_code,
            "resp_headers": _sanitize_headers(dict(resp.headers)),
            "resp_body": resp_body,
            "resp_size": len(resp.raw_content or b""),
        }
        self._emit(rec)
        self.buf.append(rec)

    def done(self):
        self.fh.close()


addons = [SanitizingLog()]
