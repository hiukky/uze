#!/usr/bin/env python3
"""The Rendering Lab's entry point, from the repository root.

    python3 conformance/rendering/lab.py --suite metrics
    python3 conformance/rendering/lab.py --suite terminals [--terminal kitty] [--font "Hack"]
    python3 conformance/rendering/lab.py --suite replay --evidence DIR
    python3 conformance/rendering/lab.py --sandbox

Everything runs in the Lab image (`uze-rendering-lab`, built from
`conformance/rendering/Dockerfile`) with no network: the fonts, terminals
and `uze` are the ones the image pinned. Evidence lands in
`target/rendering-lab/` unless `--evidence` says otherwise. The screenshots
are artifacts, not repository content. The summary that the docs are
generated from is written with `catalog.py`.
"""

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
IMAGE = "uze-rendering-lab"


def build():
    subprocess.run(
        ["docker", "build", "-f", "conformance/rendering/Dockerfile", "-t", IMAGE, "."],
        cwd=ROOT,
        check=True,
    )


def run(
    args: list[str],
    evidence: Path,
    interactive: bool = False,
    entrypoint: str | None = None,
    uze: Path | None = None,
) -> int:
    evidence.mkdir(parents=True, exist_ok=True)
    command = [
        "docker",
        "run",
        "--rm",
        "--network",
        "none",
        "--shm-size",
        "1g",
        "-v",
        f"{evidence.resolve()}:/evidence",
        # The Lab's own code from the working tree, so a change to an
        # analyzer or a binding is measured without rebuilding the image;
        # the terminals, fonts and `uze` still come from the image.
        "-v",
        f"{ROOT / 'conformance' / 'rendering'}:/lab:ro",
        "-v",
        f"{ROOT / 'crates' / 'uze-theme' / 'themes'}:/themes:ro",
        "-e",
        "UZE_LAB_THEMES=/themes",
    ]
    if uze:
        # Another `uze` than the image's, for a before/after comparison: a
        # build of this tree with an older glyph set, say.
        command += ["-v", f"{uze.resolve()}:/usr/local/bin/uze:ro"]
    if interactive:
        command.append("-it")
    if entrypoint:
        command += ["--entrypoint", entrypoint]
    return subprocess.run(command + [IMAGE, *args]).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--suite", choices=["metrics", "terminals", "replay"], default="terminals"
    )
    parser.add_argument("--terminal", action="append", default=[])
    parser.add_argument("--font", action="append", default=[])
    parser.add_argument("--build", action="append", default=[], dest="builds")
    parser.add_argument("--set", default="nerd")
    parser.add_argument(
        "--uze", type=Path, help="measure this `uze` binary instead of the image's"
    )
    parser.add_argument(
        "--evidence", type=Path, default=ROOT / "target" / "rendering-lab"
    )
    parser.add_argument(
        "--no-build", action="store_true", help="use the image as it is"
    )
    parser.add_argument(
        "--sandbox", action="store_true", help="a shell in the Lab image"
    )
    args = parser.parse_args()

    if not args.no_build:
        build()
    if args.sandbox:
        return run([], args.evidence, interactive=True, entrypoint="bash")
    if args.suite == "metrics":
        return run(
            ["--set", args.set, "--out", "/evidence/metrics"],
            args.evidence,
            entrypoint="/lab/metrics.py",
        )
    if args.suite == "replay":
        return run(["/evidence"], args.evidence, entrypoint="/lab/replay.py")
    forwarded = ["--set", args.set, "--out", "/evidence"]
    for terminal in args.terminal:
        forwarded += ["--terminal", terminal]
    for font in args.font:
        forwarded += ["--font", font]
    for build_name in args.builds:
        forwarded += ["--build", build_name]
    return run(forwarded, args.evidence, uze=args.uze)


if __name__ == "__main__":
    sys.exit(main())
