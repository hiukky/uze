#!/usr/bin/env python3
"""Minimal deterministic fake Anthropic API provider — the Claude Code stub.

Contract (derived from observed behavior of the REAL claude 2.1.245):
  * The interactive TUI runs a connectivity preflight against hardcoded hosts
    (api.anthropic.com, platform.claude.com, ...). This stub terminates TLS
    for those hosts (cert for all SANs, injected CA via NODE_EXTRA_CA_CERTS +
    /etc/hosts) and answers non-/v1/messages paths with a synthetic 200.
  * Model calls are `POST /v1/messages` (stream=true) in the Anthropic
    Messages API shape; the deterministic response is an SSE stream of
    message_start/content_block_start/content_block_delta/content_block_stop/
    message_delta/message_stop events.

Modes (PROVIDER_MODE):
  static   : every /v1/messages turn -> a text SSE (RESPONSE_TEXT).
  toolcall : /v1/messages carrying a user text turn -> tool_use SSE
             (MCP_TOOL); /v1/messages carrying a tool_result -> final text
             SSE (FINAL_TEXT). Proves the deep MCP tool-call path.

This stub records ONLY a structural summary of each request (skill markers,
tool names, tool_use/tool_result presence, MCP proof) — never the verbatim
body — into PROVIDER_STRUCT: the model-facing observation boundary.

Env:
  PROVIDER_MODE, PROVIDER_STRUCT, RESPONSE_TEXT, FINAL_TEXT, MCP_TOOL,
  MCP_PROOF, LEAF_CERT, LEAF_KEY
"""

import json
import os
import ssl
from http.server import BaseHTTPRequestHandler, HTTPServer

import capture
import variation

STRUCT_PATH = os.environ.get("PROVIDER_STRUCT", "/tmp/claude-struct.json")
MODE = os.environ.get("PROVIDER_MODE", "static")
RESPONSE_TEXT = os.environ.get("RESPONSE_TEXT", "UZE_CONFORMANCE_OK")
FINAL_TEXT = os.environ.get("FINAL_TEXT", "UZE_CONFORMANCE_PASS")
MCP_TOOL = os.environ.get("MCP_TOOL", "mcp__uze-conformance__uze_conformance")
MCP_PROOF = os.environ.get("MCP_PROOF", "UZE_MCP_CONFORMANCE_PROOF_1")
LEAF_CERT = os.environ.get("LEAF_CERT", "/app/leaf.crt")
LEAF_KEY = os.environ.get("LEAF_KEY", "/app/leaf.key")

# The hook scenarios script a tool call to the harness's native shell tool;
# the MCP phases keep their own default. TOOL_ARGS mirrors the `input` the
# hook's normalized ABI payload will carry.
TOOL_NAME = os.environ.get("TOOL_NAME", MCP_TOOL)
TOOL_ARGS = json.loads(os.environ.get("TOOL_ARGS", "{}"))

SKILL_MARKERS = ["flow:commit", "flow:review", "commit", "review", "init"]
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
    tools = [t.get("name") for t in b.get("tools", [])] if b.get("tools") else []
    has_tool_use = "tool_use" in body
    has_tool_result = "tool_result" in body
    return {
        "content_types": [c.get("type") for c in b.get("content", [])],
        "tools": tools,
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "has_tool_use": has_tool_use,
        "has_tool_result": has_tool_result,
        "hook_markers": {m: (m in body) for m in HOOK_MARKERS},
        "mcp_proof_present": MCP_PROOF in body,
        "user_text_present": '"type": "text"' in body and '"role": "user"' in body,
        "preview": body[:900],
        "len": len(body),
    }


def sse(events):
    return "".join(f"event: {e}\ndata: {json.dumps(d)}\n\n" for e, d in events).encode()


def text_events(text):
    return [
        (
            "message_start",
            {
                "type": "message_start",
                "message": {
                    "id": "msg_uze_1",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-opus-5",
                    "content": [],
                    "stop_reason": None,
                    "usage": {"input_tokens": 10, "output_tokens": 1},
                },
            },
        ),
        (
            "content_block_start",
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            },
        ),
        (
            "content_block_delta",
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text},
            },
        ),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        (
            "message_delta",
            {
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                "usage": {"output_tokens": 3},
            },
        ),
        ("message_stop", {"type": "message_stop"}),
    ]


def tool_use_events():
    return [
        (
            "message_start",
            {
                "type": "message_start",
                "message": {
                    "id": "msg_uze_2",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-opus-5",
                    "content": [],
                    "stop_reason": None,
                    "usage": {"input_tokens": 10, "output_tokens": 1},
                },
            },
        ),
        # The real Messages API opens a tool_use block with an empty input
        # and streams the arguments as `input_json_delta`; the harness
        # accumulates only those deltas, so an input placed on the start
        # event is silently dropped and any tool with required parameters
        # is rejected as "Invalid tool parameters" before a hook can run.
        (
            "content_block_start",
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": TOOL_NAME,
                    "input": {},
                },
            },
        ),
        (
            "content_block_delta",
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": json.dumps(TOOL_ARGS),
                },
            },
        ),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        (
            "message_delta",
            {
                "type": "message_delta",
                "delta": {"stop_reason": "tool_use", "stop_sequence": None},
                "usage": {"output_tokens": 3},
            },
        ),
        ("message_stop", {"type": "message_stop"}),
    ]


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
        print(
            f"[claude-provider:{MODE}] {self.command} {self.path} req#{n}", flush=True
        )

        if self.path.startswith("/v1/messages"):
            b = json.loads(body) if body else {}
            msgs = json.dumps(b.get("messages", []))
            if "tool_result" in msgs:
                payload = sse(text_events(FINAL_TEXT))
            elif MODE == "toolcall" and '"type": "text"' in msgs:
                payload = sse(tool_use_events())
            else:
                payload = sse(text_events(RESPONSE_TEXT))
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
        else:
            payload = b'{"success":true}'
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
