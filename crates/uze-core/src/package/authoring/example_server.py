"""The stub server the example key in mcp.json runs.

A complete MCP stdio server with no dependencies: newline-delimited
JSON-RPC on stdin/stdout, one tool. Swap it for your real server — or
delete the key in mcp.json and this file with it.

stdout carries the protocol and nothing else; anything meant for a person
goes to stderr, or the harness reads it as a broken message and drops the
connection.
"""

import json
import sys

SERVER = {"name": "example", "version": "0.1.0"}
PROTOCOL_VERSION = "2025-06-18"

TOOLS = [
    {
        "name": "hello",
        "description": "Answers with a greeting, proving the server is wired.",
        "inputSchema": {
            "type": "object",
            "properties": {"name": {"type": "string"}},
        },
    }
]


def call_tool(name, arguments):
    if name != "hello":
        raise LookupError(f"unknown tool: {name}")
    who = arguments.get("name") or "world"
    return {"content": [{"type": "text", "text": f"hello {who}"}]}


def answer(method, params):
    if method == "initialize":
        return {
            "protocolVersion": params.get("protocolVersion", PROTOCOL_VERSION),
            "capabilities": {"tools": {}},
            "serverInfo": SERVER,
        }
    if method == "ping":
        return {}
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        return call_tool(params.get("name"), params.get("arguments") or {})
    raise NotImplementedError(method)


def reply(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def main():
    for line in sys.stdin:
        if not line.strip():
            continue
        request = json.loads(line)
        if "id" not in request:
            continue  # a notification expects no answer
        try:
            result = answer(request.get("method"), request.get("params") or {})
            reply({"jsonrpc": "2.0", "id": request["id"], "result": result})
        except NotImplementedError as missing:
            reply(
                {
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {
                        "code": -32601,
                        "message": f"method not found: {missing}",
                    },
                }
            )
        except LookupError as unknown:
            reply(
                {
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {"code": -32602, "message": str(unknown)},
                }
            )


if __name__ == "__main__":
    main()
