#!/usr/bin/env python3
"""Deterministic unit tests for adversarial provider variations.

Pure chunk-stream semantics — no docker, no harness binaries.
`python3 conformance/tests/test_variation.py`.
"""

import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared import variation

PAYLOAD = (
    b'event: a\ndata: {"n":1}\n\nevent: b\ndata: {"n":2}\n\nevent: c\ndata: {"n":3}\n\n'
)


def time_sleep_noop(_):
    pass


def consume(payload, spec, sleep=time_sleep_noop):
    return b"".join(variation.chunk_stream(payload, spec, sleep=sleep))


class ParseTest(unittest.TestCase):
    def test_empty_spec_is_no_steps(self):
        self.assertEqual(variation.parse(""), ([], []))
        self.assertEqual(variation.parse(None), ([], []))

    def test_unknown_kind_is_reported_invalid(self):
        steps, invalid = variation.parse("slow_sse:0.4,boom:1")
        self.assertEqual(steps, [("slow_sse", "0.4"), ("boom", "1")])
        self.assertEqual(invalid, ["boom:1"])


class ChunkStreamTest(unittest.TestCase):
    def test_no_spec_is_a_single_verbatim_chunk(self):
        chunks = list(variation.chunk_stream(PAYLOAD, "", sleep=time_sleep_noop))
        self.assertEqual(chunks, [PAYLOAD])

    def test_slow_sse_preserves_payload_and_sleeps(self):
        sleeps = []

        def spy(_):
            sleeps.append(1)

        out = consume(PAYLOAD, "slow_sse:1.0", sleep=spy)
        self.assertEqual(out, PAYLOAD)
        # one sleep per frame, nothing else
        self.assertEqual(len(sleeps), 3)

    def test_disconnect_after_truncates_without_completion(self):
        out = consume(PAYLOAD, "disconnect_after:2")
        self.assertEqual(out, b'event: a\ndata: {"n":1}\n\nevent: b\ndata: {"n":2}\n\n')
        self.assertNotIn(b"event: c", out)

    def test_duplicate_repeats_the_named_frame(self):
        out = consume(PAYLOAD, "duplicate:event: b")
        self.assertEqual(out.count(b"event: b"), 2)
        self.assertEqual(out.count(b"event: a"), 1)

    def test_malformed_corrupts_only_the_named_frame(self):
        out = consume(PAYLOAD, "malformed:event: b")
        self.assertIn(b"corrupted-by-uze-variation", out)
        self.assertIn(b"event: a", out)
        self.assertIn(b"event: c", out)

    def test_chopped_splits_the_named_frame(self):
        chunk = consume(
            PAYLOAD, "chopped:2", sleep=mock.Mock(side_effect=lambda _: None)
        )
        self.assertEqual(chunk, PAYLOAD)

    def test_unknown_kind_is_recorded_never_silent(self):
        # The stream still carries the exact payload (unknown kinds have
        # nothing to act on), but the record file captures the unsupported
        # step so the experiment sees the tolerance.
        with mock.patch("shared.variation._unsupported_record") as record:
            writer = mock.Mock()
            variation.emit(writer, PAYLOAD, spec="boom:1")
            called = [c.args for c in record.call_args_list]
            self.assertEqual(called[0][1], ["unknown-kind:boom:1"])
        written = b"".join(c.args[0] for c in writer.write.call_args_list)
        self.assertEqual(written, PAYLOAD)

    def test_emit_without_spec_writes_nothing_extra(self):
        writer = mock.Mock()
        variation.emit(writer, PAYLOAD, spec="")
        writer.write.assert_called_once_with(PAYLOAD)


if __name__ == "__main__":
    unittest.main()
