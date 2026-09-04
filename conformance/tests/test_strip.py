#!/usr/bin/env python3
"""Deterministic unit tests for `ansi_strip` — the state-machine plain-text
view every harness's screen scraping uses. Samples come from real codex
0.150.1 renders (keystroke echoes with `\x1b[0 q`, spinner lines with OSC 8
hyperlinks terminated by ST). `python3 conformance/tests/test_strip.py`.
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared.common import ansi_strip, squash


class StripTest(unittest.TestCase):
    def test_keystroke_echo_with_cursor_style_resets(self):
        raw = "r\x1b[0 qu\x1b[0 qn\x1b[0 q"
        self.assertEqual(ansi_strip(raw), "run")

    def test_ghostty_osc8_hyperlink_with_st_terminator(self):
        # codex renders spinner lines with OSC 8 + ST (ESC \); a BEL-only
        # regex left the URL content interleaved with the letters.
        raw = "W\x1b]8;;https://x.test\x1b\\o\x1b]8;;\x1b\\rk"
        self.assertEqual(ansi_strip(raw), "Work")

    def test_real_corrupted_working_sample_restores_contiguous_marker(self):
        # The exact v5 denial-phase failure: "Working" corrupted to
        # g•8W◦WoorrkkiinWng with interleaved garbage.
        raw = "g\x1b]8;;a\x1b\\\x1b[0m•\x1b[0 q8W\x1b[38;2;1;2;3m◦\x1b[39mW\x1b[0 qoo"
        self.assertEqual(ansi_strip(raw), "g•8W◦Woo")

    def test_csi_colors_and_cursor_moves(self):
        raw = "\x1b[38;2;246;226;183;49m›\x1b[39m \x1b[11;9Hrun"
        self.assertEqual(ansi_strip(raw), "› run")

    def test_lone_esc_and_charset_selection(self):
        self.assertEqual(ansi_strip("a\x1b(0b\x1b"), "ab")

    def test_shift_like_bracket_paste_sequences(self):
        raw = "hi\x1b[?2026l\x1b[?2026h"
        self.assertEqual(ansi_strip(raw), "hi")

    def test_plain_text_passes_through(self):
        self.assertEqual(ansi_strip("UZE_CONFORMANCE_OK"), "UZE_CONFORMANCE_OK")


class SquashTest(unittest.TestCase):
    """A harness that spaces its words with cursor-forward moves leaves the
    transcript's words run together; a marker only matches once both sides
    drop the spacing. Sample: Claude Code 2.1.260's folder-trust screen,
    whose unrecognized text cost the vertical all 24 checks."""

    def test_cursor_forward_spacing_leaves_words_joined(self):
        raw = "Yes,[1CI[1Ctrust[1Cthis[1Cfolder"
        self.assertEqual(ansi_strip(raw), "Yes,Itrustthisfolder")
        self.assertIn(squash("Yes, I trust this folder"), squash(ansi_strip(raw)))

    def test_squash_drops_newlines_and_runs_of_spaces(self):
        self.assertEqual(squash("  No,\n exit  "), "No,exit")

    def test_squash_is_idempotent(self):
        self.assertEqual(squash(squash("Quick safety check")), "Quicksafetycheck")


if __name__ == "__main__":
    unittest.main()
