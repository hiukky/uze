"""What the second process does on this harness — see `experiments.relaunch_probe`.

python3 conformance/lab.py --harness claude --experiment claude/relaunch
"""

from experiments.relaunch_probe import run

__all__ = ["run"]
