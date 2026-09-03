#!/usr/bin/env python3
"""Minimal deterministic fake Gemini provider — the permanent runtime stub.

Contract (derived from observed behavior of the REAL AGY 1.1.20):
  * AGY (API-key mode) POSTs to `{GOOGLE_GEMINI_BASE_URL}/v1beta/models/
    {model}:streamGenerateContent?alt=sse` with a GenerateContent JSON body.
  * The response is an SSE stream of `data: {json}` lines. IMPORTANT: it must
    NOT contain a `data: [DONE]` terminal — AGY's stream parser fails on it
    (observed: "error unmarshalling data data: [DONE]").

It also serves the harness's **signed-in plane** over TLS on 443: the
feature flags that decide whether `hooks.json` hooks execute at all, the
identity/account endpoints, and — when the vertical runs signed in — the
model path itself. Observed on a real, logged-in session (AGY 1.1.22
binary, backend UA `antigravity/cli/1.1.24`, 2026-09-02):

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

**Signed-in ("consumer") mode.** With a `consumer` token file in place the
CLI stops honouring `GOOGLE_GEMINI_BASE_URL` and speaks the CloudCode
protocol instead. Shapes observed on the same session (structure only; every
value served here is synthetic):

  * `GET https://www.googleapis.com/oauth2/v2/userinfo` ->
    `{id, email, verified_email, name, given_name, picture}`;
  * `POST https://oauth2.googleapis.com/token` (only if a refresh is
    attempted) -> `{access_token, token_type, expires_in}`;
  * `POST https://daily-cloudcode-pa.googleapis.com/v1internal:<rpc>` with
    JSON bodies: `fetchUserInfo` -> `{userSettings, regionCode}`,
    `loadCodeAssist` -> a tier document, `listExperiments` -> the flag list
    above, and `setUserSettings` / `retrieveUserQuotaSummary` /
    `fetchAdminControls` / `fetchAvailableModels` /
    `recordCodeAssistMetrics` / `writeTrajectoryAcls` -> `{}`;
  * `POST …/v1internal:streamGenerateContent`, whose body wraps the same
    GenerateContent request as `{project, requestId, model, userAgent,
    requestType, request:{…}}` and whose SSE events wrap each
    GenerateContentResponse as `{"response": …, "traceId": …, "metadata":
    {}}`.

The model logic is the one below, unchanged: the CloudCode listener unwraps
`request` before the same summary/decision path and re-wraps every event it
emits, so `static`/`toolcall`, `wants_function_call` and the variation
machinery behave identically in both auth modes.

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
from http.server import BaseHTTPRequestHandler, HTTPServer, ThreadingHTTPServer

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

ISOLATION_MARKERS = ["already isolated", "UZE_CONFORMANCE_REBASE"]
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
# The signed-in listener is threaded (the harness polls its flag plane while
# a turn streams), so the evidence file is a shared resource: one writer at
# a time, or a run loses requests it did observe.
RECORD_LOCK = threading.Lock()
# The summaries this process has recorded. Held in memory and rewritten
# whole: re-reading the file per request made a retrying harness quadratic.
STRUCT = []


def structural_summary(body_text):
    b = json.loads(body_text) if body_text else {}
    body = json.dumps(b)
    has_fc = "functionCall" in body
    has_fr = "functionResponse" in body
    return {
        "content_roles": [c.get("role") for c in b.get("contents", [])],
        # Every declaration across every `tools` entry: signed in, the
        # harness sends one entry per tool, so reading only the first left
        # the provider believing the turn declared a single tool and never
        # answering with the call the scenario scripted.
        "tools": [
            declaration.get("name")
            for entry in (b.get("tools") or [])
            for declaration in entry.get("functionDeclarations", [])
        ],
        "tool_config": b.get("toolConfig"),
        "skill_markers": {m: (m in body) for m in SKILL_MARKERS},
        "isolation_markers": {m: (m in body) for m in ISOLATION_MARKERS},
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


def unwrap_consumer_request(body_text):
    """The GenerateContent request the signed-in envelope carries.

    CloudCode wraps it as `{project, requestId, model, userAgent,
    requestType, request:{contents, systemInstruction, …}}`; everything the
    provider decides on (declared tools, roles, markers) lives in `request`,
    so the model path sees exactly the body it sees in API-key mode."""
    try:
        wrapper = json.loads(body_text) if body_text else {}
    except ValueError:
        return body_text
    if isinstance(wrapper, dict) and isinstance(wrapper.get("request"), dict):
        return json.dumps(wrapper["request"])
    return body_text


def wrap_consumer_stream(payload):
    """Each `data: <GenerateContentResponse>` frame, re-framed as the
    signed-in envelope `data: {"response": …, "traceId": …, "metadata": {}}`.

    Framing stays `\\n\\n` — the same delimiter `variation.emit` splits on,
    so the adversarial variations keep working in this mode too."""
    out = b""
    for frame in payload.split(b"\n\n"):
        frame = frame.strip()
        if not frame.startswith(b"data: "):
            continue
        try:
            response = json.loads(frame[len(b"data: ") :])
        except ValueError:
            continue
        out += sse({"response": response, "traceId": "synthetic", "metadata": {}})
    return out


def record_request(handler, body_text):
    """Appends this request's structural summary to the run's evidence and
    returns it. Never the verbatim body — that boundary is the whole point
    of the summary (see the module docstring)."""
    with RECORD_LOCK:
        n = COUNTER["n"]
        COUNTER["n"] += 1
        rec = {
            "method": handler.command,
            "path": handler.path,
            "seq": n,
            "summary": structural_summary(body_text),
        }
        if not STRUCT and os.path.exists(STRUCT_PATH):
            try:
                STRUCT.extend(json.load(open(STRUCT_PATH)))
            except Exception:
                pass
        STRUCT.append(rec)
        with open(STRUCT_PATH, "w") as f:
            json.dump(STRUCT, f, indent=1)
    print(f"[provider:{MODE}] {handler.command} {handler.path} req#{n}", flush=True)
    return rec


def text_frame(text):
    return sse(
        {
            "candidates": [
                {
                    "content": {"parts": [{"text": text}], "role": "model"},
                    "finishReason": "STOP",
                    "index": 0,
                }
            ]
        }
    )


def model_payload(summary):
    """The SSE the mode dictates for this request — the one decision path,
    shared by the API-key listener and the signed-in one."""
    if MODE == "toolcall" and wants_function_call(summary):
        fc = {"functionCall": {"name": FC_NAME, "args": FC_ARGS}}
        return sse(
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
    if MODE == "toolcall":
        return text_frame(FINAL_TEXT)
    if RESP_SSE and os.path.exists(RESP_SSE):
        with open(RESP_SSE, "rb") as f:
            payload = f.read()
        if payload:
            return payload
    return text_frame(FINAL_TEXT)


def serve_model(handler, body_text, consumer=False):
    """Records the request and streams the answer; `consumer` re-frames
    every event into the signed-in envelope."""
    rec = record_request(handler, body_text)
    payload = model_payload(rec["summary"])
    if consumer:
        payload = wrap_consumer_stream(payload)
    handler.send_response(200)
    handler.send_header("Content-Type", "text/event-stream; charset=UTF-8")
    handler.send_header("Content-Length", str(len(payload)))
    handler.end_headers()
    variation.emit(handler.wfile, payload)


class H(BaseHTTPRequestHandler):
    def _handle(self):
        body = capture.read_body(self).decode("utf-8", "replace")
        serve_model(self, body)

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


# The signed-in identity. Every field is a literal: the Lab's account is
# nobody's, and `uze.invalid` can never resolve.
USERINFO = {
    "id": "synthetic",
    "email": "conformance@uze.invalid",
    "verified_email": True,
    "name": "UZE Conformance",
    "given_name": "UZE",
    "picture": "https://lh3.googleusercontent.com/synthetic",
}

# Served only if the CLI decides to refresh; the token fixture's expiry is
# far enough away that it should not. Answering with the same synthetic
# access token keeps a refresh from being the thing that ends a run.
REFRESHED_TOKEN = {
    "access_token": "synthetic-access-token",
    "token_type": "Bearer",
    "expires_in": 3600,
}

# The account shapes the CLI reads before a turn. Minimal by intent: a tier
# document with an id and a name, no privacy-notice text, no entitlement
# semantics invented.
FETCH_USER_INFO = {"userSettings": {"telemetryEnabled": False}, "regionCode": "us"}

# The model catalogue. In API-key mode the CLI carries its own Gemini
# catalogue (`modelProvider: gemini` + GEMINI_API_KEY); signed in, it has
# none until the backend answers `fetchAvailableModels`, and every turn dies
# with "neither PlanModel nor RequestedModel specified" — the executor
# resolves a model to an `exa.codeium_common_pb.Model` enum, and an id
# without one resolves to nothing.
#
# The response shape is
# `google.internal.cloud.code.v1internal.FetchAvailableModelsResponse`, read
# from the descriptor this binary embeds (the CLI parses it with protojson,
# so a wrong cardinality or an unknown enum name is a hard error). The
# catalogue itself is the Lab's, not a recording: two models, because the
# harness uses two (the user's turn, and a lighter side call), named for the
# newest Gemini the binary's own enum knows — the 3.x ids it ships are
# `MODEL_PLACEHOLDER_*` in this build, so no honest id/enum pair exists for
# them here. Everything else stays default.
AGENT_MODEL_ID = "gemini-3.1-pro-preview"
SIDE_MODEL_ID = "gemini-3.1-flash-lite-preview"


def model_details(display_name, model):
    return {
        "displayName": display_name,
        "model": model,
        "modelProvider": "MODEL_PROVIDER_GOOGLE",
        "maxTokens": 1000000,
        "maxOutputTokens": 65536,
    }


FETCH_AVAILABLE_MODELS = {
    "models": {
        AGENT_MODEL_ID: dict(
            model_details("Gemini 3.1 Pro", "MODEL_PLACEHOLDER_M50"),
            recommended=True,
        ),
        SIDE_MODEL_ID: model_details("Gemini 3.1 Flash Lite", "MODEL_PLACEHOLDER_M51"),
    },
    "defaultAgentModelId": AGENT_MODEL_ID,
    "agentModelSorts": [
        {
            "displayName": "Models",
            "groups": [
                {"displayName": "Gemini", "modelIds": [AGENT_MODEL_ID, SIDE_MODEL_ID]}
            ],
        }
    ],
    "commandModelIds": [SIDE_MODEL_ID],
    "commitMessageModelIds": [SIDE_MODEL_ID],
    "webSearchModelIds": [SIDE_MODEL_ID],
    "mqueryModelIds": [SIDE_MODEL_ID],
    "tabModelIds": [SIDE_MODEL_ID],
    # Each tier is a *repeated* string in the descriptor, not a single id.
    "tieredModelIds": {
        "flashLite": [SIDE_MODEL_ID],
        "flash": [SIDE_MODEL_ID],
        "pro": [AGENT_MODEL_ID],
    },
    "experimentIds": [],
}
LOAD_CODE_ASSIST = {
    "currentTier": {
        "id": "free-tier",
        "name": "Antigravity",
        "description": "Synthetic tier served by the Conformance Lab",
    },
    "allowedTiers": [{"id": "free-tier", "name": "Antigravity", "isDefault": True}],
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


class SignedIn(BaseHTTPRequestHandler):
    """The harness's signed-in plane: feature flags, the identity and
    account endpoints it calls around them, and — when the vertical runs
    signed in — the CloudCode model path. Every request is logged with its
    Host so a run records which delivery path this harness build used, and
    an unrecognized one is answered `{}` and named in the log rather than
    guessed at."""

    def _answer(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _handle(self):
        body = capture.read_body(self).decode("utf-8", "replace")
        host = self.headers.get("Host", "?")
        print(f"[provider:flags] {self.command} {host}{self.path}", flush=True)
        # The RPC is the path without its query: the model call arrives as
        # `…:streamGenerateContent?alt=sse`, and matching the raw path sent
        # it to the catch-all — a harness retrying its own turn forever.
        route = self.path.split("?", 1)[0]
        if route.endswith(":streamGenerateContent"):
            serve_model(self, unwrap_consumer_request(body), consumer=True)
        elif route.startswith("/api/client/register"):
            self._answer(202, {})
        elif route.startswith("/api/client/features"):
            self._answer(200, unleash_features())
        elif route.endswith(":listExperiments"):
            self._answer(200, LIST_EXPERIMENTS)
        elif route.startswith("/oauth2/v2/userinfo"):
            self._answer(200, USERINFO)
        elif route.startswith("/token"):
            self._answer(200, REFRESHED_TOKEN)
        elif route.endswith(":fetchUserInfo"):
            self._answer(200, FETCH_USER_INFO)
        elif route.endswith(":loadCodeAssist"):
            self._answer(200, LOAD_CODE_ASSIST)
        elif route.endswith(":fetchAvailableModels"):
            self._answer(200, FETCH_AVAILABLE_MODELS)
        else:
            # Everything else the CLI touches online (setUserSettings,
            # retrieveUserQuotaSummary, fetchAdminControls,
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
    server = ThreadingHTTPServer(("0.0.0.0", 443), SignedIn)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(LEAF_CERT, LEAF_KEY)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    server.serve_forever()


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9999
    if os.path.exists(LEAF_CERT):
        threading.Thread(target=serve_flags, daemon=True).start()
    HTTPServer(("0.0.0.0", port), H).serve_forever()
