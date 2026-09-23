#!/usr/bin/env python3
"""Re-analyzes a recorded run's screenshots and checks that the verdicts
come out the same. No terminal is involved.

    replay.py EVIDENCE_DIR

A verdict that cannot be reproduced from its own screenshot was never
evidence. This is also how a change to the analyzer is judged: it must
still read every recorded capture the way it did before, unless the
change is exactly about that.
"""

import json
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import analyze  # noqa: E402


def main() -> int:
    evidence = Path(sys.argv[1] if len(sys.argv) > 1 else "/evidence")
    recorded = json.loads((evidence / "verdict.json").read_text())
    layout = recorded.get("layout")
    if layout is None:
        print(
            "the run recorded no specimen layout; it cannot be replayed",
            file=sys.stderr,
        )
        return 1
    mismatches = 0
    replayed = 0
    for result in recorded["results"]:
        if not result.get("drawn", True) or "symbols" not in result:
            continue
        again = analyze.analyze(Image.open(evidence / result["capture"]), layout)
        replayed += 1
        if (
            again["criteria"] != result["criteria"]
            or again["symbols"] != result["symbols"]
        ):
            mismatches += 1
            print(f"differs: {result['terminal']} / {result['file']}")
    print(f"{replayed - mismatches}/{replayed} captures reproduce their verdict")
    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main())
