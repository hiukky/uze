#!/usr/bin/env python3
"""Deterministic unit tests for the adaptive-result gate (ADR-035).

Run without the Lab: no docker, no harness binaries — pure registry and
adjudication semantics. `python3 conformance/tests/test_gate.py`.
"""

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import gate

ALLOW_ENTRY = {
    "harness": "codex",
    "check": "hooks-allow-approval-gate",
    "suite": "hooks > allow",
    "reason": "approval gate",
    "versions": ["*"],
    "observed_at": "2026-08-26",
}


def registry_file(entries):
    fd, path = tempfile.mkstemp(suffix=".json")
    with open(fd, "w") as f:
        json.dump({"adaptive": entries}, f)
    return path


def verdict(name, kind="assert", ok=True, harness="codex", version="0.150.0"):
    return {
        "check": name,
        "suite": "hooks > allow",
        "pass": ok,
        "detail": "",
        "kind": kind,
        "harness": harness,
        "harness_version": version,
    }


class LoadRegistryTest(unittest.TestCase):
    def test_missing_registry_is_an_error_not_an_empty_gate(self):
        with self.assertRaises(OSError):
            gate.load_registry("/nonexistent/expected.json")

    def test_parses_entries_into_harness_check_map(self):
        path = registry_file([ALLOW_ENTRY])
        loaded = gate.load_registry(path)
        self.assertIn(("codex", "hooks-allow-approval-gate"), loaded)


class EvaluateTest(unittest.TestCase):
    def setUp(self):
        self.registry = gate.load_registry(registry_file([ALLOW_ENTRY]))

    def test_unregistered_adapt_fails(self):
        results = gate.evaluate(
            "codex", [verdict("mystery-adapt", kind="adapted")], self.registry
        )
        self.assertFalse(results[0]["pass"])
        self.assertEqual(results[0]["gate"]["adjudication"], "unregistered_adapt")

    def test_registered_adapt_passes_as_known(self):
        results = gate.evaluate(
            "codex",
            [verdict("hooks-allow-approval-gate", kind="adapted")],
            self.registry,
        )
        self.assertTrue(results[0]["pass"])
        self.assertEqual(results[0]["gate"]["adjudication"], "known_adapt")

    def test_registered_adapt_now_passing_escalates(self):
        # The scenario was promoted (assert) while the entry still exists.
        results = gate.evaluate(
            "codex", [verdict("hooks-allow-approval-gate")], self.registry
        )
        self.assertFalse(results[0]["pass"])
        self.assertEqual(results[0]["gate"]["adjudication"], "escalated")

    def test_plain_assert_is_untouched(self):
        results = gate.evaluate(
            "codex", [verdict("plain-check", ok=False)], self.registry
        )
        self.assertFalse(results[0]["pass"])
        self.assertEqual(results[0]["gate"]["adjudication"], "asserted")

    def test_other_harness_entry_never_matches(self):
        # An antigravity-only registration must not bless a codex adapt.
        entry = {**ALLOW_ENTRY, "harness": "antigravity"}
        registry = gate.load_registry(registry_file([entry]))
        results = gate.evaluate(
            "codex", [verdict("hooks-allow-approval-gate", kind="adapted")], registry
        )
        self.assertEqual(results[0]["gate"]["adjudication"], "unregistered_adapt")


class VersionCoverageTest(unittest.TestCase):
    def test_wildcard_covers_any_version(self):
        self.assertTrue(gate.covers_version(ALLOW_ENTRY, "9.9.9"))

    def test_exact_list_covers_and_rejects(self):
        entry = {**ALLOW_ENTRY, "versions": ["0.150.0", "0.151.0"]}
        self.assertTrue(gate.covers_version(entry, "0.151.0"))
        self.assertFalse(gate.covers_version(entry, "0.152.0"))

    def test_unprobed_version_counts_as_covered(self):
        # The manifest records the probe failure; the gate cannot fail a
        # record it cannot verify.
        self.assertTrue(gate.covers_version(ALLOW_ENTRY, None))
        self.assertTrue(gate.covers_version(ALLOW_ENTRY, ""))

    def test_version_mismatch_fails_with_drift(self):
        entry = {**ALLOW_ENTRY, "versions": ["0.150.0"]}
        registry = gate.load_registry(registry_file([entry]))
        results = gate.evaluate(
            "codex",
            [verdict("hooks-allow-approval-gate", kind="adapted", version="0.152.0")],
            registry,
        )
        self.assertFalse(results[0]["pass"])
        self.assertEqual(results[0]["gate"]["adjudication"], "version_drift")
        self.assertIn("0.152.0", results[0]["gate"]["reason"])


if __name__ == "__main__":
    unittest.main()
