"""What the second process does on this harness — see `experiments.relaunch_probe`.

python3 conformance/lab.py --harness opencode --experiment opencode/relaunch
"""

from experiments.relaunch_probe import run

__all__ = ["run"]
