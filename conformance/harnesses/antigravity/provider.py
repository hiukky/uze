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
import re
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

STRUCT_PATH = os.environ.get("PROVIDER_STRUCT", "/tmp/agy-provider-struct.json")
RESP_SSE = os.environ.get("PROVIDER_RESP", "")
MODE = os.environ.get("PROVIDER_MODE", "static")
FC_ARGS = json.loads(os.environ.get("FC_ARGS",
                                     '{"serverName":"uze-conformance","toolName":"uze_conformance","arguments":{}}'))
FINAL_TEXT = os.environ.get("FINAL_TEXT", "UZE_CONFORMANCE_PASS")
MCP_PROOF = os.environ.get("MCP_PROOF", "UZE_MCP_CONFORMANCE_PROOF_1")

SKILL_MARKERS = ["flow:commit", "flow:review", "flow:analyze",
                 "commit", "review", "analyze", "init"]
TOOL_NAMES = ["grep_search", "list_dir", "manage_task", "read_url_content",
              "replace_file_content", "run_command", "schedule", "search_web",
              "view_file", "write_to_file", "generate_image", "call_mcp_tool"]
COUNTER = {"n": 0}


def structural_summary(body_text):
    b = json.loads(body_text) if body_text else {}
    body = json.dumps(b)
    has_fc = "functionCall" in body
    has_fr = "functionResponse" in body
    return {
        "content_roles": [c.get("role") for c in b.get("contents", [])],
        "tools": [t.get("name") for t in
                  (b.get("tools") or [{}])[0].get("functionDeclarations", [])]
                  if b.get("tools") else [],
        "tool_config": b.get("toolConfig"),
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "has_function_call": has_fc,
        "has_function_response": has_fr,
        "mcp_proof_present": MCP_PROOF in body,
        "has_user_request_tag": "<USER_REQUEST>" in body,
    }


def sse(obj):
    return f'data: {json.dumps(obj)}\n\n'.encode()


class H(BaseHTTPRequestHandler):
    def _handle(self):
        length = int(self.headers.get("Content-Length", 0) or 0)
        body = self.rfile.read(length).decode("utf-8", "replace") if length else ""
        n = COUNTER["n"]; COUNTER["n"] += 1
        rec = {"method": self.command, "path": self.path,
               "seq": n, "summary": structural_summary(body)}
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

        if MODE == "toolcall" and n == 0:
            fc = {"functionCall": {"name": "call_mcp_tool", "args": FC_ARGS}}
            payload = sse({"candidates": [{"content": {"parts": [fc], "role": "model"},
                                          "finishReason": "STOP", "index": 0}]})
        else:
            payload = b''
            if RESP_SSE and os.path.exists(RESP_SSE):
                with open(RESP_SSE, "rb") as f:
                    payload = f.read()
            if MODE == "toolcall" and n > 0:
                payload = sse({"candidates": [{"content": {"parts": [{"text": FINAL_TEXT}],
                                                           "role": "model"},
                                              "finishReason": "STOP", "index": 0}]})
            if not payload:
                payload = sse({"candidates": [{"content": {"parts": [{"text": FINAL_TEXT}],
                                                           "role": "model"},
                                              "finishReason": "STOP", "index": 0}]})
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=UTF-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    do_POST = _handle
    do_GET = _handle

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    HTTPServer(("0.0.0.0", port), H).serve_forever()
