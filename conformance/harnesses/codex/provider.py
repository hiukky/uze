#!/usr/bin/env python3
"""Minimal deterministic fake OpenAI API provider — the Codex stub.

Contract (derived from observed behavior of the REAL codex-cli 0.149.1):
  * Codex first attempts a WebSocket to `wss://api.openai.com/v1/responses`
    (host resolved via /etc/hosts, CA injected via CODEX_CA_CERTIFICATES +
    SSL_CERT_FILE). Accepting the upgrade and cleanly closing the socket
    makes codex fall back to HTTPS.
  * The HTTPS model call is `POST /v1/responses` (stream=true, OpenAI
    Responses API), answered with SSE events (response.created /
    output_item.added / content_part.added / output_text.delta / completed).
  * Model catalog: `GET /v1/models` (served for the TUI boot panel).

This stub records ONLY a structural summary of each request (skill markers,
catalog presence, user-text presence) — never the verbatim body — into
PROVIDER_STRUCT: the model-facing observation boundary.

Env: PROVIDER_STRUCT, RESPONSE_TEXT, LEAF_CERT, LEAF_KEY, CA_TRUST_FILE
"""

import base64
import hashlib
import json
import os
import ssl
from http.server import BaseHTTPRequestHandler, HTTPServer

import capture
import variation
import websocket

STRUCT_PATH = os.environ.get("PROVIDER_STRUCT", "/tmp/codex-struct.json")
RESPONSE_TEXT = os.environ.get("RESPONSE_TEXT", "UZE_CONFORMANCE_OK")
MODE = os.environ.get("PROVIDER_MODE", "static")
LEAF_CERT = os.environ.get("LEAF_CERT", "/app/leaf.crt")
LEAF_KEY = os.environ.get("LEAF_KEY", "/app/leaf.key")
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

SKILL_MARKERS = [
    "flow:commit",
    "flow:review",
    "commit",
    "review",
    "init",
    "North Star",
    "Review code",
]
COUNTER = {"n": 0}

# The hook scenarios script a tool call to the harness's native shell tool
# (`Bash`); TOOL_ARGS mirrors the tool `input` the hook's normalized ABI
# payload will carry.
TOOL_NAME = os.environ.get("TOOL_NAME", "Bash")
TOOL_ARGS = os.environ.get("TOOL_ARGS", "{}")

# Conformance evidence markers carried by portable-hook denial reasons
# (ADR-033): presence/absence in the structural summary proves what the real
# harness relayed after the hook executed.
HOOK_MARKERS = [
    "blocked by protect-env",
    "first-handler-denied",
    "second-handler-ran",
    "second-handler-reached",
    "Denied by UZE hook",
    # The real tool stdout marker: only present when the intercepted tool
    # actually executed after an allow — the deny/allow contrast relies
    # on it, not on the ambiguous presence of a tool result.
    "plain output",
]


def structural_summary(body_text):
    body = body_text or ""
    tools = []
    try:
        doc = json.loads(body)

        def walk(items, prefix=""):
            for item in items or []:
                name = item.get("name")
                if item.get("type") == "custom" and name:
                    tools.append(f"{prefix}{name}")
                walk(item.get("tools"), prefix=f"{prefix}{name}.")

        walk(doc.get("input"))
        for entry in doc.get("input") or []:
            walk(entry.get("tools"))
    except Exception:
        tools = []
    return {
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "custom_tools": tools,
        "preview": body[:900],
        "has_available_skills": "### Available skills" in body,
        "has_user_text": '"role": "user"' in body or '"input"' in body,
        "has_function_call": '"type": "function_call"' in body,
        "hook_markers": {m: (m in body) for m in HOOK_MARKERS},
        "len": len(body),
    }


def text_events(text):
    rid, mid = "resp_uze_1", "msg_uze_1"
    evs = [
        (
            "response.created",
            {
                "type": "response.created",
                "response": {
                    "id": rid,
                    "object": "response",
                    "created_at": 1750000000,
                    "status": "in_progress",
                    "model": "gpt-5.6-sol",
                    "output": [],
                    "usage": None,
                },
            },
        ),
        (
            "response.output_item.added",
            {
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": mid,
                    "role": "assistant",
                    "status": "in_progress",
                    "content": [],
                },
            },
        ),
        (
            "response.content_part.added",
            {
                "type": "response.content_part.added",
                "item_id": mid,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []},
            },
        ),
        (
            "response.output_text.delta",
            {
                "type": "response.output_text.delta",
                "item_id": mid,
                "output_index": 0,
                "content_index": 0,
                "delta": text,
            },
        ),
        (
            "response.output_text.done",
            {
                "type": "response.output_text.done",
                "item_id": mid,
                "output_index": 0,
                "content_index": 0,
                "text": text,
            },
        ),
        (
            "response.output_item.done",
            {
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {
                    "type": "message",
                    "id": mid,
                    "role": "assistant",
                    "status": "completed",
                    "content": [
                        {"type": "output_text", "text": text, "annotations": []}
                    ],
                },
            },
        ),
        (
            "response.completed",
            {
                "type": "response.completed",
                "response": {
                    "id": rid,
                    "object": "response",
                    "created_at": 1750000000,
                    "status": "completed",
                    "model": "gpt-5.6-sol",
                    "output": [
                        {
                            "type": "message",
                            "id": mid,
                            "role": "assistant",
                            "status": "completed",
                            "content": [
                                {"type": "output_text", "text": text, "annotations": []}
                            ],
                        }
                    ],
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 3,
                        "total_tokens": 13,
                    },
                },
            },
        ),
    ]
    return evs


def sse_bytes(evs):
    return "".join(f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in evs).encode()


def responses_sse(text):
    return sse_bytes(text_events(text))


