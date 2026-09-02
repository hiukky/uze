#!/usr/bin/env python3
"""Deterministic unit tests for Antigravity's signed-in provider plane.

The envelope the CloudCode path wraps a turn in, and the synthetic account
payloads served behind it — no docker, no harness binaries.
`python3 conformance/tests/test_signed_in.py`.
"""

import importlib.util
import io
import json
import os
import sys
import unittest

ROOT = os.path.join(os.path.dirname(__file__), "..")
sys.path.insert(0, ROOT)
# The provider runs with `shared/` mounted flat beside it in the container,
# so `capture` and `variation` are top-level there.
sys.path.insert(0, os.path.join(ROOT, "shared"))

import capture  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "antigravity_provider",
    os.path.join(ROOT, "harnesses", "antigravity", "provider.py"),
)
provider = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(provider)


class ConsumerEnvelopeTest(unittest.TestCase):
    """`{project, requestId, model, request:{…}}` in, `{"response": …}` out."""

    def test_the_wrapped_request_is_unwrapped_to_the_generate_content_body(self):
        wrapper = {
            "project": "synthetic",
            "requestId": "r-1",
            "model": "gemini-3.1-pro-preview",
            "userAgent": "antigravity/cli",
            "requestType": "agent",
            "request": {"contents": [{"role": "user", "parts": [{"text": "hi"}]}]},
        }
        unwrapped = json.loads(provider.unwrap_consumer_request(json.dumps(wrapper)))
        self.assertEqual(unwrapped, wrapper["request"])

    def test_an_unwrapped_body_passes_through(self):
        """API-key mode sends the GenerateContent body directly; the same
        decision path must see it unchanged."""
        body = json.dumps({"contents": [{"role": "user"}]})
        self.assertEqual(provider.unwrap_consumer_request(body), body)

    def test_a_body_that_is_not_json_passes_through(self):
        self.assertEqual(provider.unwrap_consumer_request("not json"), "not json")

    def test_every_event_is_re_framed_in_the_signed_in_envelope(self):
        payload = provider.sse({"candidates": [{"index": 0}]}) + provider.sse(
            {"candidates": [{"index": 1}]}
        )
        frames = [
            json.loads(frame[len(b"data: ") :])
            for frame in provider.wrap_consumer_stream(payload).split(b"\n\n")
            if frame.strip()
        ]
        self.assertEqual(len(frames), 2)
        for index, frame in enumerate(frames):
            self.assertEqual(frame["response"], {"candidates": [{"index": index}]})
            self.assertEqual(frame["traceId"], "synthetic")
            self.assertEqual(frame["metadata"], {})

    def test_the_framing_stays_the_one_the_variations_split_on(self):
        """`variation.emit` splits frames on a blank line; a re-framed stream
        that lost that delimiter would silently disable every adversarial
        variation in signed-in mode."""
        wrapped = provider.wrap_consumer_stream(provider.sse({"candidates": []}))
        self.assertTrue(wrapped.startswith(b"data: "))
        self.assertTrue(wrapped.endswith(b"\n\n"))
        self.assertNotIn(b"[DONE]", wrapped)

    def test_a_frame_that_is_not_an_event_is_dropped(self):
        self.assertEqual(provider.wrap_consumer_stream(b": keep-alive\n\n"), b"")


class DeclaredToolsTest(unittest.TestCase):
    def test_declarations_are_read_from_every_tools_entry(self):
        """Signed in, the harness sends one `tools` entry per tool. Reading
        only the first left the provider believing the turn declared a single
        tool, so the scripted call was never served."""
        body = json.dumps(
            {
                "contents": [],
                "tools": [
                    {"functionDeclarations": [{"name": "generate_image"}]},
                    {"functionDeclarations": [{"name": "run_command"}]},
                ],
            }
        )
        summary = provider.structural_summary(body)
        self.assertEqual(summary["tools"], ["generate_image", "run_command"])
        self.assertTrue(
            provider.wants_function_call(
                {"tools": [provider.FC_NAME], "has_function_response": False}
            )
        )
        self.assertFalse(
            provider.wants_function_call(
                {"tools": [provider.FC_NAME], "has_function_response": True}
            )
        )

    def test_a_request_without_tools_declares_none(self):
        self.assertEqual(provider.structural_summary("{}")["tools"], [])


