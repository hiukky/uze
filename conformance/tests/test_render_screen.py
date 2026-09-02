#!/usr/bin/env python3
"""Deterministic unit tests for the TUI screen reconstruction.

Pure text-grid semantics — no docker, no harness binaries.
`python3 conformance/tests/test_render_screen.py`.
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.common import ansi_strip, render_screen


class RenderScreenTest(unittest.TestCase):
    def test_a_line_continued_by_cursor_motion_is_rejoined(self):
        """The regression this exists for: AGY streams its answer, then
        continues the same line with `cursor up` + `cursor forward`, so the
        string a person reads never appears contiguously in the bytes."""
        # Trimmed from a real recording (agy 1.1.24): the answer's first
        # half, three lines of chrome, then `cursor up 4` + `cursor forward
        # 14` and the rest of the word.
        raw = (
            "  UZE_CONFORMA\x1b[K\r\n"
            "⣿  \x1b[94mRunning command...\x1b[m\r\n"
            "\n\x1b[94m>\x1b[m\x1b[K\r\n"
            "\x1b[4A\x1b[14CNCE_PASS\r\n"
        )
        self.assertNotIn("UZE_CONFORMANCE_PASS", ansi_strip(raw))
        self.assertIn("UZE_CONFORMANCE_PASS", render_screen(raw))

    def test_a_carriage_return_overwrites_rather_than_appends(self):
        self.assertIn("world", render_screen("hello\rworld"))
        self.assertNotIn("hello", render_screen("hello\rworld"))

    def test_erase_to_end_of_line_clears_what_it_covers(self):
        self.assertNotIn("stale", render_screen("stale text\r\x1b[K"))

    def test_absolute_positioning_places_text_on_its_row(self):
        screen = render_screen("\x1b[2;1Hsecond\x1b[1;1Hfirst").split("\n")
        self.assertEqual(screen[0], "first")
        self.assertEqual(screen[1], "second")

    def test_non_csi_escapes_place_nothing(self):
        """OSC hyperlinks (codex spinner lines) and charset selection must
        vanish, exactly as `ansi_strip` drops them."""
        self.assertEqual(render_screen("\x1b]8;;http://x\x07link\x1b]8;;\x07"), "link")
        self.assertEqual(render_screen("\x1b(Bplain"), "plain")

    def test_an_empty_stream_renders_an_empty_screen(self):
        self.assertEqual(render_screen(""), "")


if __name__ == "__main__":
    unittest.main()
