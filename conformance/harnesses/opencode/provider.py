#!/usr/bin/env python3
"""Minimal deterministic fake OpenAI-compatible provider — the OpenCode stub.

Contract (derived from observed behavior of the REAL opencode 1.18.23):
  * The provider is declared in the global opencode.json with a custom
    `baseURL` (`@ai-sdk/openai-compatible`) — unlike claude/codex there is NO
    hardcoded host to intercept; the TUI talks plain HTTP to this server.
  * Model calls are `POST /v1/chat/completions` (stream=true, OpenAI Chat
    Completions API), answered with SSE `data:` chunks and a `data: [DONE]`
    terminal.
  * Tool calls follow the streaming shape the AI SDK tracks: a first delta
    carrying `role`/`id`/`name`, a second delta with `arguments`, then
    `finish_reason: "tool_calls"`. After the harness executes the MCP tool
    and returns `role: "tool"` content, the next turn answers the final
    text.

Modes (PROVIDER_MODE):
  static   : every turn -> a text SSE (RESPONSE_TEXT).
  toolcall : the first request carrying tools (no tool result yet) ->
             tool call SSE to the UZE MCP tool; requests carrying the
             `role: "tool"` result -> final text SSE (FINAL_TEXT). Proves
             the deep MCP round-trip with zero model.

This stub records ONLY a structural summary of each request (skill markers,
catalog presence, tool-result presence, MCP proof) — never the verbatim
body — into PROVIDER_STRUCT: the model-facing observation boundary.

Env: PROVIDER_MODE, PROVIDER_STRUCT, RESPONSE_TEXT, FINAL_TEXT, MCP_PROOF
"""

import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

import capture
import variation

STRUCT_PATH = os.environ.get("PROVIDER_STRUCT", "/tmp/oc-struct.json")
MODE = os.environ.get("PROVIDER_MODE", "static")
RESPONSE_TEXT = os.environ.get("RESPONSE_TEXT", "UZE_CONFORMANCE_OK")
FINAL_TEXT = os.environ.get("FINAL_TEXT", "UZE_CONFORMANCE_PASS")
MCP_PROOF = os.environ.get("MCP_PROOF", "UZE_MCP_CONFORMANCE_PROOF_1")

# The MCP tool name the real opencode builds from the delivered server
# (`<server>-<tool>`), observed in the primary request.
MCP_TOOL = "uze-mcp-conformance-uze-conformance_uze_conformance"

# The hook scenarios script a tool call to the harness's native shell tool
# (`bash`); the MCP phases keep their own default. TOOL_ARGS mirrors the
# `arguments` the hook's normalized ABI payload will carry.
TOOL_NAME = os.environ.get("TOOL_NAME", MCP_TOOL)
TOOL_ARGS = os.environ.get("TOOL_ARGS", "{}")

SKILL_MARKERS = [
    "flow:analyze",
    "flow:commit",
    "flow:review",
    "analyze",
    "commit",
    "review",
    "init",
    "North Star",
    "Review code",
]
# Conformance evidence markers carried by portable-hook denial reasons
# (ADR-033): presence/absence in the structural summary proves what the real
# harness relayed after the bridge executed.
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
]
COUNTER = {"n": 0}


def structural_summary(body_text):
    body = body_text or ""
    return {
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "has_available_skills": (
            "### Available skills" in body or "<available_skills>" in body
        ),
        "has_user_text": '"role": "user"' in body or '"role":"user"' in body,
        "has_tool_result": ('"role": "tool"' in body or '"role":"tool"' in body),
        "hook_markers": {m: (m in body) for m in HOOK_MARKERS},
        "mcp_proof_present": MCP_PROOF in body,
        "mcp_tool_present": MCP_TOOL in body,
        "len": len(body),
    }


def sse(chunks):
    out = "".join(f"data: {json.dumps(c)}\n\n" for c in chunks)
    return (out + "data: [DONE]\n\n").encode()


def text_chunks(text):
    return [
        {
            "id": "c1",
            "object": "chat.completion.chunk",
            "model": "uze-model",
            "choices": [
                {
                    "index": 0,
                    "delta": {"role": "assistant", "content": ""},
                    "finish_reason": None,
                }
            ],
        },
        {
            "id": "c1",
            "object": "chat.completion.chunk",
            "model": "uze-model",
            "choices": [
                {"index": 0, "delta": {"content": text}, "finish_reason": None}
            ],
        },
        {
            "id": "c1",
            "object": "chat.completion.chunk",
            "model": "uze-model",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        },
    ]


def tool_call_chunks():
    return [
        {
            "id": "c1",
            "object": "chat.completion.chunk",
            "model": "uze-model",
            "choices": [
                {
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {
                                "index": 0,
                                "id": "call_uze_1",
                                "type": "function",
                                "function": {"name": TOOL_NAME, "arguments": ""},
                            }
                        ],
                    },
                    "finish_reason": None,
                }
            ],
        },
        {
            "id": "c1",
            "object": "chat.completion.chunk",
            "model": "uze-model",
            "choices": [
                {
                    "index": 0,
                    "delta": {
                        "tool_calls": [
                            {"index": 0, "function": {"arguments": TOOL_ARGS}}
                        ]
                    },
                    "finish_reason": None,
                }
            ],
        },
        {
            "id": "c1",
            "object": "chat.completion.chunk",
            "model": "uze-model",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
        },
    ]


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

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
            f"[opencode-provider:{MODE}] {self.command} {self.path} req#{n}", flush=True
        )

        has_tools = '"tools"' in body
        has_result = '"role":"tool"' in body or '"tool_result"' in body
        if MODE == "toolcall" and has_tools and not has_result:
            payload = sse(tool_call_chunks())
        elif MODE == "toolcall" and has_result:
            payload = sse(text_chunks(FINAL_TEXT))
        else:
            payload = sse(text_chunks(RESPONSE_TEXT))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Connection", "close")
        self.end_headers()
        variation.emit(self.wfile, payload)

    do_POST = _handle
    do_GET = _handle

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    srv = HTTPServer(("0.0.0.0", port), H)
    srv.serve_forever()
