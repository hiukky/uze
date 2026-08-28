"""Adversarial provider variations (openspec/changes/conformance-exploration-sandbox).

Scripted degraded-path behavior for the synthetic providers: slow or
chopped streaming, malformed payloads, mid-stream disconnect, and duplicated
events. Each provider routes its response emission through `emit`, so a
variation is expressed once in a spec string and honored wherever the
provider serves bytes. A kind a provider cannot express is *recorded* as
its observed tolerance (`unsupported` in the variation record), never
faked.

Spec format: comma-separated `<kind>:<arg>` steps, e.g.
`slow_sse:0.4,duplicate:message_stop` or `disconnect_after:2`.

Kinds:
- `slow_sse:<seconds>`  sleep before each event frame
- `disconnect_after:<n>`  serve only the first n frames, then close the
  stream (EOF without completion)
- `duplicate:<event>`  emit the named event's frame twice
- `malformed:<event>`  corrupt the named event's frame (invalid JSON data)
- `chopped:<n>`  split frame n in half with a pause between the halves

Unset spec = zero behavior change (single, unfragmented write).
"""

from __future__ import annotations

import json
import os
import time
from typing import Callable, Iterable

SUPPORTED_KINDS = ("slow_sse", "disconnect_after", "duplicate", "malformed", "chopped")

FRAME_DELIM = b"\n\n"


def parse(spec: str | None) -> list[tuple[str, str]]:
    """Splits a variation spec into `(kind, arg)` steps. Unparseable steps
    are dropped and reported via the returned `invalid` list — a typo must
    be visible, not silently ignored."""
    steps: list[tuple[str, str]] = []
    invalid: list[str] = []
    for token in (spec or "").split(","):
        token = token.strip()
        if not token:
            continue
        if ":" in token:
            kind, arg = token.split(":", 1)
        else:
            kind, arg = token, ""
        steps.append((kind.strip(), arg.strip()))
        if kind.strip() not in SUPPORTED_KINDS:
            invalid.append(token)
    return steps, invalid


def _split_frames(payload: bytes) -> list[bytes]:
    return [f for f in payload.split(FRAME_DELIM) if f]


def _unsupported_record(spec: str, unsupported: list[str], observed_note: str) -> None:
    """Writes the provider-side variation record next to struct.json so the
    lab can read back what was applied and what a provider cannot express —
    the observed-tolerance contract, never a fake."""
    try:
        with open("/app/variation.json", "w") as f:
            json.dump(
                {
                    "spec": spec,
                    "unsupported": unsupported,
                    "observed": observed_note,
                },
                f,
                indent=1,
            )
    except OSError:
        pass


def chunk_stream(
    payload: bytes,
    spec: str,
    sleep: Callable[[float], None] = time.sleep,
) -> Iterable[bytes]:
    """Yields the payload as byte chunks honoring the variation steps.

    No spec: yields the whole payload as a single chunk (canonical runs'
    exact behavior). `slow_sse` divides the payload into its SSE frames and
    sleeps before each; all other kinds operate on that frame list.
    """
    steps, invalid = parse(spec)
    if not steps:
        yield payload
        return

    frames = _split_frames(payload)
    # A step that names an event absent from this payload has nothing to
    # corrupt/duplicate — it is applied as a no-op, and the record below
    # only reports kind-level unsupported, so the experiment asserts on
    # what the channel actually carried.
    n_stream = int(next((a for k, a in steps if k == "disconnect_after"), "0") or 0)
    slow = next((float(a) for k, a in steps if k == "slow_sse"), 0.0)
    dup_names = [a for k, a in steps if k == "duplicate" and a]
    malformed_names = [a for k, a in steps if k == "malformed"]
    chopped_n = next((int(a) for k, a in steps if k == "chopped"), 0)

    streamed = 0
    for index, frame in enumerate(frames):
        if n_stream and streamed >= n_stream:
            break  # disconnect_after: stream ends without completion
        has_dup = any(name.encode() in frame for name in dup_names)
        for part in [frame] + ([frame] if has_dup else []):
            if slow:
                sleep(slow)
            if chopped_n and index + 1 == chopped_n:
                half = len(part) // 2
                yield part[:half]
                sleep(0.3 if slow <= 0 else slow)
                yield part[half:]
            elif any(name.encode() in part for name in malformed_names):
                # Keep the `event:` line, break the `data:` JSON.
                head = part.split(b"data:", 1)
                yield b"data: {corrupted-by-uze-variation} " + (
                    head[1] if len(head) > 1 else b""
                )
            else:
                yield part
            streamed += 1
            yield FRAME_DELIM


def emit(wfile, payload: bytes, spec: str | None = None, flush: bool = True) -> None:
    """Streams `payload` through the variation chunks into `wfile` and
    records what was applied / what a provider cannot express.

    Providers call this instead of `self.wfile.write(payload)`; the
    spec comes from their `VARIATION` environment (set by lab.py).
    """
    spec = spec if spec is not None else os.environ.get("VARIATION", "")
    _, invalid = parse(spec)
    unsupported = []
    if invalid:
        unsupported = [f"unknown-kind:{token}" for token in invalid]
    applied = "observed-tolerance" if unsupported else "applied"
    written = 0
    for chunk in chunk_stream(payload, spec):
        wfile.write(chunk)
        written += len(chunk)
    if flush:
        wfile.flush()
    if spec:
        _unsupported_record(spec, unsupported, f"{applied} ({written} bytes)")
