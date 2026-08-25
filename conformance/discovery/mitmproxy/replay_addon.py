"""Deterministic replay addon for the AGY record->replay experiment.

Modes (AGY_REPLAY_MODE):
  passthrough : forward everything upstream (plain proxy)
  replay      : serve sanitized fixtures for observed hosts; forward the rest
  offline     : serve fixtures for observed hosts; FAIL+record everything else

Fixtures are sanitized captures from the real working AGY (see
fixtures/observed_endpoints.json) — protocol shape, synthetic identity only.
"""
import json
import os
import time

from mitmproxy import http


def _load_fixtures():
    path = os.environ.get("AGY_FIXTURES")
    if not path:
        return {}
    try:
        with open(path) as f:
            return json.load(f)
    except Exception as exc:
        print(f"[replay] fixtures load failed: {exc}", flush=True)
        return {}


class Replay:
    def __init__(self):
        self.fixtures = _load_fixtures()
        self.unmatched = []
        self.log_path = os.environ.get("AGY_REPLAY_LOG", "/tmp/agy-replay-unmatched.jsonl")

    def _record_unmatched(self, flow, reason):
        req = flow.request
        rec = {
            "ts": time.time(),
            "host": req.host,
            "method": req.method,
            "path": req.path,
            "scheme": req.scheme,
            "req_headers": {k: ("<masked>" if "authorization" in k.lower() or "cookie" in k.lower() else v)
                            for k, v in req.headers.items()},
            "req_body": req.get_text()[:4000] if req.raw_content else None,
            "reason": reason,
        }
        self.unmatched.append(rec)
        try:
            with open(self.log_path, "a") as f:
                f.write(json.dumps(rec) + "\n")
        except Exception:
            pass
        print(f"[replay] UNMATCHED {req.method} {req.host}{req.path} ({reason})", flush=True)

    def request(self, flow):
        mode = os.environ.get("AGY_REPLAY_MODE", "passthrough")
        req = flow.request
        key = f"{req.host}|{req.path}"
        fixture = self.fixtures.get(key)
        if fixture is not None:
            body = fixture.get("resp_body")
            if not isinstance(body, str):
                body = json.dumps(body)
            headers = dict(fixture.get("resp_headers", {}))
            headers.setdefault("Content-Type", "application/json; charset=UTF-8")
            flow.response = http.Response.make(fixture.get("status", 200), body, headers)
            flow.metadata["replayed"] = key
            return
        if mode == "offline":
            self._record_unmatched(flow, "offline-no-fixture")
            flow.response = http.Response.make(
                599,
                json.dumps({"error": "UZE_REPLAY_UNMATCHED", "key": key}),
                {"Content-Type": "application/json"},
            )
            flow.metadata["unmatched"] = key
            return
        if mode == "block" and key not in self.fixtures:
            self._record_unmatched(flow, "blocked")
            flow.response = http.Response.make(
                503, json.dumps({"error": "UZE_REPLAY_BLOCKED", "key": key}),
                {"Content-Type": "application/json"},
            )
            flow.metadata["blocked"] = key
            return
        # passthrough / replay-with-upstream: let it go upstream


addons = [Replay()]
