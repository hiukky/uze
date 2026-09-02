#!/usr/bin/env python3
"""Minimal deterministic fake Gemini provider — the permanent runtime stub.

Contract (derived from observed behavior of the REAL AGY 1.1.20):
  * AGY (API-key mode) POSTs to `{GOOGLE_GEMINI_BASE_URL}/v1beta/models/
    {model}:streamGenerateContent?alt=sse` with a GenerateContent JSON body.
  * The response is an SSE stream of `data: {json}` lines. IMPORTANT: it must
    NOT contain a `data: [DONE]` terminal — AGY's stream parser fails on it
    (observed: "error unmarshalling data data: [DONE]").

It also serves the harness's **feature-flag plane** over TLS on 443, which
is what decides whether `hooks.json` hooks execute at all. Observed on a
real, logged-in session (AGY 1.1.22 binary, backend UA
`antigravity/cli/1.1.24`, 2026-09-02):

  * the language-server process polls Unleash at
    `GET https://antigravity-unleash.goog/api/client/features` (and
    `POST /api/client/register`), evaluating strategies locally with
    `unleash-client-go`; the flag that gates JSON hooks is
    `json-hooks-enabled` ("Whether to enable hooks based on json files"),
    a `flexibleRollout` at 100% constrained to `ide IN [jetski]` — which
    is the context this CLI reports;
  * `POST https://daily-cloudcode-pa.googleapis.com/v1internal:listExperiments`
    answers the same flag as `{"name":"json-hooks-enabled","boolValue":true}`.

Both are served here exactly as recorded — the strategy is replayed, not
flattened to a bare `default`, so the harness's own evaluation is what
decides. The remaining `v1internal:*` endpoints and `play.googleapis.com/log`
are answered with the smallest shape that keeps the harness moving; no tier,
quota or entitlement semantics are invented.

Modes (PROVIDER_MODE):
  static   : serve the synthetic SSE fixture to every request (default).
  toolcall : request #1 -> functionCall(call_mcp_tool, FC_ARGS) so the REAL
             harness executes a real MCP server; request #2+ -> FINAL text.
             This proves the deep MCP tool-call path with zero model.

This stub records ONLY a structural summary of each request — never the
verbatim body — into a JSON file (PROVIDER_STRUCT). That summary is the
conformance observation boundary: it lets tests assert what the REAL AGY sent
toward its provider (model-visible vs user-only Skills, MCP tool execution)
without persisting vendor internal payloads.

Env:
  PROVIDER_MODE    : static | toolcall
  PROVIDER_STRUCT  : path to write the structural summary (JSON list)
  RESP_SSE         : path to the synthetic SSE response to serve (static)
  FC_ARGS          : JSON args for the call_mcp_tool functionCall (toolcall)
  FINAL_TEXT       : deterministic text served after request #1 (toolcall)
  PORT (argv[1])   : listen port
"""

import json
import os
import ssl
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

import capture
import variation

LEAF_CERT = os.environ.get("LEAF_CERT", "/app/leaf.crt")
LEAF_KEY = os.environ.get("LEAF_KEY", "/app/leaf.key")

STRUCT_PATH = os.environ.get("PROVIDER_STRUCT", "/tmp/agy-provider-struct.json")
RESP_SSE = os.environ.get("PROVIDER_RESP", "")
MODE = os.environ.get("PROVIDER_MODE", "static")
# Arguments are scripted in the shape the tool declares to the model
# (`parametersJsonSchema`, PascalCase plus the `toolSummary`/`toolAction`
# pair every AGY tool requires): the harness validates a call against that
# schema before any hook runs or any tool executes, and rejects the rest as
# "invalid arguments" — a turn that settles with no hook and no tool.
FC_ARGS = json.loads(
    os.environ.get(
        "FC_ARGS",
        '{"ServerName":"uze-conformance","ToolName":"uze_conformance","Arguments":{},"toolSummary":"Conformance proof","toolAction":"Calling MCP tool"}',
    )
)
FINAL_TEXT = os.environ.get("FINAL_TEXT", "UZE_CONFORMANCE_PASS")
MCP_PROOF = os.environ.get("MCP_PROOF", "UZE_MCP_CONFORMANCE_PROOF_1")

