"""Minimal RFC 6455 WebSocket server side for the synthetic providers.

codex 0.150.1 speaks the Responses API over a real WebSocket and treats a
handshake-then-close as "Stream disconnected before completion" — the old
accept-and-close fake made it reconnect in a loop. This module implements
just enough of the protocol for the lab: masked client frames (with
fragmentation), unmasked server text frames, close handshake, ping/pong
tolerance. Pure stdlib, unit-tested over `socket.socketpair`.

No compression, no extensions, no RFC-6455 autobahn compliance claim —
the surface is bounded by what the real harness actually sends.
"""

from __future__ import annotations


def _read_exact(conn, n: int) -> bytes:
    chunks = []
    remaining = n
    while remaining > 0:
        chunk = conn.recv(min(remaining, 65536))
        if not chunk:
            break
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_message(conn):
    """Reads one complete client message, reassembling fragments.

    Returns `(opcode, payload)` for a finished data message, or
    `(None, None)` on a close frame, close handshake, or EOF. Ping frames
    are answered with a pong (the harness keeps the connection warm)."""
    payload = b""
    first_opcode = None
    while True:
        header = _read_exact(conn, 2)
        if len(header) < 2:
            return None, None
        fin = bool(header[0] & 0x80)
        opcode = header[0] & 0x0F
        masked = bool(header[1] & 0x80)
        length = header[1] & 0x7F
        if length == 126:
            length = int.from_bytes(_read_exact(conn, 2), "big")
        elif length == 127:
            length = int.from_bytes(_read_exact(conn, 8), "big")
        mask = _read_exact(conn, 4) if masked else b""
        data = _read_exact(conn, length)
        if masked:
            data = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
        if opcode == 0x9:  # ping -> pong
            send_frame(conn, 0xA, data)
            continue
        if opcode == 0x8:  # close
            return None, None
        if opcode in (0x1, 0x2) and first_opcode is None:
            first_opcode = opcode
        if opcode in (0x1, 0x2, 0x0):
            payload += data
        if fin:
            return first_opcode, payload


def send_frame(conn, opcode: int, payload: bytes) -> None:
    """Sends one unmasked server frame (text=0x1, pong=0xA, close=0x8)."""
    header = bytes([0x80 | opcode])
    length = len(payload)
    if length < 126:
        header += bytes([length])
    elif length < 65536:
        header += bytes([126]) + length.to_bytes(2, "big")
    else:
        header += bytes([127]) + length.to_bytes(8, "big")
    conn.sendall(header + payload)


def send_text(conn, payload: bytes) -> None:
    send_frame(conn, 0x1, payload)


def send_close(conn, code: int = 1000) -> None:
    send_frame(conn, 0x8, code.to_bytes(2, "big"))


def serve(conn, respond, timeout: float = 90.0) -> None:
    """The WS serving loop: reads client messages, answers each with
    `respond(body_text) -> bytes`, and closes cleanly when the client does.
    A timeout bounds one idle read so a harness that keeps the socket open
    without talking cannot pin the provider forever."""
    conn.settimeout(timeout)
    try:
        while True:
            opcode, payload = read_message(conn)
            if opcode is None:
                break
            if opcode in (0x1, 0x2):
                response = respond(payload.decode("utf-8", "replace"))
                if response:
                    send_text(conn, response)
    except (OSError, TimeoutError):
        pass
    finally:
        try:
            send_close(conn)
        except OSError:
            pass


if __name__ == "__main__":
    # Roundtrip smoke: unused by the providers; the unit tests cover this.
    raise SystemExit("use conformance/tests/test_websocket.py instead")
