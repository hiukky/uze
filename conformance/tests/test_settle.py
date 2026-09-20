#!/usr/bin/env python3
"""Deterministic unit tests for `settle_and_quiet` — the settled-absence
contract of ADR-035. Quiet is a property of the rendered screen, not of
bytes arriving: a finished Claude Code turn keeps writing bare cursor
motion to hold the caret, and a byte-counting window never closes on it.
`python3 conformance/tests/test_settle.py`.
"""

import itertools
import os
import sys
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from shared import common
from shared.common import settle_and_quiet


def feeding(chunks, endless=False):
    """A `screen`-shaped callable that hands back one chunk per call.

    It sleeps like the real one: `screen(wait)` blocks for up to `wait`,
    so a test's budget measures the same thing the Lab's does. `endless`
    cycles the chunks forever, for the streams that must never go quiet.
    """
    queue = itertools.cycle(chunks) if endless else iter(list(chunks))

    def screen(wait=0.5):
        time.sleep(wait)
        return (next(queue, ""), "")

    return screen


class SettleTest(unittest.TestCase):
    def test_silence_settles(self):
        self.assertTrue(settle_and_quiet(feeding([""]), quiet=0.01, budget=2.0))

    def test_bare_cursor_motion_is_not_activity(self):
        # The exact tail of the 2.1.278 `hooks > order` capture: the turn
        # is done, the prompt is back, and the TUI holds the caret with
        # cursor moves alone. This is the case that failed the gate.
        caret = "\x1b[2C\x1b[3A\x1b[2D\x1b[3B"
        self.assertTrue(
            settle_and_quiet(feeding([caret], endless=True), quiet=0.01, budget=2.0)
        )

    def test_printable_output_is_activity(self):
        # A stream that keeps adding glyphs never goes quiet, so an
        # absence cannot be concluded on it.
        self.assertFalse(
            settle_and_quiet(feeding(["x"], endless=True), quiet=0.3, budget=1.2)
        )

    def test_an_erase_that_blanks_a_line_is_activity(self):
        # A spinner clearing itself writes no glyph but changes what is
        # shown, so it must still count.
        self.assertFalse(
            settle_and_quiet(
                feeding(["hello\r", "\x1b[K"], endless=True),
                quiet=0.3,
                budget=1.2,
            )
        )

    def test_quiet_after_output_stops(self):
        self.assertTrue(
            settle_and_quiet(feeding(["rendering", "more", ""]), quiet=0.01, budget=2.0)
        )

    def test_a_wall_clock_step_cannot_fail_a_settled_turn(self):
        """The failure this contract must never produce.

        A WSL guest re-syncing with its Windows host stepped `time.time()`
        mid-window. The budget then expired on its first comparison, the
        loop never ran, and two absence checks failed on a turn that had
        settled correctly. Durations here are monotonic, so a wall clock
        that steps and stays stepped changes nothing.
        """
        real = time.time
        calls = [0]

        def stepped():
            calls[0] += 1
            # Re-sync after the window opens, then stay stepped.
            return real() + (0 if calls[0] < 2 else 60)

        common.time.time = stepped
        try:
            self.assertTrue(settle_and_quiet(feeding([""]), quiet=0.01, budget=2.0))
        finally:
            common.time.time = real


if __name__ == "__main__":
    unittest.main()
