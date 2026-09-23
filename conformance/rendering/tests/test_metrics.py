#!/usr/bin/env python3
"""The fonts suite against a font whose answer is known: one built with
fontTools, carrying a letter to size the cell by and icons drawn exactly
as a good build, a wide build and a careless set would draw them.
`python3 conformance/rendering/tests/test_metrics.py`.
"""

import os
import sys
import tempfile
import unittest
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import fonts as catalog  # noqa: E402
from metrics import measure_font  # noqa: E402

CELL = 600
ASCENT, DESCENT = 800, -200


def rect(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()
    return pen.glyph()


def build(path: Path, icons: dict[int, tuple[int, int, int, int]]):
    names = [".notdef", "M"] + [f"icon{i}" for i in range(len(icons))]
    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder(names)
    cmap = {ord("M"): "M"}
    glyphs = {".notdef": rect(0, 0, 0, 0), "M": rect(50, 0, 550, 700)}
    for index, (point, box) in enumerate(icons.items()):
        cmap[point] = f"icon{index}"
        glyphs[f"icon{index}"] = rect(*box)
    builder.setupCharacterMap(cmap)
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({name: (CELL, 0) for name in names})
    builder.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT)
    builder.setupOS2()
    builder.setupPost()
    builder.setupNameTable({"familyName": "Synthetic", "styleName": "Regular"})
    builder.save(str(path))


SQUARE = (0, 100, 600, 700)  # one cell wide, 0.6 of the line tall


class MetricsTest(unittest.TestCase):
    def measure(self, icons, declared):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "synthetic.ttf"
            build(path, icons)
            return measure_font(
                catalog.Font("Synthetic", "mono", path.name, path), declared
            )

    def test_a_set_of_one_size_passes(self):
        report = self.measure(
            {0xE000: SQUARE, 0xE001: SQUARE, 0xE002: SQUARE},
            {"code": "", "architect": "", "map": ""},
        )
        self.assertTrue(report["passes"], report)

    def test_ink_beyond_the_gutter_is_named(self):
        report = self.measure(
            {0xE000: SQUARE, 0xE001: (0, 100, 1500, 700)},
            {"code": "", "architect": ""},
        )
        self.assertEqual(report["overflowing"], ["architect"])

    def test_ink_into_the_gutter_is_not(self):
        report = self.measure(
            {0xE000: SQUARE, 0xE001: (0, 100, 1000, 700)},
            {"code": "", "architect": ""},
        )
        self.assertEqual(report["overflowing"], [])

    def test_a_missing_icon_is_named(self):
        report = self.measure({0xE000: SQUARE}, {"code": "", "architect": ""})
        self.assertEqual(report["missing"], ["architect"])
        self.assertFalse(report["passes"])

    def test_an_icon_outside_the_band_is_named(self):
        report = self.measure(
            {
                0xE000: SQUARE,
                0xE001: SQUARE,
                0xE002: SQUARE,
                0xE003: (200, 300, 400, 500),
            },
            {"code": "", "architect": "", "map": "", "changes": ""},
        )
        self.assertEqual(report["outliers"], ["changes"])

    def test_a_mark_is_not_held_to_the_band(self):
        report = self.measure(
            {0xE000: SQUARE, 0xE001: SQUARE, 0xE002: (250, 350, 350, 450)},
            {"code": "", "architect": "", "mark.dot": ""},
        )
        self.assertEqual(report["outliers"], [])


if __name__ == "__main__":
    unittest.main()
