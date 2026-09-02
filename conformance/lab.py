#!/usr/bin/env python3
"""Conformance Lab entry point — Real Harness + Synthetic World, vertical per
harness (one directory per vendor: `harnesses/<vendor>/` owns its provider,
TUI drive, scenarios and fixtures).

Run: python3 lab.py --harness antigravity|claude|codex|opencode [run-index]
Each run recreates the `--internal` network + provider + harness containers
from clean state. Evidence goes under AGY_OUTDIR (default
/tmp/harness-conformance/<harness>/run<N>).

Gate (ADR-035): every canonical run is adjudicated against
`conformance/evidence/expected.json` — an unregistered ADAPTED result
fails, a registered ADAPTED that starts passing fails (escalate), the real
harness version is probed into the run manifest (drift is an explicit
event), and absence assertions require a settled, quiet turn.
`--write-summary` records the in-repo per-harness evidence summary;
`--retry-once` reruns only a run-level crash (never an assertion failure).

Exploration modes (openspec/changes/conformance-exploration-sandbox):
- `--sandbox <h> [--shell] [--keep] [-- cmd...]` — interactive sandbox:
  topology stays alive, operator gets a recorded harness-TUI session, a
  rootless shell inside the harness container, or runs one command.
- `--experiment <vendor>/<name> [--variation SPEC]` — versioned
  experiment scenarios outside the canonical suite (separate verdict).
- `--matrix <variants.json> [--harnesses a,b,c]` — cross-harness
  compatibility matrix: variant overlays on the fixture marketplace ×
  harnesses, one report per cell.
"""

import argparse
import importlib
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import contract
import contract.bindings
import gate
from shared import common
from shared.common import sh


def parse_args(argv):
    # `-- cmd...` (sandbox scripted commands) must survive argparse: split
    # the trailing command off before parsing and reattach it to the args.
    trailing = []
    if "--" in argv:
        split = argv.index("--")
        trailing = argv[split + 1 :]
        argv = argv[:split]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--harness",
        default="antigravity",
        choices=("antigravity", "claude", "codex", "opencode", "uze"),
    )
    parser.add_argument(
        "run_index",
        nargs="?",
        default="1",
        help="run identifier (evidence run directory)",
    )
    parser.add_argument(
        "--write-summary",
        action="store_true",
        help="write conformance/evidence/<harness>.json (in-repo evidence trail)",
    )
    parser.add_argument(
        "--retry-once",
        action="store_true",
        help="rerun once on a run-level crash only; assertion failures are never retried",
    )
    parser.add_argument(
        "--sandbox",
        action="store_true",
        help="interactive sandbox: provision the topology and hand over a recorded session",
    )
    parser.add_argument(
        "--shell",
        action="store_true",
        help="sandbox: rootless sh inside the harness container (default: the harness TUI)",
    )
    parser.add_argument(
        "--keep",
        action="store_true",
        help="sandbox/matrix: keep the provider container and network after exiting",
    )
    parser.add_argument(
        "--experiment",
        metavar="VENDOR/NAME",
        help="run an experiment scenario from conformance/experiments/<vendor>/<name>.py",
    )
    parser.add_argument(
        "--variation",
        metavar="SPEC",
        help="adversarial provider variation spec (e.g. slow_sse:0.4,duplicate:message_stop)",
    )
    parser.add_argument(
        "--matrix",
        metavar="VARIANTS.JSON",
        help="cross-harness compatibility matrix over the given variant manifest",
    )
    parser.add_argument(
        "--harnesses",
        default="",
        help="matrix: comma-separated harness subset (default: all four)",
    )
    parser.add_argument(
        "--discovery",
        action="store_true",
        help="capture raw provider-side requests beside the run evidence "
        "(never committed; sandbox/experiment/vertical runs)",
    )
    args = parser.parse_args(argv)
    args.trailing = trailing
    return args


def teardown(cfg):
    subprocess.run(["docker", "rm", "-f", cfg.prov_name], capture_output=True)
    subprocess.run(["docker", "network", "rm", cfg.net], capture_output=True)


