#!/usr/bin/env python3
"""Cross-harness compatibility matrix (openspec/changes/
conformance-exploration-sandbox).

Turns a variant manifest — overlays on `_fixtures/marketplace/` (hooks.json
shapes, invoke policies, AGENTS.md forms) — into measured compatibility
evidence: each (variant × harness) cell runs the harness's canonical
vertical against the overlaid market and lands in a single report of
PASS/ADAPTED/FAIL with evidence links. Trade-offs are measured, never
assumed; cells are independent runs (no cross-cell state).

Run: python3 conformance/lab.py --matrix variants.json
     [--harnesses claude,opencode] [--keep]
"""

from __future__ import annotations

import importlib
import json
import os
import shutil
import sys
import time
from typing import Any

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lab
from shared import common

MATRIX_OUT = "matrix"


def load_variants(path: str) -> dict[str, Any]:
    """Validates the variant manifest: `{"variants": [{"id", "overlay"}]}`
    where each overlay maps a marketplace-relative path to its replacement
    content, or to `null` for deletion."""
    with open(path) as f:
        document = json.load(f)
    variants = document.get("variants")
    if not variants:
        raise RuntimeError(f"matrix variant manifest has no variants: {path}")
    for variant in variants:
        if not variant.get("id") or not isinstance(variant.get("overlay"), dict):
            raise RuntimeError(f"invalid matrix variant entry: {variant}")
    return document


def build_variant_market(cfg, variant: dict[str, Any], out_dir: str) -> str:
    """A fresh copy of the canonical fixture market with the variant's
    overlays applied (files replaced, `null` deletes). Deterministic per
    variant — cells never mutate the canonical fixture tree."""
    source = os.path.join(cfg.repo, "_fixtures", "marketplace")
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)
    shutil.copytree(source, out_dir)
    for relative, content in variant.get("overlay", {}).items():
        target = os.path.join(out_dir, relative)
        if content is None:
            os.remove(target)
            continue
        os.makedirs(os.path.dirname(target), exist_ok=True)
        if isinstance(content, (dict, list)):
            with open(target, "w") as f:
                json.dump(content, f, indent=2)
                f.write("\n")
        else:
            with open(target, "w") as f:
                f.write(content)
    return out_dir


def run_cell(
    harness: str, variant: dict[str, Any], variant_dir: str, run_id: str
) -> dict[str, Any]:
    """One (variant × harness) cell: the harness's canonical vertical
    against the overlaid market, adjudicated by the same gate."""
    cfg = common.Config(harness, run_id)
    cfg.outdir = os.path.join(
        cfg.repo, MATRIX_OUT, run_id, f"{harness}-{variant['id']}"
    )
    os.makedirs(cfg.outdir, exist_ok=True)
    os.environ["UZE_MARKETPLACE_MOUNT"] = variant_dir
    scenario = importlib.import_module(f"harnesses.{harness}.scenarios")
    outcome = lab.run_once(cfg, scenario)
    os.environ.pop("UZE_MARKETPLACE_MOUNT", None)
    return {
        "harness": harness,
        "variant": variant["id"],
        "passed": outcome["passed"],
        "total": outcome["total"],
        "known_adapted": outcome["known_adapted"],
        "crash": outcome["crash"],
        "failures": [
            {
                "check": r["check"],
                "suite": r["suite"],
                "adjudication": r["gate"]["adjudication"],
            }
            for r in outcome["failures"]
        ],
        "evidence": cfg.outdir,
    }


def render_table(cells: list[dict[str, Any]]) -> str:
    """A readable PASS/ADAPTED/FAIL grid (variant × harness)."""
    harnesses = sorted({c["harness"] for c in cells})
    variants = sorted({c["variant"] for c in cells})
    rows = [
        "| variant \\ harness | " + " | ".join(harnesses) + " |",
        "|" + "---|" * (len(harnesses) + 1),
    ]
    verdicts = {(c["variant"], c["harness"]): c for c in cells}
    for variant in variants:
        row = [f"| {variant}"]
        for harness in harnesses:
            cell = verdicts.get((variant, harness))
            if cell is None:
                row.append("—")
            elif cell["crash"]:
                row.append("CRASH")
            elif cell["passed"] == cell["total"]:
                row.append(f"{cell['passed']}/{cell['total']} ✅")
            elif cell["known_adapted"] or any(
                f["adjudication"] != "asserted" for f in cell["failures"]
            ):
                row.append(f"{cell['passed']}/{cell['total']} 🟡")
            else:
                row.append(f"{cell['passed']}/{cell['total']} ❌")
        rows.append(" | ".join(row) + " |")
    return "\n".join(rows)


def run_matrix(cfg, variants_path, harnesses, keep=False):
    """Entry point from lab.py --matrix."""
    document = load_variants(variants_path)
    variants = document["variants"]
    selected = (harnesses or "antigravity,claude,codex,opencode").split(",")
    run_id = time.strftime("%Y%m%d-%H%M%S")
    cells = []
    for variant in variants:
        variant_dir = os.path.join(
            cfg.repo, MATRIX_OUT, run_id, f"market-{variant['id']}"
        )
        build_variant_market(cfg, variant, variant_dir)
        for harness in selected:
            print(
                f"\n=== matrix cell: variant={variant['id']} harness={harness} ===",
                flush=True,
            )
            cells.append(run_cell(harness, variant, variant_dir, run_id))
            if not keep:
                shutil.rmtree(variant_dir, ignore_errors=True)

    report_path = os.path.join(cfg.repo, MATRIX_OUT, run_id, "matrix.json")
    os.makedirs(os.path.dirname(report_path), exist_ok=True)
    with open(report_path, "w") as f:
        json.dump(
            {"run_id": run_id, "cells": cells, "table": render_table(cells)},
            f,
            indent=1,
        )
    print("\n=== compatibility matrix ===")
    print(render_table(cells))
    print(f"=== matrix evidence: {report_path} ===", flush=True)
    sys.exit(
        0 if all(c["passed"] == c["total"] and not c["crash"] for c in cells) else 1
    )
