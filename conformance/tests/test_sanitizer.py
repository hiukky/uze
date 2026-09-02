#!/usr/bin/env python3
"""Deterministic unit tests for the observation sanitizer.

Identity must not survive a capture — including identity that never appears
under a key the addon knows. A real response carried an account address
inside a URL string (`upgradeSubscriptionUri=...?Email=<addr>&...`), which
every key-based rule walked straight past.

Run without the Lab: no docker, no mitmproxy.
`python3 conformance/tests/test_sanitizer.py`.
"""

import os
import sys
import unittest

sys.path.insert(
    0, os.path.join(os.path.dirname(__file__), "..", "discovery", "mitmproxy")
)

from sanitizing_addon import (  # noqa: E402
    _mask,
    _sanitize_json_body,
    _sanitize_query,
)


class AddressesNeverSurvive(unittest.TestCase):
    def test_an_address_inside_a_url_is_masked(self):
        masked = _mask("https://pay.example/upgrade?Email=someone@corp.com&plan=pro")
        self.assertNotIn("someone@corp.com", masked)
        self.assertNotIn("someone", masked)
        self.assertIn("plan=pro", masked)

    def test_an_url_encoded_address_is_masked_by_parameter_name(self):
        masked = _mask("https://pay.example/u?Email=someone%40corp.com&plan=pro")
        self.assertNotIn("corp.com", masked)
        self.assertIn("plan=pro", masked)

    def test_a_bare_address_is_masked_whatever_its_length(self):
        # Short enough that the 24-character token heuristic never fires.
        self.assertNotIn("a@b.co", _mask("write to a@b.co"))
        self.assertIn("<UZE_MASKED_EMAIL>", _mask("write to a@b.co"))

    def test_the_address_is_masked_wherever_the_json_carries_it(self):
        body = _sanitize_json_body(
            '{"quota":{"upgradeSubscriptionUri":'
            '"https://pay.example/u?Email=someone@corp.com&utm=cli"},'
            '"nested":["mail a@b.co"]}'
        )
        rendered = str(body)
        self.assertNotIn("someone@corp.com", rendered)
        self.assertNotIn("a@b.co", rendered)

    def test_an_email_query_parameter_is_masked_by_name(self):
        self.assertEqual(
            _sanitize_query("email=a@b.com&plan=pro"), "email=<UZE_MASKED>&plan=pro"
        )

    def test_a_bearer_token_is_still_masked(self):
        self.assertIn("<UZE_TEST_TOKEN>", _mask("Bearer ya29.abcdefghijklmnop"))


if __name__ == "__main__":
    unittest.main()
