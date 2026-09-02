"""Raw request capture for the synthetic providers (`lab.py --discovery`).

The provider is the endpoint the real harness talks to — it already sees
every request the harness sends, with no proxy topology needed. When a
sandbox/experiment run passes `--discovery`, each provider appends the raw
request (method, path, headers, body) to `/app/raw-requests.log`; the lab
pulls that file beside the run evidence. Raw captures never enter the
repository (outdirs are ephemeral / CI artifacts only) — the same rule as
`conformance/discovery/`.
"""

from __future__ import annotations

import os


def _read_chunked(stream) -> bytes:
    """The body of a `Transfer-Encoding: chunked` request.

    `BaseHTTPRequestHandler` does not decode chunked bodies, and a harness
    that streams its request (Antigravity's signed-in CloudCode path does)
    would otherwise hand every provider an empty body — a request that
    declares tools the provider never sees, and a turn where nothing
    happens. A malformed stream stops the read rather than blocking: the
    provider must always answer.
    """
    body = b""
    while True:
        line = stream.readline()
        if not line:
            break
        try:
            size = int(line.split(b";", 1)[0].strip() or b"0", 16)
        except ValueError:
            break
        if size == 0:
            stream.readline()  # the trailer's terminating CRLF
            break
        body += stream.read(size)
        stream.readline()  # the CRLF that closes this chunk
    return body


def read_body(handler) -> bytes:
    """Reads the request body once, appending the raw request to the
    provider's capture log when the lab enabled `DISCOVERY=1`.

    The body is a stream: whoever reads it owns it. Capturing and serving
    used to read it twice, and the second read blocked on a socket with
    nothing left — every discovery run hung on its first model call. An
    OSError on the log is swallowed: a capture failure must never break
    the provider serving a harness.
    """
    if "chunked" in (handler.headers.get("Transfer-Encoding", "") or "").lower():
        body = _read_chunked(handler.rfile)
    else:
        length = int(handler.headers.get("Content-Length", "0") or 0)
        body = handler.rfile.read(length) if length else b""
    if not os.environ.get("DISCOVERY"):
        return body
    try:
        with open("/app/raw-requests.log", "ab") as f:
            f.write(f"### {handler.command} {handler.path}\n".encode())
            for key, value in handler.headers.items():
                f.write(f"{key}: {value}\n".encode())
            f.write(body + b"\n\n")
    except OSError:
        pass
    return body
