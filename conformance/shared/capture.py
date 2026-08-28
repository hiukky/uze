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


def capture(handler) -> None:
    """Appends one raw request to the provider's capture log. No-op unless
    the lab enabled `DISCOVERY=1`; OSError is swallowed (a capture failure
    must never break the provider serving a harness)."""
    if not os.environ.get("DISCOVERY"):
        return
    try:
        with open("/app/raw-requests.log", "ab") as f:
            f.write(f"### {handler.command} {handler.path}\n".encode())
            for key, value in handler.headers.items():
                f.write(f"{key}: {value}\n".encode())
            length = int(handler.headers.get("Content-Length", "0") or 0)
            if length:
                f.write(handler.rfile.read(length) + b"\n\n")
    except OSError:
        pass
