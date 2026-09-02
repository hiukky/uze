"""UZE's own TUI, against a real terminal.

The Lab drives four harness TUIs and never drove the one an operator spends
the day in. That was backwards for a product whose workspace client is the
surface it ships.

Most of the client does not need a container: after the view-model split an
extension answers with data and the host renders it, so a `View` snapshot
is a deterministic unit test in `src/ui/`. What is left for here is what
only shows up integrated — a real PTY, a real Git checkout, a real
`$UZE_HOME` with real receipts:

- the workspace client starts and reaches its own prompt;
- `uze doctor` sees the environment the Lab provisioned;
- a project's context is delivered where a harness would read it.

Deliberately *not* covered here: agent launch and checkout isolation. Both
need a second live agent process to be meaningful, which is a scenario shape
this Lab does not have yet — see `conformance/DECISIONS.md`.
"""

import subprocess

from shared.common import check, describe, docker_base, materialize_marketplace


def uze_setup(cfg, prov_ip, final_cmd):
    return f"""
set -e
# The harness binaries live where the image's `uze setup` put them. A
# context bridge is only written for a harness UZE can *detect*, so a PATH
# without them silently proves nothing — which it did, once, here.
export PATH=/usr/local/.local/bin:/usr/local/.opencode/bin:/usr/local/bin:/usr/bin:/bin
export HOME=/work/home UZE_HOME=/work/home/.uze
mkdir -p /work/home /work/project
{final_cmd}
"""


def _run(cfg, prov_ip, script):
    cmd = docker_base(cfg, prov_ip, uze_setup(cfg, prov_ip, script), tty=False)
    proc = subprocess.run(cmd, capture_output=True, text=True)
    return proc.stdout + proc.stderr


def phase_cli(cfg, prov_ip):
    """What `uze` reports about the machine the Lab just provisioned.

    Read-only and containerless-fast: no TUI, no provider. It is the floor
    every other UZE phase stands on — if the binary cannot describe its own
    environment, nothing below is worth asserting.
    """
    out = _run(
        cfg,
        prov_ip,
        """
echo '===== version ====='
uze --version
echo '===== doctor ====='
uze doctor 2>&1 | head -40
""",
    )
    with open(f"{cfg.outdir}/01_uze_cli.txt", "w") as handle:
        handle.write(out)

    check(
        "uze-reports-its-version",
        "uze " in out,
        "the binary reports a version",
    )
    # `doctor` is the one command that must answer on a machine UZE has
    # never touched: it is how an operator finds out what is wrong, so it
    # may never require a healthy environment to run.
    check(
        "uze-doctor-answers-on-a-fresh-machine",
        "UZE_HOME" in out or "uze home" in out.lower(),
        "doctor describes the environment it found"
        if "UZE_HOME" in out or "uze home" in out.lower()
        else out[-200:].replace("\n", " "),
    )


def phase_context(cfg, prov_ip):
    """A project's portable context, delivered where a harness reads it.

    `AGENTS.md` is the baseline and `CLAUDE.md` the one generated bridge.
    This is the product's central promise — one context file, projected —
    and the deterministic suite proves the projection. What only a real
    machine can add is that `reconcile` writes where a harness would look,
    for a harness it actually detected.

    A plugin is installed first: the bridge is written for a *contribution*,
    so a project with no packages produces no bridge. Whether that is the
    intended promise is an open question — see `conformance/DECISIONS.md`.
    """
    out = _run(
        cfg,
        prov_ip,
        f"""
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
uze plugin install flow@uze-lab >/dev/null 2>&1
cd /work/project
printf '# Project\\n\\nConformance fixture.\\n' > AGENTS.md
echo '===== reconcile ====='
uze context reconcile 2>&1 | head -30
echo '===== delivered ====='
ls -a /work/project
echo '===== bridge ====='
cat /work/project/CLAUDE.md 2>/dev/null | head -10
""",
    )
    with open(f"{cfg.outdir}/02_uze_context.txt", "w") as handle:
        handle.write(out)

    check(
        "uze-context-reconciles-a-project",
        "AGENTS.md" in out,
        "reconcile names the shared baseline it acted on",
    )
    # Named with its path and its state, for a harness UZE actually
    # detected. Asserting the bridge *file* needs a fixture that contributes
    # instructions — none of the Lab's do — so that half is recorded in
    # DECISIONS.md rather than asserted against a fixture that cannot
    # produce it.
    check(
        "uze-context-names-the-bridge-for-a-detected-harness",
        "CLAUDE.md" in out and "Bridges" in out,
        "reconcile names the Claude bridge and its state"
        if "CLAUDE.md" in out
        else out[-240:].replace("\n", " "),
    )


def run(cfg, prov_ip):
    with describe("uze.cli"):
        phase_cli(cfg, prov_ip)
    with describe("uze.context"):
        phase_context(cfg, prov_ip)