# The hook scenarios script a functionCall to the harness's native shell
# tool (`run_command`); the MCP phases keep the default below.
FC_NAME = os.environ.get("TOOL_NAME", "call_mcp_tool")

SKILL_MARKERS = [
    "flow:commit",
    "flow:review",
    "flow:analyze",
    "commit",
    "review",
    "analyze",
    "init",
]
TOOL_NAMES = [
    "grep_search",
    "list_dir",
    "manage_task",
    "read_url_content",
    "replace_file_content",
    "run_command",
    "schedule",
    "search_web",
    "view_file",
    "write_to_file",
    "generate_image",
    "call_mcp_tool",
]
# Conformance evidence markers carried by portable-hook denial reasons
# (ADR-033): presence/absence in the structural summary proves what the real
# harness relayed after the hook executed.
HOOK_MARKERS = [
    "blocked by protect-env",
    "first-handler-denied",
    "second-handler-ran",
    "second-handler-reached",
    "Denied by UZE hook",
    # The portable vocabulary row itself: the guard echoes the alias it was
    # handed, so a relayed denial proves the handler read `shell` (not the
    # harness's own tool name) whichever harness delivered the hook.
    "tool=shell",
    # The real tool stdout marker: only present when the intercepted tool
    # actually executed after an allow — the deny/allow contrast relies
    # on it, not on the ambiguous presence of a tool result.
    "plain output",
]
COUNTER = {"n": 0}


def structural_summary(body_text):
    b = json.loads(body_text) if body_text else {}
    body = json.dumps(b)
    has_fc = "functionCall" in body
    has_fr = "functionResponse" in body
    return {
        "content_roles": [c.get("role") for c in b.get("contents", [])],
        "tools": [
            t.get("name")
            for t in (b.get("tools") or [{}])[0].get("functionDeclarations", [])
        ]
        if b.get("tools")
        else [],
        "tool_config": b.get("toolConfig"),
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "has_function_call": has_fc,
        "has_function_response": has_fr,
        "hook_markers": {m: (m in body) for m in HOOK_MARKERS},
        "mcp_proof_present": MCP_PROOF in body,
        "has_user_request_tag": "<USER_REQUEST>" in body,
    }


def sse(obj):
    return f"data: {json.dumps(obj)}\n\n".encode()


def wants_function_call(summary):
    """A model can only call a function the request declared, and has no
    reason to call it again once the response is in the conversation. The
    harness also makes side requests (a lighter model, no tools declared)
    around the user's turn; counting requests handed the call to one of
    those and the real turn never saw a tool."""
    return FC_NAME in summary["tools"] and not summary["has_function_response"]


class H(BaseHTTPRequestHandler):
    def _handle(self):

        body = capture.read_body(self).decode("utf-8", "replace")
        n = COUNTER["n"]
        COUNTER["n"] += 1
        rec = {
            "method": self.command,
            "path": self.path,
            "seq": n,
            "summary": structural_summary(body),
        }
        struct = []
        if os.path.exists(STRUCT_PATH):
            try:
                struct = json.load(open(STRUCT_PATH))
            except Exception:
                struct = []
        struct.append(rec)
        with open(STRUCT_PATH, "w") as f:
            json.dump(struct, f, indent=1)
        print(f"[provider:{MODE}] {self.command} {self.path} req#{n}", flush=True)

        if MODE == "toolcall" and wants_function_call(rec["summary"]):
            fc = {"functionCall": {"name": FC_NAME, "args": FC_ARGS}}
            payload = sse(
                {
                    "candidates": [
                        {
                            "content": {"parts": [fc], "role": "model"},
                            "finishReason": "STOP",
                            "index": 0,
                        }
                    ]
                }
            )
        else:
            payload = b""
            if RESP_SSE and os.path.exists(RESP_SSE):
                with open(RESP_SSE, "rb") as f:
                    payload = f.read()
            if MODE == "toolcall":
                payload = sse(
                    {
                        "candidates": [
                            {
                                "content": {
                                    "parts": [{"text": FINAL_TEXT}],
                                    "role": "model",
                                },
                                "finishReason": "STOP",
                                "index": 0,
                            }
                        ]
                    }
                )
            if not payload:
                payload = sse(
                    {
                        "candidates": [
                            {
                                "content": {
                                    "parts": [{"text": FINAL_TEXT}],
                                    "role": "model",
                                },
                                "finishReason": "STOP",
                                "index": 0,
                            }
                        ]
                    }
                )
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=UTF-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        variation.emit(self.wfile, payload)

    do_POST = _handle
    do_GET = _handle

    def log_message(self, *a):
        pass


