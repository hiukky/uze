#!/usr/bin/env python3
"""Deterministic unit tests for version provenance (ADR-035).

Covers the probe fallback contract (a probe that cannot run records
`unknown`, never a crash) and the manifest drift event computation
(harness version change vs. the previous committed summary is explicit).
Docker-invoking paths are mocked out. `python3 conformance/tests/test_provenance.py`.
"""

import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared import common


class Config:
    harness = "claude"
    repo = os.path.join(os.path.dirname(__file__), "..")
    outdir = "/tmp/uze-provenance-test"


class ProbeFallbackTest(unittest.TestCase):
    def test_probe_failure_records_unknown_never_crashes(self):
        # docker unavailable / probe hangs / no output: always `unknown`.
        with (
            mock.patch("subprocess.run", side_effect=Exception("no docker")),
            mock.patch("shared.common.CURRENT_HARNESS", "claude"),
        ):
            self.assertEqual(common.probe_harness_version(Config()), "unknown")

    def test_probe_first_line_is_the_version(self):
        fake = mock.Mock()
        fake.stdout = "2.1.239 (Claude Code)\n"
        fake.stderr = ""
        fake.returncode = 0
        with mock.patch("subprocess.run", return_value=fake):
            self.assertEqual(
                common.probe_harness_version(Config()), "2.1.239 (Claude Code)"
            )

    def test_unknown_harness_has_no_probe(self):
        cfg = Config()
        cfg.harness = "nonsense"
        with mock.patch("subprocess.run") as run:
            self.assertEqual(common.probe_harness_version(cfg), "unknown")
            run.assert_not_called()


class ManifestDriftTest(unittest.TestCase):
    def test_version_change_is_an_explicit_event(self):
        with (
            mock.patch("shared.common.previous_harness_version", return_value="1.1.19"),
            mock.patch("shared.common.uze_version", return_value="x"),
            mock.patch("shared.common.repo_revision", return_value="x"),
            mock.patch("shared.common.image_id", return_value="x"),
        ):
            manifest = common.run_manifest(
                Config(), "1.2.0", "2026-08-27T00:00:00+0000"
            )
        self.assertEqual(manifest["version_drift"], {"from": "1.1.19", "to": "1.2.0"})

    def test_same_version_has_no_drift_event(self):
        with (
            mock.patch("shared.common.previous_harness_version", return_value="1.1.19"),
            mock.patch("shared.common.uze_version", return_value="x"),
            mock.patch("shared.common.repo_revision", return_value="x"),
            mock.patch("shared.common.image_id", return_value="x"),
        ):
            manifest = common.run_manifest(
                Config(), "1.1.19", "2026-08-27T00:00:00+0000"
            )
        self.assertIsNone(manifest["version_drift"])

    def test_unprobed_version_never_fabricates_drift(self):
        with (
            mock.patch("shared.common.previous_harness_version", return_value="1.1.19"),
            mock.patch("shared.common.uze_version", return_value="x"),
            mock.patch("shared.common.repo_revision", return_value="x"),
            mock.patch("shared.common.image_id", return_value="x"),
        ):
            manifest = common.run_manifest(
                Config(), "unknown", "2026-08-27T00:00:00+0000"
            )
        self.assertIsNone(manifest["version_drift"])


if __name__ == "__main__":
    unittest.main()
