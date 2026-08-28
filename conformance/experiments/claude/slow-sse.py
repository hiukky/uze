"""Tolerance-mapping experiment: real Claude Code under slow streaming.

The canonical phase asserts `UZE_CONFORMANCE_OK` renders while the synthetic
provider answers promptly; this experiment adds `slow_sse:0.4` (0.4s between
SSE frames) and runs the same TUI drive, recording what still holds under
degraded streaming — the tolerance evidence.

Not part of the canonical suite (no gate registry): promote into
`harnesses/claude/scenarios.py` only after the promotion checklist (3
consecutive clean runs) passes.

Run: python3 conformance/lab.py --harness claude --experiment claude/slow-sse
"""

from harnesses.claude.scenarios import phase_tui
from shared import common

VARIATION = "slow_sse:0.4"


def run(cfg, prov_ip):
    try:
        phase_tui(cfg, prov_ip)
    except Exception as exc:  # the experiment records the crash, never hides it
        common.check(
            "experiment-ran-to-completion",
            False,
            f"the TUI drive crashed under the variation: {type(exc).__name__}: {exc}",
        )
        return
    common.check(
        "experiment-ran-to-completion",
        True,
        f"canonical TUI drive completed under VARIATION={VARIATION}",
    )