def provision(cfg):
    subprocess.run(["docker", "rm", "-f", cfg.prov_name], capture_output=True)
    subprocess.run(["docker", "network", "rm", cfg.net], capture_output=True)
    sh("docker", "network", "create", "--internal", cfg.net)


def load_bindings(harness):
    """This harness's bindings, or `None` while it still has none.

    Returning `None` is deliberate during the migration: a vertical without
    bindings keeps running exactly as before, so the contract can be adopted
    one harness at a time instead of in a single unreviewable step.
    """
    try:
        module = importlib.import_module(f"harnesses.{harness}.bindings")
    except ModuleNotFoundError:
        return None
    for value in vars(module).values():
        if (
            isinstance(value, type)
            and issubclass(value, contract.bindings.Bindings)
            and value is not contract.bindings.Bindings
        ):
            return value()
    return None


def run_once(cfg, scenario, variation=None, discovery=False):
    """One full vertical: provision, run, adjudicate. Returns the outcome
    dict plus the run manifest; `outcome["crash"]` records a run-level
    crash (never an assertion failure — those are verdict entries)."""
    provision(cfg)
    started_at = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    extra_env = {}
    if variation:
        extra_env["VARIATION"] = variation
    if discovery:
        extra_env["DISCOVERY"] = "1"
    prov_ip = common.start_provider(cfg, "static", extra_env or None)
    common.reset_results()
    crash = None
    try:
        # The common contract first, then whatever is genuinely unique to
        # this vendor. Order matters for reading a failed run: a contract
        # failure is the product; a vertical failure is one harness.
        bindings = load_bindings(cfg.harness)
        if bindings is not None:
            contract.run(cfg, prov_ip, bindings)
        scenario.run(cfg, prov_ip)
    except Exception as exc:
        crash = f"{type(exc).__name__}: {exc}"

    harness_version = common.probe_harness_version(cfg)
    for result in common.results:
        result.setdefault("harness", cfg.harness)
        result["harness_version"] = harness_version

    # The gate adjudicates every verdict (ADR-035); a malformed/missing
    # registry must fail loudly, never default to green.
    registry = gate.load_registry()
    results = gate.evaluate(cfg.harness, common.results, registry)
    passed = sum(1 for r in results if r["pass"])
    failures = [r for r in results if not r["pass"]]
    manifest = common.run_manifest(cfg, harness_version, started_at, crash=crash)
    manifest["variation"] = variation
    manifest["discovery"] = bool(discovery)
    verdict = {"manifest": manifest, "gate": "v1", "results": results}
    with open(os.path.join(cfg.outdir, "verdict.json"), "w") as f:
        json.dump(verdict, f, indent=1)
    if discovery:
        common.pull_captures(cfg)

    teardown(cfg)
    return {
        "passed": passed,
        "total": len(results),
        "known_adapted": sum(
            1 for r in results if r.get("gate", {}).get("adjudication") == "known_adapt"
        ),
        "failures": failures,
        "crash": crash,
        "manifest": manifest,
    }


def print_summary(outcome):
    passed, total = outcome["passed"], outcome["total"]
    adapted = outcome["known_adapted"]
    print(
        f"\n=== {passed}/{total} asserted PASS, {adapted} ADAPTED (gate) ===",
        flush=True,
    )
    if outcome["crash"]:
        print(f"=== RUN CRASH: {outcome['crash']} ===", flush=True)
    for r in outcome["failures"]:
        adj = r.get("gate", {}).get("adjudication", "asserted")
        reason = r["gate"].get("reason") or r["detail"]
        print(f"  ❌ {r['suite']}: {r['check']} [{adj}] — {reason}", flush=True)
    groups = {}
    for r in common.results:
        top = r.get("suite", r["check"]).split(" > ")[0]
        entry = groups.setdefault(top, {"pass": 0, "total": 0})
        entry["pass"] += int(r["pass"])
        entry["total"] += 1
    print("=== by group ===")
    for name, entry in sorted(groups.items()):
        mark = "✅" if entry["pass"] == entry["total"] else "❌"
        print(f"  {mark} {name}: {entry['pass']}/{entry['total']}", flush=True)
    manifest = outcome["manifest"]
    print(
        f"=== provenance: harness={manifest['harness_version']!r} "
        f"uze={manifest['uze_version']!r} image={manifest['image_id']!r} "
        f"fixture={manifest['fixture_revision']!r}",
        flush=True,
    )
    if manifest.get("version_drift"):
        drift = manifest["version_drift"]
        print(
            f"=== VERSION DRIFT: {drift['from']} -> {drift['to']} "
            "(explicit event per ADR-035)",
            flush=True,
        )


