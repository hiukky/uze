#!/usr/bin/env python3
"""The pinned font catalog: fetched, verified by digest, never committed.

`python3 conformance/rendering/fonts.py fetch [--into DIR]` downloads every
archive `fonts.json` names, refuses one whose sha256 differs from the pin,
and extracts only the builds the catalog measures. An archive already in
`DIR` is verified rather than fetched again, so a warm cache needs no
network.
"""

import argparse
import hashlib
import json
import os
import sys
import tarfile
import urllib.request
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
CATALOG = HERE / "fonts.json"


@dataclass(frozen=True)
class Font:
    family: str
    build: str  # "plain" | "mono" | "propo"
    file: str
    path: Path

    @property
    def id(self) -> str:
        return f"{self.family} / {self.build}"


def default_cache() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or os.path.join(Path.home(), ".cache")
    return Path(base) / "uze-rendering-lab" / "fonts"


def catalog() -> dict:
    return json.loads(CATALOG.read_text())


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _fetch_verified(url: str, target: Path, pinned: str) -> None:
    if not target.exists():
        partial = target.with_suffix(target.suffix + ".part")
        with urllib.request.urlopen(url) as response, partial.open("wb") as out:
            while chunk := response.read(1 << 20):
                out.write(chunk)
        partial.rename(target)
    actual = _sha256(target)
    if actual != pinned:
        target.unlink()
        raise SystemExit(
            f"{target.name}: sha256 {actual} does not match the pinned {pinned}"
        )


def fetch(into: Path) -> list[Font]:
    """Every font of the catalog, fetched and verified into `into`."""
    spec = catalog()
    into.mkdir(parents=True, exist_ok=True)
    _fetch_verified(
        spec["glyphnames"]["url"],
        into / "glyphnames.json",
        spec["glyphnames"]["sha256"],
    )
    fonts = []
    for entry in spec["archives"]:
        archive = into / entry["archive"]
        _fetch_verified(spec["base_url"] + entry["archive"], archive, entry["sha256"])
        wanted = set(entry["builds"].values())
        missing = [name for name in wanted if not (into / name).exists()]
        if missing:
            with tarfile.open(archive) as tar:
                for member in tar.getmembers():
                    if member.name in missing:
                        tar.extract(member, into, filter="data")
        for build, name in entry["builds"].items():
            path = into / name
            if not path.exists():
                raise SystemExit(f"{entry['archive']} does not contain {name}")
            fonts.append(Font(entry["family"], build, name, path))
    return fonts


def installed(into: Path | None = None) -> list[Font]:
    """The catalog as already fetched; fails if anything is missing, so a
    run never measures fewer fonts than the catalog names."""
    into = into or Path(os.environ.get("UZE_LAB_FONTS", default_cache()))
    fonts = []
    for entry in catalog()["archives"]:
        for build, name in entry["builds"].items():
            path = into / name
            if not path.exists():
                raise SystemExit(f"{name} is not in {into}; run `fonts.py fetch` first")
            fonts.append(Font(entry["family"], build, name, path))
    return fonts


def glyphnames(into: Path | None = None) -> dict:
    into = into or Path(os.environ.get("UZE_LAB_FONTS", default_cache()))
    return json.loads((into / "glyphnames.json").read_text())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    fetch_cmd = sub.add_parser("fetch", help="download and verify the catalog")
    fetch_cmd.add_argument("--into", type=Path, default=None)
    args = parser.parse_args()
    if args.command == "fetch":
        into = args.into or Path(os.environ.get("UZE_LAB_FONTS", default_cache()))
        for font in fetch(into):
            print(f"{font.id:32} {font.path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
