#!/usr/bin/env python3
"""Suite 1: what each font of the catalog says about UZE's glyphs.

Reads outlines, not pixels, so it needs no terminal and answers the same
on every run. For every font and every icon a glyph set declares it
records presence, the advance the font reserves, the ink box relative to
the cell, and the optical size, then judges them by `contract.py`:

- a Nerd Fonts v3 build missing an icon fails;
- an icon whose ink leaves the slot fails *Contained*;
- a set whose icons spread beyond the tolerance fails *One size*.

`python3 conformance/rendering/metrics.py [--set nerd] [--out DIR]`
"""

import argparse
import json
import os
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path

from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import fonts as catalog  # noqa: E402
from nerd_set import MARKS  # noqa: E402

import contract  # noqa: E402

# The bundled glyph sets: the repository's, or wherever the Lab image was
# handed them.
THEMES = Path(
    os.environ.get(
        "UZE_LAB_THEMES", HERE.parent.parent / "crates" / "uze-theme" / "themes"
    )
)


def is_icon(glyph: str) -> bool:
    """A codepoint only a patched font supplies: the private-use areas."""
    return any(
        0xE000 <= ord(ch) <= 0xF8FF or 0xF0000 <= ord(ch) <= 0x10FFFD for ch in glyph
    )


def icons_of(set_name: str) -> dict[str, str]:
    """Every icon a glyph set declares, by symbol name."""
    symbols = json.loads((THEMES / f"{set_name}.json").read_text())["symbols"]
    icons = {}
    for name, value in symbols.items():
        if isinstance(value, dict):
            glyph = value.get("glyph") or "".join(value.get("frames", []))
        elif isinstance(value, list):
            glyph = "".join(value)
        else:
            glyph = value
        if glyph and is_icon(glyph):
            icons[name] = glyph
    return icons


@dataclass
class Metrics:
    """The outline facts that decide how a font draws one glyph."""

    advance: float  # cells
    ink: contract.Box  # cells, relative to the glyph's own cell


class FontReader:
    def __init__(self, path: Path):
        self.font = TTFont(path, lazy=True)
        self.cmap = self.font.getBestCmap()
        self.glyphs = self.font.getGlyphSet()
        self.hmtx = self.font["hmtx"]
        hhea = self.font["hhea"]
        self.ascent = hhea.ascent
        self.line = hhea.ascent - hhea.descent
        self.cell = self._cell_width()

    def _cell_width(self) -> float:
        """The advance the terminal's grid is built from: a letter's, or —
        in a symbols-only font, which has none — the advance most of its
        glyphs share."""
        if ord("M") in self.cmap:
            return self.hmtx[self.cmap[ord("M")]][0]
        advances = Counter(self.hmtx[name][0] for name in self.cmap.values())
        return advances.most_common(1)[0][0]

    @property
    def cell_aspect(self) -> float:
        return self.cell / self.line

    def measure(self, glyph: str) -> Metrics | None:
        point = ord(glyph[0])
        if point not in self.cmap:
            return None
        name = self.cmap[point]
        pen = BoundsPen(self.glyphs)
        self.glyphs[name].draw(pen)
        advance = self.hmtx[name][0] / self.cell
        if pen.bounds is None:
            return Metrics(advance, contract.Box(0, 0, 0, 0))
        x0, y0, x1, y1 = pen.bounds
        return Metrics(
            advance,
            contract.Box(
                x0 / self.cell,
                (self.ascent - y1) / self.line,
                x1 / self.cell,
                (self.ascent - y0) / self.line,
            ),
        )


def measure_font(font: catalog.Font, icons: dict[str, str]) -> dict:
    reader = FontReader(font.path)
    symbols = {}
    sizes = {}
    missing = []
    overflowing = []
    for name, glyph in sorted(icons.items()):
        metrics = reader.measure(glyph)
        if metrics is None:
            missing.append(name)
            continue
        size = contract.optical_size(metrics.ink, reader.cell_aspect)
        if name not in MARKS:
            sizes[name] = size
        fits = contract.contained(metrics.ink, contract.FONT_SLACK)
        if not fits:
            overflowing.append(name)
        symbols[name] = {
            "codepoint": f"U+{ord(glyph[0]):04X}",
            "advance": round(metrics.advance, 3),
            "ink": {k: round(v, 3) for k, v in asdict(metrics.ink).items()},
            "size": round(size, 3),
            "contained": fits,
            "mark": name in MARKS,
        }
    outliers = contract.outliers(sizes)
    for name, deviation in outliers.items():
        symbols[name]["deviation"] = round(deviation, 3)
    return {
        "font": font.id,
        "file": font.file,
        "cell_aspect": round(reader.cell_aspect, 3),
        "missing": missing,
        "overflowing": overflowing,
        "spread": round(contract.spread(sizes), 3),
        "outliers": sorted(outliers),
        "passes": not missing and not overflowing and not outliers,
        "symbols": symbols,
    }


def run(set_name: str, out: Path | None) -> list[dict]:
    icons = icons_of(set_name)
    reports = [measure_font(font, icons) for font in catalog.installed()]
    if out:
        out.mkdir(parents=True, exist_ok=True)
        (out / f"{set_name}.json").write_text(
            json.dumps(
                {"set": set_name, "tolerance": contract.TOLERANCE, "fonts": reports},
                indent=2,
            )
            + "\n"
        )
    return reports


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--set", default="nerd")
    parser.add_argument("--out", type=Path, default=None)
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()
    reports = run(args.set, args.out)
    failed = 0
    for report in reports:
        verdict = "pass" if report["passes"] else "FAIL"
        failed += not report["passes"]
        print(
            f"{verdict:4}  {report['font']:28} spread {report['spread']:.2f}"
            f"  missing {len(report['missing'])}  overflowing {len(report['overflowing'])}"
            f"  outliers {', '.join(report['outliers']) or '-'}"
        )
        if args.verbose:
            for name, symbol in report["symbols"].items():
                print(
                    f"        {name:24} size {symbol['size']:.2f}  ink {symbol['ink']}"
                )
    print(
        f"{len(reports) - failed}/{len(reports)} fonts pass at tolerance ±{contract.TOLERANCE:.0%}"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
