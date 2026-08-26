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
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

STRUCT_PATH = os.environ.get("PROVIDER_STRUCT", "/tmp/codex-struct.json")
RESPONSE_TEXT = os.environ.get("RESPONSE_TEXT", "UZE_CONFORMANCE_OK")
MODE = os.environ.get("PROVIDER_MODE", "static")
LEAF_CERT = os.environ.get("LEAF_CERT", "/app/leaf.crt")
LEAF_KEY = os.environ.get("LEAF_KEY", "/app/leaf.key")
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

SKILL_MARKERS = ["flow:commit", "flow:review", "commit", "review", "init",
                 "North Star", "Review code"]
COUNTER = {"n": 0}

# The hook scenarios script a tool call to the harness's native shell tool
# (`Bash`); TOOL_ARGS mirrors the tool `input` the hook's normalized ABI
# payload will carry.
TOOL_NAME = os.environ.get("TOOL_NAME", "Bash")
TOOL_ARGS = os.environ.get("TOOL_ARGS", "{}")

# Conformance evidence markers carried by portable-hook denial reasons
# (ADR-033): presence/absence in the structural summary proves what the real
# harness relayed after the hook executed.
HOOK_MARKERS = ["blocked by protect-env", "first-handler-denied",
                "second-handler-ran", "second-handler-reached",
                "Denied by UZE hook"]


def structural_summary(body_text):
    body = body_text or ""
    return {
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "has_available_skills": "### Available skills" in body,
        "has_user_text": '"role": "user"' in body or '"input"' in body,
        "has_function_call": '"type": "function_call"' in body,
        "hook_markers": {m: (m in body) for m in HOOK_MARKERS},
        "len": len(body),
    }


def responses_sse(text):
    rid, mid = "resp_uze_1", "msg_uze_1"
    evs = [
        ("response.created", {"type": "response.created", "response": {
            "id": rid, "object": "response", "created_at": 1750000000,
            "status": "in_progress", "model": "gpt-5.6-sol", "output": [],
            "usage": None}}),
        ("response.output_item.added", {"type": "response.output_item.added",
                                        "output_index": 0,
                                        "item": {"type": "message", "id": mid,
                                                 "role": "assistant",
                                                 "status": "in_progress",
                                                 "content": []}}),
        ("response.content_part.added", {"type": "response.content_part.added",
                                         "item_id": mid, "output_index": 0,
                                         "content_index": 0,
                                         "part": {"type": "output_text",
                                                  "text": "", "annotations": []}}),
        ("response.output_text.delta", {"type": "response.output_text.delta",
                                        "item_id": mid, "output_index": 0,
                                        "content_index": 0, "delta": text}),
        ("response.output_text.done", {"type": "response.output_text.done",
                                       "item_id": mid, "output_index": 0,
                                       "content_index": 0, "text": text}),
        ("response.output_item.done", {"type": "response.output_item.done",
                                       "output_index": 0,
                                       "item": {"type": "message", "id": mid,
                                                "role": "assistant",
                                                "status": "completed",
                                                "content": [{"type": "output_text",
                                                             "text": text,
                                                             "annotations": []}]}}),
        ("response.completed", {"type": "response.completed", "response": {
            "id": rid, "object": "response", "created_at": 1750000000,
            "status": "completed", "model": "gpt-5.6-sol",
            "output": [{"type": "message", "id": mid, "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": text,
                                     "annotations": []}]}],
            "usage": {"input_tokens": 10, "output_tokens": 3,
                      "total_tokens": 13}}}),
    ]
    return "".join(f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in evs).encode()


def function_call_sse():
    """A tool-call response (Responses API): one `function_call` output item
    naming TOOL_NAME with TOOL_ARGS as its arguments. The harness executes
    the tool (through the UZE hook wrapper); the follow-up request carries
    the `function_call_output`, which the handler answers with the final
    text."""
    rid, fid = "resp_uze_2", "fc_uze_1"
    item = {"type": "function_call", "id": fid, "call_id": fid,
            "name": TOOL_NAME, "arguments": "", "status": "in_progress"}
    evs = [
        ("response.created", {"type": "response.created", "response": {
            "id": rid, "object": "response", "created_at": 1750000000,
            "status": "in_progress", "model": "gpt-5.6-sol", "output": [],
            "usage": None}}),
        ("response.output_item.added", {"type": "response.output_item.added",
                                        "output_index": 0, "item": item}),
        ("response.function_call_arguments.delta",
         {"type": "response.function_call_arguments.delta",
          "item_id": fid, "output_index": 0, "delta": TOOL_ARGS}),
        ("response.function_call_arguments.done",
         {"type": "response.function_call_arguments.done",
          "item_id": fid, "output_index": 0, "arguments": TOOL_ARGS}),
        ("response.output_item.done", {"type": "response.output_item.done",
                                       "output_index": 0,
                                       "item": {**item, "arguments": TOOL_ARGS,
                                                "status": "completed"}}),
        ("response.completed", {"type": "response.completed", "response": {
            "id": rid, "object": "response", "created_at": 1750000000,
            "status": "completed", "model": "gpt-5.6-sol",
            "output": [{**item, "arguments": TOOL_ARGS, "status": "completed"}],
            "usage": {"input_tokens": 10, "output_tokens": 3,
                      "total_tokens": 13}}}),
    ]
    return "".join(f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in evs).encode()


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _handle(self):
        ln = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(ln).decode("utf-8", "replace") if ln else ""
        n = COUNTER["n"]; COUNTER["n"] += 1
        rec = {"method": self.command, "path": self.path, "seq": n,
               "summary": structural_summary(body)}
        struct = []
        if os.path.exists(STRUCT_PATH):
            try:
                struct = json.load(open(STRUCT_PATH))
            except Exception:
                struct = []
        struct.append(rec)
        with open(STRUCT_PATH, "w") as f:
            json.dump(struct, f, indent=1)
        print(f"[codex-provider] {self.command} {self.path} req#{n}", flush=True)

        if self.headers.get("Upgrade", "").lower() == "websocket":
            key = self.headers.get("Sec-WebSocket-Key", "")
            accept = base64.b64encode(
                hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
            self.send_response(101)
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", accept)
            self.end_headers()
            try:
                self.wfile.write(bytes([0x88, 0x00]))  # clean WS close
            except Exception:
                pass
            return
        if self.path.startswith("/v1/responses"):
            if MODE == "toolcall" and '"function_call_output"' not in body:
                payload = function_call_sse()
            else:
                payload = responses_sse(RESPONSE_TEXT)
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
        elif self.path.startswith("/v1/models"):
            payload = json.dumps({"object": "list", "data": [
                {"id": "gpt-5.6-sol", "object": "model", "created_at": 1750000000,
                 "owned_by": "openai"},
                {"id": "o3", "object": "model", "created_at": 1750000000,
                 "owned_by": "openai"}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
        else:
            payload = b'{"ok":true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

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
