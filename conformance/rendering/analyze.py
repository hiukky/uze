#!/usr/bin/env python3
"""Pixels -> per-symbol measurements -> a verdict, for one screenshot of
`uze theme specimen`.

The Lab calls this on every capture, and so can anyone with a screenshot
from a terminal the Lab cannot run:

    uze theme specimen nerd --format json > layout.json
    # screenshot the terminal showing `uze theme specimen nerd`
    python3 conformance/rendering/analyze.py shot.png --layout layout.json \\
        --terminal "Windows Terminal 1.23" --font "JetBrainsMono Nerd Font Mono"

The screenshot is read against the specimen's own calibration row, a run of
full blocks, so window chrome, a crop or a scale factor around it does not
matter. Ink is whatever differs from the terminal's background, which the
specimen leaves unpainted.
"""

import argparse
import json
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path

from PIL import Image, ImageChops

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from nerd_set import MARKS  # noqa: E402

import contract  # noqa: E402

# How far a pixel must sit from the background to count as ink, on a
# 0–255 scale per channel. Anti-aliased fringes below it are not a glyph's
# extent; a glyph's body is far above it on a dark-on-light specimen.
INK_THRESHOLD = 96


@dataclass(frozen=True)
class Grid:
    """Where the specimen's cells are in the image, in pixels."""

    x0: float
    y0: float
    cell_width: float
    cell_height: float

    @property
    def aspect(self) -> float:
        return self.cell_width / self.cell_height

    def left(self, column: float) -> float:
        return self.x0 + column * self.cell_width

    def top(self, row: float) -> float:
        return self.y0 + row * self.cell_height


class Screen:
    """A screenshot as a mask of ink: every pixel far enough from the
    background, whose colour is the most common one on the screen."""

    def __init__(
        self, image: Image.Image, background: tuple[int, int, int] | None = None
    ):
        image = image.convert("RGB")
        self.width, self.height = image.size
        background = background or max(image.getcolors(self.width * self.height))[1]
        distance = ImageChops.difference(
            image, Image.new("RGB", image.size, background)
        )
        channels = distance.split()
        strongest = ImageChops.lighter(
            ImageChops.lighter(channels[0], channels[1]), channels[2]
        )
        self.mask = strongest.point(
            lambda value: 255 if value >= INK_THRESHOLD else 0
        ).tobytes()

    def row(self, y: int) -> bytes:
        return self.mask[y * self.width : (y + 1) * self.width]

    def calibrate(self, cells: int) -> Grid:
        """The grid, from the calibration row of full blocks: the band of
        rows with the most ink on the screen.

        Measured by how much of each row is inked rather than by one
        unbroken run, because a terminal that draws the block from its
        font rather than filling the cell leaves hairline gaps between
        blocks."""
        counts = [self.row(y).count(255) for y in range(self.height)]
        widest = max(counts)
        if widest < cells * 3:
            raise SystemExit(
                "no calibration row found: is this a screenshot of the specimen?"
            )
        peak = counts.index(widest)
        top = peak
        while top > 0 and counts[top - 1] >= widest * 0.5:
            top -= 1
        bottom = peak
        while bottom + 1 < self.height and counts[bottom + 1] >= widest * 0.5:
            bottom += 1
        lefts = Counter(self.row(y).find(255) for y in range(top, bottom + 1))
        rights = Counter(self.row(y).rfind(255) + 1 for y in range(top, bottom + 1))
        x0 = lefts.most_common(1)[0][0]
        x1 = rights.most_common(1)[0][0]
        return Grid(x0, top, (x1 - x0) / cells, bottom - top + 1)

    def pitch(self, grid: Grid, column: int, rows: list[int]) -> Grid:
        """The grid with its line height measured from the text itself.

        The calibration row's height is the block glyph's, which a terminal
        drawing blocks from the font leaves short of the line. Over ninety
        rows that error adds up to whole lines. So the pitch comes from where
        the labels at `column` sit, fitted over every labelled row. Row 0's
        centre stays where the calibration row's centre is."""
        left = int(round(grid.left(column)))
        right = min(self.width, int(round(grid.left(column + 8))))
        start = int(grid.y0 + grid.cell_height)
        bands, inside = [], None
        for y in range(start, self.height):
            inked = self.row(y)[left:right].find(255) >= 0
            if inked and inside is None:
                inside = y
            elif not inked and inside is not None:
                bands.append((inside + y) / 2)
                inside = None
        if len(bands) != len(rows) or len(rows) < 2:
            return grid
        mean_row = sum(rows) / len(rows)
        mean_y = sum(bands) / len(bands)
        pitch = sum((r - mean_row) * (y - mean_y) for r, y in zip(rows, bands)) / sum(
            (r - mean_row) ** 2 for r in rows
        )
        centre = grid.y0 + grid.cell_height / 2
        return Grid(grid.x0, centre - pitch / 2, grid.cell_width, pitch)

    def ink_box(
        self, grid: Grid, row: int, from_column: float, to_column: float, origin: float
    ):
        """The ink inside a region, in cells relative to (`origin`, `row`).

        The region reaches half a row above and below, into the blank rows
        the specimen leaves, so ink that leaves its row is still seen."""
        left = max(0, int(round(grid.left(from_column))))
        right = min(self.width, int(round(grid.left(to_column))))
        top = max(0, int(round(grid.top(row - 0.5))))
        bottom = min(self.height, int(round(grid.top(row + 1.5))))
        xs0, xs1, ys = [], [], []
        for y in range(top, bottom):
            span = self.row(y)[left:right]
            first = span.find(255)
            if first < 0:
                continue
            xs0.append(left + first)
            xs1.append(left + span.rfind(255) + 1)
            ys.append(y)
        if not ys:
            return None
        return contract.Box(
            (min(xs0) - grid.left(origin)) / grid.cell_width,
            (min(ys) - grid.top(row)) / grid.cell_height,
            (max(xs1) - grid.left(origin)) / grid.cell_width,
            (max(ys) + 1 - grid.top(row)) / grid.cell_height,
        )


