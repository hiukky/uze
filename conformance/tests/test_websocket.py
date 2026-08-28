#!/usr/bin/env python3
"""Deterministic unit tests for the minimal WebSocket server side
(conformance/shared/websocket.py) — frame encode/decode roundtrips over
real `socket.socketpair`, including masking, fragmentation, ping/pong and
close. No docker. `python3 conformance/tests/test_websocket.py`.
"""

import os
import socket
import sys
import threading
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared import websocket


def masked_client_frame(opcode, payload, fin=True):
    """Builds a client-to-server frame: masked, per RFC 6455."""
    mask = b"\x11\x22\x33\x44"
    header = bytes([(0x80 if fin else 0x00) | opcode])
    length = len(payload)
    if length < 126:
        header += bytes([0x80 | length])
    elif length < 65536:
        header += bytes([0x80 | 126]) + length.to_bytes(2, "big")
    else:
        header += bytes([0x80 | 127]) + length.to_bytes(8, "big")
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    return header + mask + masked


class FrameTest(unittest.TestCase):
    def roundtrip(self, fragments):
        left, right = socket.socketpair()
        try:
            for entry in fragments:
                opcode, part, fin = (entry + (True,))[:3]
                left.sendall(masked_client_frame(opcode, part, fin=fin))
            left.sendall(masked_client_frame(0x8, b""))
            opcode, payload = websocket.read_message(right)
            return opcode, payload
        finally:
            left.close()
            right.close()

    def test_single_text_frame(self):
        opcode, payload = self.roundtrip([(0x1, b'{"input": "hi"}')])
        self.assertEqual(opcode, 0x1)
        self.assertEqual(payload, b'{"input": "hi"}')

    def test_fragmented_message_reassembles(self):
        opcode, payload = self.roundtrip(
            [(0x1, b'{"in', False), (0x0, b'put": "hi"}', False), (0x0, b"", True)]
        )
        self.assertEqual(opcode, 0x1)
        self.assertEqual(payload, b'{"input": "hi"}')

    def test_close_frame_terminates(self):
        left, right = socket.socketpair()
        left.sendall(masked_client_frame(0x8, b""))
        self.assertEqual(websocket.read_message(right), (None, None))
        left.close()
        right.close()

    def test_large_payload_two_byte_length(self):
        payload = b"x" * 300
        opcode, got = self.roundtrip([(0x1, payload)])
        self.assertEqual(got, payload)

    def test_large_payload_eight_byte_length(self):
        payload = b"x" * 70000
        opcode, got = self.roundtrip([(0x1, payload)])
        self.assertEqual(got, payload)

    def test_ping_is_answered_with_pong(self):
        left, right = socket.socketpair()
        left.sendall(masked_client_frame(0x9, b"keepalive"))
        # server answers pong immediately, then a text message follows
        left.sendall(masked_client_frame(0x1, b"after"))
        opcode, payload = websocket.read_message(right)
        self.assertEqual(opcode, 0x1)
        self.assertEqual(payload, b"after")
        left.close()
        right.close()


class ServeLoopTest(unittest.TestCase):
    def test_serve_answers_each_message_and_closes(self):
        left, right = socket.socketpair()

        def respond(body):
            return b'{"echo": ' + body.encode() + b"}"

        thread = threading.Thread(target=websocket.serve, args=(right, respond))
        thread.start()
        left.sendall(masked_client_frame(0x1, b"one"))
        # first response
        left.settimeout(5)
        header = left.recv(2)
        n = header[1] & 0x7F
        body = left.recv(n)
        self.assertEqual(body, b'{"echo": one}')
        left.sendall(masked_client_frame(0x8, b""))
        thread.join(timeout=5)
        left.close()
        right.close()


if __name__ == "__main__":
    unittest.main()
