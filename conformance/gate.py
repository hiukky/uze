"""Adaptive-result gate for the Conformance Lab (ADR-035).

Deterministic gate logic over verdict entries: a checked-in registry
(`conformance/evidence/expected.json`) lists every check whose ADAPTED
result is an expected, honest vendor-limitation record. The gate turns the
registry into the anti-false-positive contract:

- an ADAPTED result without a registry entry FAILS the run;
- an ADAPTED result whose entry does not cover the probed harness version
  FAILS with the drift reported;
- a registered ADAPTED check that starts passing FAILS (escalate) until the
  scenario is promoted to an asserted check and the entry removed.

Pure functions only — no docker, no harness knowledge — so the semantics
are unit-tested without the Lab (ADR-035: a silent change can never pass).
"""

from __future__ import annotations

import json
import os
from typing import Any

# `*` in an entry's `versions` list matches any probed version; entries pin
# concrete versions once observed runs establish them, so a vendor bump
# cannot silently change the meaning of a registered adaptation.
ANY_VERSION = "*"

REGISTRY_PATH = os.path.join(os.path.dirname(__file__), "evidence", "expected.json")


def load_registry(path: str | None = None) -> dict[tuple[str, str], dict[str, Any]]:
    """Reads the registry into a {(harness, check): entry} map.

    A missing or unreadable registry is an error, never an empty gate: the
    safe default is that every adaptation is unexpected (fails), not that
    everything passes.
    """
    with open(path or REGISTRY_PATH) as f:
        document = json.load(f)
    return {
        (entry["harness"], entry["check"]): entry
        for entry in document.get("adaptive", [])
    }


def covers_version(entry: dict[str, Any], version: str | None) -> bool:
    """Whether the entry's recorded versions cover the probed version.

    An unprobed version (`None`/empty) counts as covered: the gate cannot
    fail a record it cannot verify — the run manifest records the probe
    failure instead, so the gap stays visible.
    """
    if not version:
        return True
    versions = entry.get("versions") or []
    return ANY_VERSION in versions or version in versions


def evaluate(
    harness: str,
    results: list[dict[str, Any]],
    registry: dict[tuple[str, str], dict[str, Any]],
) -> list[dict[str, Any]]:
    """Applies the gate to verdict entries in place and returns them.

    Each verdict gains `gate`: `{"adjudication": ..., "reason": ...}`.
    Adjudications: `asserted` (ordinary pass/fail), `known_adapt`,
    `unregistered_adapt`, `escalated`, `version_drift`.
    """
    for result in results:
        key = (result.get("harness") or harness, result["check"])
        entry = registry.get(key)
        adjudication = "asserted"
        reason = None
        if result.get("kind") == "adapted":
            if entry is None:
                result["pass"] = False
                adjudication = "unregistered_adapt"
                reason = (
                    "unregistered ADAPTED result — investigate the behavior and "
                    "record it, or register the entry in "
                    "conformance/evidence/expected.json"
                )
            elif not covers_version(entry, result.get("harness_version")):
                result["pass"] = False
                adjudication = "version_drift"
                reason = (
                    f"registered ADAPTED covers {entry.get('versions')} but the "
                    f"run probed {result.get('harness_version')!r}; update the entry"
                )
            else:
                adjudication = "known_adapt"
        elif entry is not None and result["pass"]:
            # A registered adaptive check now passes: the capability
            # surfaced — promote the scenario, then remove the entry.
            result["pass"] = False
            adjudication = "escalated"
            reason = (
                "registered ADAPTED check now passes — promote the scenario to an "
                "asserted check and remove the entry from expected.json"
            )
        result["gate"] = {"adjudication": adjudication, "reason": reason}
    return results


def gate_failures(results: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The verdict entries the gate adjudicated as failures."""
    return [
        r
        for r in results
        if not r["pass"] and r.get("gate", {}).get("adjudication") != "asserted"
    ]