def analyze(image: Image.Image, layout: dict, background=None) -> dict:
    screen = Screen(image, background)
    grid = screen.calibrate(layout["calibration_cells"])
    grid = screen.pitch(
        grid, layout["label_column"], [entry["row"] for entry in layout["symbols"]]
    )
    slack = 1.5 / grid.cell_width  # a pixel and a half of anti-aliasing
    text, free, label = (
        layout["text_column"],
        layout["free_column"],
        layout["label_column"],
    )
    reference = screen.ink_box(grid, layout["reference_row"], text, text + 1, text)
    if reference is None:
        raise SystemExit("the reference letter is not on the screenshot")

    symbols = {}
    sizes = {}
    for entry in layout["symbols"]:
        row = entry["row"]
        in_free = screen.ink_box(grid, row, free - 1, label - 1, free)
        in_slot = screen.ink_box(
            grid, row, layout["slot_column"], text, layout["slot_column"]
        )
        after = screen.ink_box(grid, row, text, text + 1, text)
        if in_free is None or in_slot is None:
            symbols[entry["name"]] = {"drawn": False, "passes": False}
            continue
        size = contract.optical_size(in_free, grid.aspect)
        judged = {
            "contained": contract.contained(in_free, slack),
            "whole": contract.whole(in_slot, in_free, slack),
            "aligned": after is not None
            and contract.aligned(after.x0, reference.x0, slack),
        }
        if entry["icon"] and entry["name"] not in MARKS:
            sizes[entry["name"]] = size
        symbols[entry["name"]] = {
            "drawn": True,
            "icon": entry["icon"],
            "ink": {k: round(v, 3) for k, v in asdict(in_free).items()},
            "slot_ink": {k: round(v, 3) for k, v in asdict(in_slot).items()},
            "size": round(size, 3),
            **judged,
        }
    # A glyph rasterised to whole pixels can be a pixel off either way, so
    # One size allows the fraction of the median icon that one pixel is.
    median_px = (
        sorted(sizes.values())[len(sizes) // 2] * grid.cell_height if sizes else 1
    )
    outliers = contract.outliers(sizes, contract.TOLERANCE + 1 / max(median_px, 1))
    for name, symbol in symbols.items():
        if name in outliers:
            symbol["deviation"] = round(outliers[name], 3)
        symbol["one_size"] = name not in outliers
        symbol["passes"] = bool(
            symbol.get("drawn")
            and symbol["contained"]
            and symbol["whole"]
            and symbol["aligned"]
            and symbol["one_size"]
        )
    icon_sizes = sorted(sizes.values())
    return {
        "set": layout["set"],
        "grid": {k: round(v, 3) for k, v in asdict(grid).items()},
        "icon_size": round(icon_sizes[len(icon_sizes) // 2], 3) if icon_sizes else None,
        "spread": round(contract.spread(sizes), 3),
        "criteria": {
            criterion: sorted(
                n for n, s in symbols.items() if not s.get(criterion, False)
            )
            for criterion in ("drawn", "contained", "whole", "aligned", "one_size")
        },
        "passes": all(symbol["passes"] for symbol in symbols.values()),
        "symbols": symbols,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("screenshot", type=Path)
    parser.add_argument(
        "--layout", type=Path, required=True, help="`uze theme specimen --format json`"
    )
    parser.add_argument(
        "--terminal", required=True, help="the terminal and its version"
    )
    parser.add_argument(
        "--font", required=True, help="the font family, as the terminal was given it"
    )
    parser.add_argument("--out", type=Path, help="write the verdict here as JSON")
    parser.add_argument(
        "--background",
        help="the terminal's background as rrggbb, when it is not the most common colour "
        "in the screenshot (a window smaller than the screen around it)",
    )
    args = parser.parse_args()
    background = (
        tuple(int(args.background[i : i + 2], 16) for i in (0, 2, 4))
        if args.background
        else None
    )
    verdict = analyze(
        Image.open(args.screenshot), json.loads(args.layout.read_text()), background
    )
    verdict = {"terminal": args.terminal, "font": args.font, "manual": True, **verdict}
    if args.out:
        args.out.write_text(json.dumps(verdict, indent=2) + "\n")
    failing = {k: v for k, v in verdict["criteria"].items() if v}
    print(f"{'pass' if verdict['passes'] else 'FAIL'}  {args.terminal} / {args.font}")
    for criterion, names in failing.items():
        print(f"  {criterion}: {', '.join(names)}")
    return 0 if verdict["passes"] else 1


if __name__ == "__main__":
    sys.exit(main())