class ChunkedBodyTest(unittest.TestCase):
    """The signed-in model request arrives `Transfer-Encoding: chunked`;
    reading it by Content-Length handed the provider an empty body — a turn
    that declared tools nobody saw."""

    class Handler:
        def __init__(self, headers, raw):
            self.headers = headers
            self.command = "POST"
            self.path = "/v1internal:streamGenerateContent"
            self.rfile = io.BytesIO(raw)

    def read(self, headers, raw):
        return capture.read_body(self.Handler(headers, raw))

    def test_a_chunked_body_is_reassembled(self):
        raw = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"
        self.assertEqual(
            self.read({"Transfer-Encoding": "chunked"}, raw), b"hello world"
        )

    def test_a_content_length_body_is_unaffected(self):
        self.assertEqual(self.read({"Content-Length": "5"}, b"helloignored"), b"hello")

    def test_a_truncated_chunked_body_stops_rather_than_blocking(self):
        """A provider that blocks on a malformed stream never answers, and
        the harness waits out its own timeout with nothing to read."""
        self.assertEqual(
            self.read({"Transfer-Encoding": "chunked"}, b"5\r\nhel"), b"hel"
        )


class SyntheticAccountTest(unittest.TestCase):
    """Nothing served on the signed-in plane may carry real account data."""

    def test_the_identity_is_the_labs_own_unresolvable_address(self):
        self.assertEqual(provider.USERINFO["email"], "conformance@uze.invalid")
        self.assertEqual(provider.USERINFO["id"], "synthetic")

    def test_every_served_account_value_is_synthetic(self):
        served = json.dumps(
            [
                provider.USERINFO,
                provider.REFRESHED_TOKEN,
                provider.FETCH_USER_INFO,
                provider.LOAD_CODE_ASSIST,
                provider.FETCH_AVAILABLE_MODELS,
            ]
        )
        for real in ("@gmail.com", "ya29.", "AIza", "googleusercontent.com/a/"):
            self.assertNotIn(real, served)

    def test_the_refresh_stub_answers_the_same_synthetic_token(self):
        self.assertEqual(
            provider.REFRESHED_TOKEN["access_token"], "synthetic-access-token"
        )
        self.assertEqual(provider.REFRESHED_TOKEN["token_type"], "Bearer")

    def test_the_token_fixture_is_a_far_dated_consumer_session(self):
        """A near expiry would send the CLI to a refresh the Lab should not
        have to model."""
        fixture = os.path.join(
            ROOT, "harnesses", "antigravity", "fixtures", "antigravity-oauth-token"
        )
        with open(fixture) as handle:
            token = json.load(handle)
        self.assertEqual(token["auth_method"], "consumer")
        self.assertEqual(token["token"]["access_token"], "synthetic-access-token")
        self.assertGreater(int(token["token"]["expiry"][:4]), 2090)

    def test_the_model_catalogue_names_the_two_models_a_turn_uses(self):
        """Signed in the CLI has no built-in catalogue: an id without a
        `model` enum resolves to nothing and every turn dies with "neither
        PlanModel nor RequestedModel specified"."""
        catalogue = provider.FETCH_AVAILABLE_MODELS
        self.assertIn(catalogue["defaultAgentModelId"], catalogue["models"])
        for details in catalogue["models"].values():
            self.assertTrue(details["model"].startswith("MODEL_"))
        for tier in catalogue["tieredModelIds"].values():
            self.assertIsInstance(tier, list)


if __name__ == "__main__":
    unittest.main()
