#!/usr/bin/env python3
"""Deterministic unit tests for the provider-observation helpers.

Run without the Lab: no docker, no harness binaries.
`python3 conformance/tests/test_markers.py`.
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.common import observed_markers


def request(path, **markers):
    return {"path": path, "summary": {"hook_markers": markers}}


class ObservedMarkersTest(unittest.TestCase):
    def test_a_marker_seen_once_stays_seen(self):
        """The regression: a model call carried the denial, then a telemetry
        batch without it arrived — the denial must survive the batch."""
        turn = [
            request("/v1/messages", **{"blocked by protect-env": False}),
            request("/v1/messages", **{"blocked by protect-env": True}),
            request("/api/event_logging/v2/batch", **{"blocked by protect-env": False}),
        ]
        self.assertTrue(
            observed_markers(turn, "hook_markers")["blocked by protect-env"]
        )

    def test_a_marker_never_seen_is_reported_false(self):
        turn = [request("/v1/messages", **{"second-handler-ran": False})]
        self.assertFalse(observed_markers(turn, "hook_markers")["second-handler-ran"])

    def test_requests_without_the_field_are_ignored(self):
        turn = [{"path": "/api/hello", "summary": {}}, {"path": "/v1/messages"}]
        self.assertEqual(observed_markers(turn, "hook_markers"), {})


if __name__ == "__main__":
    unittest.main()
