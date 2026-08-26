#!/usr/bin/env python3
"""Conformance Lab entry point — Real Harness + Synthetic World, vertical per
harness (one directory per vendor: `harnesses/<vendor>/` owns its provider,
TUI drive, scenarios and fixtures).

Run: python3 lab.py --harness antigravity|claude|codex [run-index]
Each run recreates the `--internal` network + provider + harness containers
from clean state. Evidence goes under AGY_OUTDIR (default
/tmp/harness-conformance/<harness>/run<N>).
"""
import importlib
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shared import common
from shared.common import check, sh


def main():
    args = [a for a in sys.argv[1:] if a.startswith("--harness")]
    harness = "antigravity"
    if args:
        harness = args[0].split("=", 1)[1] if "=" in args[0] else sys.argv[sys.argv.index(args[0]) + 1]
    run_id = next((a for a in sys.argv[1:]
                   if a not in ("--harness",) and not a.startswith("--") and a != harness), "1")

    cfg = common.Config(harness, run_id)
    if harness not in ("antigravity", "claude", "codex", "opencode"):
        raise RuntimeError(f"unknown harness: {harness}")

    common.validate_marketplace(cfg)
    scenario = importlib.import_module(f"harnesses.{harness}.scenarios")

    subprocess.run(["docker", "rm", "-f", cfg.prov_name], capture_output=True)
    subprocess.run(["docker", "network", "rm", cfg.net], capture_output=True)
    sh("docker", "network", "create", "--internal", cfg.net)

    prov_ip = common.start_provider(cfg, "static")
    scenario.run(cfg, prov_ip)

    return finish(cfg)


def finish(cfg):
    passed = sum(1 for r in common.results if r["pass"])
    adapted = sum(1 for r in common.results if r["kind"] == "adapted")
    print(f"\n=== {passed}/{len(common.results)} asserted PASS, {adapted} ADAPTED ===")
    with open(f"{cfg.outdir}/verdict.json", "w") as f:
        import json
        json.dump(common.results, f, indent=1)
    subprocess.run(["docker", "rm", "-f", cfg.prov_name], capture_output=True)
    subprocess.run(["docker", "network", "rm", cfg.net], capture_output=True)
    sys.exit(0 if passed == len(common.results) else 1)


if __name__ == "__main__":
    main()