def run_canonical(cfg, scenario, args):
    """Canonical vertical with retry-once and the in-repo summary."""
    retry = 0
    outcome = None
    for attempt in (1, 2):
        if attempt > 1:
            print(
                "\n=== retry-once: rerunning after a run-level crash ===\n", flush=True
            )
            retry = 1
        outcome = run_once(
            cfg, scenario, variation=args.variation, discovery=args.discovery
        )
        # Assertion failures and gate failures return normally — never
        # retried. Only a crash may trigger the retry budget.
        if outcome["crash"] is None or not args.retry_once or attempt == 2:
            break
    print_summary(outcome)
    if args.write_summary:
        path = common.write_evidence_summary(
            cfg, outcome["manifest"], outcome, retry=retry
        )
        print(f"=== evidence summary: {path} ===", flush=True)
    sys.exit(0 if outcome["passed"] == outcome["total"] and not outcome["crash"] else 1)


def run_sandbox(cfg, scenario, args, argv):
    """Interactive sandbox (openspec/changes/conformance-exploration-sandbox).

    Provisions the topology and keeps it alive; the operator then gets:
    `-- cmd...` — one non-interactive command run inside the harness
    container with the fixture market pre-registered (recorded);
    `--shell` — a rootless sh PTY; otherwise the harness's own TUI, driven
    interactively. Sessions are recorded where a PTY is available; teardown
    is disposable unless `--keep`.
    """
    command = None
    if getattr(args, "trailing", None):
        command = args.trailing
    provision(cfg)
    prov_ip = common.start_provider(
        cfg, "static", {"DISCOVERY": "1"} if args.discovery else None
    )
    print(
        f"=== sandbox: harness={cfg.harness} network={cfg.net} provider={prov_ip} "
        f"outdir={cfg.outdir} ===",
        flush=True,
    )
    try:
        if command is not None:
            log = os.path.join(cfg.outdir, "sandbox-command.log")
            setup_fragment = common.materialize_marketplace(cfg)
            full = (
                f"{setup_fragment}\n"
                f"uze market add /work/market >/dev/null 2>&1\n" + " ".join(command)
            )
            with open(log, "wb") as f:
                r = subprocess.run(
                    docker_shell_cmd(cfg, prov_ip, full),
                    capture_output=True,
                    text=True,
                    timeout=300,
                )
                f.write(r.stdout.encode())
                f.write(r.stderr.encode())
            print(r.stdout)
            if r.stderr:
                print(r.stderr, file=sys.stderr)
            print(
                f"=== sandbox command exit: {r.returncode}; log: {log} ===", flush=True
            )
        else:
            run_interactive(cfg, scenario, prov_ip, args)
    finally:
        if not args.keep:
            teardown(cfg)
            print("=== sandbox torn down (use --keep to retain) ===", flush=True)
    sys.exit(0)


def docker_shell_cmd(cfg, prov_ip, shell_command):
    """A rootless shell session inside the harness container: fixtures and
    the pre-registered market are present, exactly like a canonical setup."""
    cmd = [
        "docker",
        "run",
        "--rm",
        "-e",
        "HOME=/work/home",
        "-e",
        "UZE_HOME=/work/home/.uze",
    ]
    for h in common.HARNESS_HOSTS.get(cfg.harness, []):
        cmd += ["--add-host", f"{h}:{prov_ip}"]
    cmd += [
        "--network",
        cfg.net,
        "-v",
        f"{cfg.fix}:/app/fixtures:ro",
        common.HARNESS_IMAGE,
        "sh",
        "-lc",
        shell_command,
    ]
    return cmd