def function_call_events():
    """A tool-call response's event list (Responses API): one `function_call`
    output item naming TOOL_NAME with TOOL_ARGS as its arguments. The
    harness executes the tool (through the UZE hook wrapper); the follow-up
    request carries the `function_call_output`, which the handler answers
    with the final text."""
    rid, fid = "resp_uze_2", "fc_uze_1"
    item = {
        "type": "function_call",
        "id": fid,
        "call_id": fid,
        "name": TOOL_NAME,
        "arguments": "",
        "status": "in_progress",
    }
    evs = [
        (
            "response.created",
            {
                "type": "response.created",
                "response": {
                    "id": rid,
                    "object": "response",
                    "created_at": 1750000000,
                    "status": "in_progress",
                    "model": "gpt-5.6-sol",
                    "output": [],
                    "usage": None,
                },
            },
        ),
        (
            "response.output_item.added",
            {"type": "response.output_item.added", "output_index": 0, "item": item},
        ),
        (
            "response.function_call_arguments.delta",
            {
                "type": "response.function_call_arguments.delta",
                "item_id": fid,
                "output_index": 0,
                "delta": TOOL_ARGS,
            },
        ),
        (
            "response.function_call_arguments.done",
            {
                "type": "response.function_call_arguments.done",
                "item_id": fid,
                "output_index": 0,
                "arguments": TOOL_ARGS,
            },
        ),
        (
            "response.output_item.done",
            {
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {**item, "arguments": TOOL_ARGS, "status": "completed"},
            },
        ),
        (
            "response.completed",
            {
                "type": "response.completed",
                "response": {
                    "id": rid,
                    "object": "response",
                    "created_at": 1750000000,
                    "status": "completed",
                    "model": "gpt-5.6-sol",
                    "output": [{**item, "arguments": TOOL_ARGS, "status": "completed"}],
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 3,
                        "total_tokens": 13,
                    },
                },
            },
        ),
    ]
    return evs


def function_call_sse():
    return sse_bytes(function_call_events())


def respond(body, path):
    """The request → payload mapping shared by the HTTP and WebSocket paths
    (codex 0.150.1 speaks the Responses API over a real WebSocket)."""
    if path.startswith("/v1/responses"):
        # A tool call is only scripted for a real turn: the TUI also
        # sends a boot/connectivity request without `input`/`inputs`,
        # and answering that with a function call hangs its model load.
        has_turn = '"input"' in body or '"inputs"' in body
        if MODE == "toolcall" and has_turn and '"function_call_output"' not in body:
            return function_call_sse()
        return responses_sse(RESPONSE_TEXT)
    if path.startswith("/v1/models"):
        return json.dumps(
            {
                "object": "list",
                "data": [
                    {
                        "id": "gpt-5.6-sol",
                        "object": "model",
                        "created_at": 1750000000,
                        "owned_by": "openai",
                    },
                    {
                        "id": "o3",
                        "object": "model",
                        "created_at": 1750000000,
                        "owned_by": "openai",
                    },
                ],
            }
        ).encode()
    return b'{"ok":true}'


def record(body, path, method):
    """Structural evidence for one request — shared by the HTTP handler
    and the per-message WebSocket loop (the real turn bodies arrive in WS
    frames after the upgrade, so an HTTP-only read would miss them)."""
    n = COUNTER["n"]
    COUNTER["n"] += 1
    rec = {
        "method": method,
        "path": path,
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
    print(f"[codex-provider] {method} {path} req#{n}", flush=True)


def ws_loop(conn, path):
    """Serves the real WebSocket the 0.150.1 harness uses for the Responses
    API: records each frame's body as evidence, answers with one JSON event
    per frame (codex's `ResponsesStreamEvent` deserializes each WS text
    frame directly — SSE framing is the HTTP transport's, not the WS
    protocol's), closes cleanly after `response.completed`."""

    def handle(text):
        if os.environ.get("DISCOVERY"):
            try:
                with open("/app/raw-requests.log", "ab") as f:
                    f.write(f"### WS {path}\n{text}\n\n".encode())
            except OSError:
                pass
        record(text, path, "WS")
        has_turn = '"input"' in text or '"inputs"' in text
        if MODE == "toolcall" and has_turn and '"function_call_output"' not in text:
            events = function_call_events()
        else:
            events = text_events(RESPONSE_TEXT)
        for _name, payload in events:
            websocket.send_text(conn, json.dumps(payload).encode())
        return None

    websocket.serve(conn, handle)


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _handle(self):

        capture.capture(self)
        ln = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(ln).decode("utf-8", "replace") if ln else ""
        record(body, self.path, self.command)

        if self.headers.get("Upgrade", "").lower() == "websocket":
            key = self.headers.get("Sec-WebSocket-Key", "")
            accept = base64.b64encode(
                hashlib.sha1((key + WS_GUID).encode()).digest()
            ).decode()
            self.send_response(101)
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", accept)
            self.end_headers()
            # codex 0.150.1 sends the Responses request over a real
            # WebSocket and treats accept-then-close as a mid-stream
            # disconnect (endless "Reconnecting..." loop). Serve the
            # protocol: read masked frames, answer each message, close
            # cleanly — the harness's own stream semantics.
            ws_loop(self.connection, self.path)
            return
        payload = respond(body, self.path)
        if self.path.startswith("/v1/responses"):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
        elif self.path.startswith("/v1/models"):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
        else:
            payload = b'{"ok":true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        variation.emit(self.wfile, payload)

    do_POST = _handle
    do_GET = _handle

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    srv = HTTPServer(("0.0.0.0", 443), H)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(LEAF_CERT, LEAF_KEY)
    srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
    srv.serve_forever()
