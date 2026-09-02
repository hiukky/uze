#!/usr/bin/env python3
"""Minimal deterministic fake Gemini provider — the permanent runtime stub.

Contract (derived from observed behavior of the REAL AGY 1.1.20):
  * AGY (API-key mode) POSTs to `{GOOGLE_GEMINI_BASE_URL}/v1beta/models/
    {model}:streamGenerateContent?alt=sse` with a GenerateContent JSON body.
  * The response is an SSE stream of `data: {json}` lines. IMPORTANT: it must
    NOT contain a `data: [DONE]` terminal — AGY's stream parser fails on it
    (observed: "error unmarshalling data data: [DONE]").

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
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

import capture
import variation

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


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    HTTPServer(("0.0.0.0", port), H).serve_forever()