# The Unleash feature set, replayed from the recorded response. Only the
# flag the Lab depends on is served: the real list carries ~450 entries the
# harness never asks about individually, and an invented one would be a
# claim nobody measured.
UNLEASH_FEATURES = {
    "version": 2,
    "features": [
        {
            "name": "json-hooks-enabled",
            "type": "release",
            "description": "Whether to enable hooks based on json files",
            "enabled": True,
            "stale": False,
            "impressionData": False,
            "project": "default",
            "strategies": [
                {
                    "name": "flexibleRollout",
                    "constraints": [
                        {
                            "contextName": "ide",
                            "operator": "IN",
                            "caseInsensitive": False,
                            "inverted": False,
                            "values": ["jetski"],
                        }
                    ],
                    "parameters": {
                        "groupId": "json-hooks-enabled",
                        "rollout": "100",
                        "stickiness": "default",
                    },
                    "variants": [],
                }
            ],
            "variants": [],
        }
    ],
}

# The second delivery path for the same gate. `enable-hook-status` and
# `enable-generative-hooks` were observed false on the same account and are
# served that way rather than omitted, so the harness sees the shape it saw
# online.
LIST_EXPERIMENTS = {
    "experimentIds": [],
    "flags": [
        {"name": "json-hooks-enabled", "boolValue": True},
        {"name": "enable-hook-status", "boolValue": False},
        {"name": "enable-generative-hooks", "boolValue": False},
    ],
}


def unleash_features():
    """The recorded feature set, or — under `UNLEASH_UNCONSTRAINED=1` — the
    same flag with its `ide IN [jetski]` constraint dropped. The switch is a
    diagnostic: it separates "the harness never received the flag" from
    "the harness received it and its own context did not satisfy the
    strategy". A canonical run always serves the recording."""
    if not os.environ.get("UNLEASH_UNCONSTRAINED"):
        return UNLEASH_FEATURES
    relaxed = json.loads(json.dumps(UNLEASH_FEATURES))
    for feature in relaxed["features"]:
        for strategy in feature["strategies"]:
            strategy["constraints"] = []
    return relaxed


class Flags(BaseHTTPRequestHandler):
    """The harness's control plane: feature flags, and the account/telemetry
    endpoints it calls around them. Every request is logged with its Host so
    a run records which delivery path this harness build actually used."""

    def _answer(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _handle(self):
        capture.read_body(self)
        host = self.headers.get("Host", "?")
        print(f"[provider:flags] {self.command} {host}{self.path}", flush=True)
        if self.path.startswith("/api/client/register"):
            self._answer(202, {})
        elif self.path.startswith("/api/client/features"):
            self._answer(200, unleash_features())
        elif self.path.endswith(":listExperiments"):
            self._answer(200, LIST_EXPERIMENTS)
        else:
            # Everything else the CLI touches online (fetchUserInfo,
            # retrieveUserQuotaSummary, loadCodeAssist, setUserSettings,
            # fetchAdminControls, fetchAvailableModels,
            # recordCodeAssistMetrics, writeTrajectoryAcls, /log): answered
            # with an empty object, which is the smallest shape that keeps
            # the harness moving and invents nothing.
            self._answer(200, {})

    do_POST = _handle
    do_GET = _handle
    do_PUT = _handle

    def log_message(self, *a):
        pass


def serve_flags():
    server = HTTPServer(("0.0.0.0", 443), Flags)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(LEAF_CERT, LEAF_KEY)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    if os.path.exists(LEAF_CERT):
        threading.Thread(target=serve_flags, daemon=True).start()
    HTTPServer(("0.0.0.0", port), H).serve_forever()
