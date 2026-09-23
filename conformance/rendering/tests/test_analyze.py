#!/usr/bin/env python3
"""The analyzer against screenshots whose answer is known: synthetic
specimens drawn with Pillow, where each "glyph" is a rectangle placed
exactly where a terminal that got it right — or wrong in one known way —
would put its ink. `python3 conformance/rendering/tests/test_analyze.py`.
"""

import os
import sys
import unittest

from PIL import Image, ImageDraw

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from analyze import analyze  # noqa: E402

CELL_W, CELL_H = 10, 20
ORIGIN = (30, 40)
NAMES = ["a", "b", "c", "d", "e"]


def layout():
    return {
        "set": "synthetic",
        "calibration_row": 0,
        "calibration_cells": 40,
        "reference_row": 2,
        "slot_column": 0,
        "text_column": 2,
        "free_column": 8,
        "label_column": 16,
        "symbols": [
            {"name": name, "glyph": "", "icon": True, "width": 1, "row": 4 + 2 * i}
            for i, name in enumerate(NAMES)
        ],
    }


def cell(column, row):
    return ORIGIN[0] + column * CELL_W, ORIGIN[1] + row * CELL_H


def box(draw, column, row, x0, y0, x1, y1):
    left, top = cell(column, row)
    draw.rectangle(
        [
            left + x0 * CELL_W,
            top + y0 * CELL_H,
            left + x1 * CELL_W - 1,
            top + y1 * CELL_H - 1,
        ],
        fill="black",
    )


def specimen(glyphs, shift_x=None):
    """`glyphs[name] = (free_box, slot_box)` in cells; `shift_x[name]` moves
    that row's reference letter."""
    image = Image.new("RGB", (500, 400), "white")
    draw = ImageDraw.Draw(image)
    box(draw, 0, 0, 0, 0, 40, 1)  # calibration row
    box(draw, 2, 2, 0.2, 0.4, 0.8, 0.9)  # reference letter
    for i, name in enumerate(NAMES):
        row = 4 + 2 * i
        free, slot = glyphs.get(name, ((0.1, 0.2, 0.9, 0.8), None))
        box(draw, 8, row, *free)
        box(draw, 0, row, *(slot or free))
        dx = (shift_x or {}).get(name, 0)
        box(draw, 2, row, 0.2 + dx, 0.4, 0.8 + dx, 0.9)
    return image


class AnalyzeTest(unittest.TestCase):
    def test_a_well_drawn_specimen_passes(self):
        verdict = analyze(specimen({}), layout())
        self.assertTrue(verdict["passes"], verdict["criteria"])
        self.assertEqual(verdict["grid"]["cell_width"], CELL_W)
        self.assertEqual(verdict["grid"]["cell_height"], CELL_H)

    def test_an_icon_into_its_gutter_is_still_contained(self):
        wide = (0.0, 0.2, 1.7, 0.8)
        verdict = analyze(specimen({"b": (wide, (0.0, 0.2, 1.7, 0.8))}), layout())
        self.assertTrue(verdict["symbols"]["b"]["contained"])

    def test_an_icon_past_its_gutter_fails_contained(self):
        verdict = analyze(
            specimen({"b": ((0.0, 0.2, 2.6, 0.8), (0.0, 0.2, 2.0, 0.8))}), layout()
        )
        self.assertIn("b", verdict["criteria"]["contained"])

    def test_an_icon_out_of_its_row_fails_contained(self):
        verdict = analyze(
            specimen({"c": ((0.1, -0.3, 0.9, 0.8), (0.1, -0.3, 0.9, 0.8))}), layout()
        )
        self.assertIn("c", verdict["criteria"]["contained"])

    def test_a_clipped_icon_fails_whole(self):
        verdict = analyze(
            specimen({"d": ((0.0, 0.2, 1.6, 0.8), (0.0, 0.2, 1.0, 0.8))}), layout()
        )
        self.assertIn("d", verdict["criteria"]["whole"])
        self.assertNotIn("d", verdict["criteria"]["contained"])

    def test_a_small_icon_among_large_ones_fails_one_size(self):
        tiny = (0.4, 0.45, 0.6, 0.55)
        verdict = analyze(specimen({"e": (tiny, tiny)}), layout())
        self.assertEqual(verdict["criteria"]["one_size"], ["e"])

    def test_text_that_moved_fails_aligned(self):
        verdict = analyze(specimen({}, shift_x={"a": 0.5}), layout())
        self.assertEqual(verdict["criteria"]["aligned"], ["a"])

    def test_the_grid_survives_window_chrome(self):
        framed = Image.new("RGB", (560, 480), "white")
        ImageDraw.Draw(framed).rectangle([0, 0, 559, 20], fill="#dddddd")
        framed.paste(specimen({}), (25, 60))
        self.assertTrue(analyze(framed, layout())["passes"])


if __name__ == "__main__":
    unittest.main()