def run_interactive(cfg, scenario, prov_ip, args):
    """A recorded PTY session — the harness TUI or a shell — handed to the
    operator. Without a controlling terminal the session cannot be driven
    interactively; the command form (-- cmd...) is the scripted path."""
    import pexpect

    if not sys.stdin.isatty():
        print(
            "=== no controlling terminal: interactive TUI unavailable — use "
            "`-- cmd...` for scripted sandbox commands ===",
            flush=True,
        )
        return None
    if args.shell:
        child = pexpect.spawn(
            docker_shell_cmd(cfg, prov_ip, "sh")[0],
            docker_shell_cmd(cfg, prov_ip, "sh")[1:],
            encoding="utf-8",
            codec_errors="replace",
            timeout=300,
        )
    else:
        cmd = sandbox_tui_command(cfg, prov_ip, scenario)
        child = pexpect.spawn(
            cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
        )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, "sandbox")
    except Exception:
        pass
    child.interact()
    child.close()
    return True


def sandbox_tui_command(cfg, prov_ip, scenario):
    """The harness's own TUI command, built by the same per-harness
    container builders the canonical phases use."""
    builders = {
        "claude": ("claude_container", (cfg, prov_ip, "exec claude")),
        "codex": ("codex_container", (cfg, prov_ip, "exec codex")),
        "antigravity": ("agy_setup", (cfg, prov_ip, True, "exec agy")),
        "opencode": (
            "opencode_container",
            (
                cfg,
                prov_ip,
                "UZE_HOME=/usr/local/.uze PATH=/usr/local/.uze/shims:$PATH exec opencode --standalone",
            ),
        ),
    }
    name, args = builders[cfg.harness]
    builder = getattr(scenario, name)
    if name == "agy_setup":
        return common.docker_base(cfg, prov_ip, builder(*args))
    return builder(*args)


def run_experiment(cfg, vendor, name, variation, discovery=False):
    """Experiment scenarios (openspec/changes/conformance-exploration-sandbox):
    versioned, outside the canonical suite, no gate registry — their verdict
    is their own, recorded under the run outdir."""
    module = importlib.import_module(f"experiments.{vendor}.{name}")
    experiment_cfg = common.Config(cfg.harness, cfg.run)
    experiment_cfg.outdir = os.path.join(cfg.outdir, "experiments", f"{vendor}-{name}")
    os.makedirs(experiment_cfg.outdir, exist_ok=True)
    variation = variation or getattr(module, "VARIATION", None)
    print(
        f"=== experiment {vendor}/{name} on {cfg.harness}"
        + (f" (variation: {variation})" if variation else "")
        + " ===",
        flush=True,
    )
    provision(experiment_cfg)
    extra_env = {}
    if variation:
        extra_env["VARIATION"] = variation
    if discovery:
        extra_env["DISCOVERY"] = "1"
    prov_ip = common.start_provider(experiment_cfg, "static", extra_env or None)
    common.reset_results()
    module.run(experiment_cfg, prov_ip)
    results = list(common.results)
    passed = sum(1 for r in results if r["pass"])
    if discovery:
        common.pull_captures(experiment_cfg)
    with open(os.path.join(experiment_cfg.outdir, "verdict.json"), "w") as f:
        json.dump(
            {
                "experiment": f"{vendor}/{name}",
                "results": results,
                "variation": variation,
                "discovery": discovery,
            },
            f,
            indent=1,
        )
    teardown(experiment_cfg)
    print(f"\n=== experiment verdict: {passed}/{len(results)} PASS", flush=True)
    sys.exit(0 if passed == len(results) else 1)


def main(argv=None):
    argv = list(argv) if argv is not None else sys.argv[1:]
    args = parse_args(argv)
    cfg = common.Config(args.harness, args.run_index)
    common.CURRENT_HARNESS = args.harness
    scenario = importlib.import_module(f"harnesses.{args.harness}.scenarios")

    if args.experiment:
        vendor, _, name = args.experiment.partition("/")
        run_experiment(cfg, vendor, name, args.variation, discovery=args.discovery)
        return

    if args.matrix:
        from matrix import run_matrix

        run_matrix(cfg, args.matrix, args.harnesses or None, keep=args.keep)
        return

    common.validate_marketplace(cfg)
    if args.sandbox:
        run_sandbox(cfg, scenario, args, argv)
        return
    run_canonical(cfg, scenario, args)


if __name__ == "__main__":
    main()
