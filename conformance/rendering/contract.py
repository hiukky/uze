"""What "an icon draws correctly" means, for every terminal and every font.

Stated once and naming no terminal, no font and no vendor: the fonts suite
judges outlines by it and the terminals suite judges pixels by it, so a
verdict means the same thing whichever of them produced it.

A measurement is a `Box` of ink in *cells* — the one unit both sources
share. The fonts suite converts font units to cells with the font's own
advance; the analyzer converts pixels to cells with the calibration row.
"""

from dataclasses import dataclass
from statistics import median

# The widest band "one size" may ever be widened to, fixed before any
# baseline was measured so the tolerance cannot be tuned until a set passes.
TOLERANCE_CAP = 0.15

# The band the bundled `nerd` set is held to: the tightest one the
# regenerated set meets in every catalog build, never wider than the cap.
# Chosen from the measurement committed in `evidence/metrics/`; see
# `generate_nerd.py` for how the set was picked to meet it.
TOLERANCE = 0.13

# Cells an icon may use: the glyph's own, and the gutter UZE reserves.
SLOT_CELLS = 2

# Anti-aliasing and hinting move an edge by up to a pixel; outlines are
# exact. Both sources pass their own slack.
FONT_SLACK = 0.02


@dataclass(frozen=True)
class Box:
    """Ink bounds relative to the icon's own cell: x from its left edge,
    y from its top, both in cells."""

    x0: float
    y0: float
    x1: float
    y1: float

    @property
    def width(self) -> float:
        return max(0.0, self.x1 - self.x0)

    @property
    def height(self) -> float:
        return max(0.0, self.y1 - self.y0)


def optical_size(ink: Box, cell_aspect: float) -> float:
    """The geometric mean of the ink's sides, in cell *heights*.

    An area, not a side, because that is what an eye compares: a portrait
    file icon and a wide check mark with the same longest side do not look
    the same size, and the same area does. In cell heights, because a cell
    is taller than it is wide and measuring in widths would count a wide
    icon twice over. `cell_aspect` is the cell's width divided by its
    height.
    """
    return (ink.width * cell_aspect * ink.height) ** 0.5


def contained(ink: Box, slack: float) -> bool:
    """The ink stays inside the slot: the glyph's cell and its gutter, and
    the row it sits on."""
    return (
        ink.x0 >= -slack
        and ink.x1 <= SLOT_CELLS + slack
        and ink.y0 >= -slack
        and ink.y1 <= 1 + slack
    )


def whole(in_slot: Box, free: Box, slack: float) -> bool:
    """The slot did not cut the icon: it drew as much ink inside the slot
    as it does with free space around it."""
    return in_slot.width >= free.width - slack and in_slot.height >= free.height - slack


def aligned(next_text_x0: float, declared_column: float, slack: float) -> bool:
    """Text after the slot starts where the layout said it would."""
    return abs(next_text_x0 - declared_column) <= slack


def outliers(sizes: dict[str, float], tolerance: float = TOLERANCE) -> dict[str, float]:
    """Symbols whose size falls outside `tolerance` of the set's median,
    with their deviation from it (signed, as a fraction)."""
    if not sizes:
        return {}
    middle = median(sizes.values())
    if middle <= 0:
        return {}
    deviation = {name: size / middle - 1 for name, size in sizes.items()}
    return {name: d for name, d in deviation.items() if abs(d) > tolerance}


def spread(sizes: dict[str, float]) -> float:
    """The widest deviation from the median, as a fraction — the smallest
    tolerance these sizes would meet."""
    if not sizes:
        return 0.0
    middle = median(sizes.values())
    return max(abs(size / middle - 1) for size in sizes.values()) if middle > 0 else 0.0
