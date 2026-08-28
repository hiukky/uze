#!/usr/bin/env python3
"""Deterministic unit tests for the compatibility matrix (overlay + report).

No docker, no harness binaries — the overlay builder and the report
rendering are pure filesystem/string logic.
`python3 conformance/tests/test_matrix.py`.
"""

import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from matrix import build_variant_market, load_variants, render_table


class Config:
    repo = os.path.join(os.path.dirname(__file__), "..")


class Market:
    """A tiny canonical-market stand-in with the shape matrix copies."""

    def __init__(self, root):
        self.plugins = os.path.join(root, "plugins")
        os.makedirs(os.path.join(self.plugins, "hook-plugin"), exist_ok=True)
        self.hooks = os.path.join(self.plugins, "hook-plugin", "hooks.json")
        self.plugin_json = os.path.join(self.plugins, "hook-plugin", "plugin.json")
        with open(self.hooks, "w") as f:
            json.dump({"hooks": {"PreToolUse": []}}, f)
        with open(self.plugin_json, "w") as f:
            json.dump({"name": "hook-plugin"}, f)


class VariantTest(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.canonical = os.path.join(self.root, "canonical-market")
        Market(self.canonical)

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def test_overlay_replaces_content(self):
        variant = {
            "id": "deny-hook",
            "overlay": {
                "plugins/hook-plugin/plugin.json": {
                    "name": "hook-plugin",
                    "extra": True,
                }
            },
        }
        out = build_variant_market(Config(), variant, os.path.join(self.root, "out"))
        with open(os.path.join(out, "plugins/hook-plugin/plugin.json")) as f:
            content = json.load(f)
        self.assertEqual(content["extra"], True)
        # The canonical market is untouched by the overlay build.
        with open(os.path.join(self.canonical, "plugins/hook-plugin/plugin.json")) as f:
            self.assertNotIn("extra", json.load(f))

    def test_overlay_delete_removes_file(self):
        variant = {
            "id": "no-hooks",
            "overlay": {"plugins/hook-plugin/hooks.json": None},
        }
        out = build_variant_market(Config(), variant, os.path.join(self.root, "out"))
        self.assertFalse(
            os.path.exists(os.path.join(out, "plugins/hook-plugin/hooks.json"))
        )

    def test_content_overlay_writes_text(self):
        variant = {"id": "notes", "overlay": {"plugins/hook-plugin/NOTES.md": "varied"}}
        out = build_variant_market(Config(), variant, os.path.join(self.root, "out"))
        with open(os.path.join(out, "plugins/hook-plugin/NOTES.md")) as f:
            self.assertEqual(f.read(), "varied")


class ManifestTest(unittest.TestCase):
    def test_load_validates_variants(self):
        path = os.path.join(tempfile.mkdtemp(), "variants.json")
        with open(path, "w") as f:
            json.dump({"variants": [{"id": "a", "overlay": {}}]}, f)
        self.assertEqual(load_variants(path)["variants"][0]["id"], "a")

    def test_missing_variants_fails(self):
        path = os.path.join(tempfile.mkdtemp(), "variants.json")
        with open(path, "w") as f:
            json.dump({"variants": []}, f)
        with self.assertRaises(RuntimeError):
            load_variants(path)


class ReportTest(unittest.TestCase):
    def test_render_table_covers_all_cells(self):
        cells = [
            {
                "harness": "claude",
                "variant": "a",
                "passed": 18,
                "total": 18,
                "known_adapted": 0,
                "crash": None,
                "failures": [],
            },
            {
                "harness": "opencode",
                "variant": "a",
                "passed": 22,
                "total": 28,
                "known_adapted": 6,
                "crash": None,
                "failures": [{"check": "x", "suite": "s", "adjudication": "escalated"}],
            },
            {
                "harness": "claude",
                "variant": "b",
                "passed": 0,
                "total": 18,
                "known_adapted": 0,
                "crash": "RuntimeError: boom",
                "failures": [],
            },
        ]
        table = render_table(cells)
        self.assertIn("claude", table)
        self.assertIn("18/18 ✅", table)
        self.assertIn("22/28 🟡", table)
        self.assertIn("CRASH", table)


if __name__ == "__main__":
    unittest.main()
