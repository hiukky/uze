#!/usr/bin/env python3
"""Shared machinery for the per-harness conformance scenarios.

Everything a scenario needs that is not harness-specific: the `--internal`
Docker topology (provider + harness containers), per-run TLS certs for the
TLS-intercepted providers, PTY screen/waiter helpers, and the evidence
`check()` accumulator.
"""

import contextlib
import json
import os
import subprocess
import sys
import time

import pexpect

PROVIDER_IMG = "python:3.12-slim"
HARNESS_IMAGE = "conformance-harness:latest"

HARNESS_HOSTS = {
    "claude": [
        "api.anthropic.com",
        "platform.claude.com",
        "console.anthropic.com",
        "statsig.anthropic.com",
        "api.statsig.com",
        "sentry.io",
        "telemetry.anthropic.com",
    ],
    "codex": ["api.openai.com"],
}
HARNESS_SANS = {
    "claude": "DNS:api.anthropic.com,DNS:*.anthropic.com,DNS:platform.claude.com,"
    "DNS:*.claude.com,DNS:console.anthropic.com,DNS:statsig.anthropic.com,"
    "DNS:api.statsig.com,DNS:sentry.io,DNS:telemetry.anthropic.com",
    "codex": "DNS:api.openai.com,DNS:*.openai.com",
    # UZE's own vertical drives no vendor endpoint — it exercises the
    # client and the CLI, never a provider — so it needs only a SAN the
    # certificate generator will accept.
    "uze": "DNS:api.example.com",
}


class Config:
    """Per-run context shared with the harness scenario module."""

    def __init__(self, harness, run):
        self.harness = harness
        self.run = run
        self.repo = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        self.fix = os.path.join(self.repo, "harnesses", harness, "fixtures")
        self.marketplace = "/opt/uze-conformance-fixtures/marketplace"
        self.marketplace_source = os.path.join(self.repo, "_fixtures", "marketplace")
        self.outdir = os.environ.get(
            "AGY_OUTDIR", f"/tmp/harness-conformance/{harness}/run{run}"
        )
        # Nonced per process so concurrent labs (vertical loop + sandbox +
        # experiment + matrix cells) never share or fight over a network —
        # cross-talk between two labs' providers would corrupt evidence.
        nonce = os.getpid()
        self.net = f"uze-harness-offline-{nonce}"
        self.prov_name = f"fake-provider-{nonce}"
        self.cert_dir = os.path.join(self.outdir, "certs")
        self.mcp_proof = "UZE_MCP_CONFORMANCE_PROOF_1"
        self.mcp_fixture_bin = "/usr/local/bin/uze-mcp-conformance-fixture"
        os.makedirs(self.outdir, exist_ok=True)


results = []

# Stamped by lab.py so every verdict carries harness provenance (ADR-035):
# the gate keys registrations by (harness, check).
CURRENT_HARNESS: str | None = None

# Active describe() group stack (Jest-style): group names are indented in
# the live log and carried into every verdict entry, so a growing suite
# (skills, mcp, hooks, ...) stays interpretable.
SUITE_STACK: list[str] = []


def reset_results():
    """Clears the verdict accumulator for a fresh run (each vertical starts
    from zero evidence; nothing carries across runs)."""
    results.clear()


@contextlib.contextmanager
def describe(name: str):
    """Opens a named group for every `check` that follows. Groups nest
    (`with describe("hooks"): with describe("deny"): ...`); the current
    chain is prepended to each verdict's `suite` field and rendered as
    indentation in the live log. `run` outside any group is allowed — the
    harness-level checks then carry no suite prefix."""
    SUITE_STACK.append(name)
    indent = "  " * (len(SUITE_STACK) - 1)
    print(f"\n{indent}▸ {name}", flush=True)
    try:
        yield
    finally:
        SUITE_STACK.pop()


def suite_path(name: str) -> str:
    return " > ".join([*SUITE_STACK, name])


def settle_and_quiet(screen, quiet=None, budget=None):
    """Requires a window with no new TUI bytes before absence checks may
    evaluate (ADR-035): 'never appeared' is only provable once the turn
    settled and the surface went quiet. Returns True when the quiet window
    elapsed within the budget. Window lengths are env-overridable
    (`UZE_CONFORMANCE_QUIET_MS` / `UZE_CONFORMANCE_QUIET_BUDGET_S`) for
    debugging short-run failures."""
    quiet = (
        quiet
        if quiet is not None
        else float(os.environ.get("UZE_CONFORMANCE_QUIET_MS", "2500")) / 1000
    )
    budget = (
        budget
        if budget is not None
        else float(os.environ.get("UZE_CONFORMANCE_QUIET_BUDGET_S", "12.0"))
    )
    deadline = time.time() + budget
    last_bytes = time.time()
    while time.time() < deadline:
        t, _p = screen(0.5)
        if t:
            last_bytes = time.time()
        if time.time() - last_bytes >= quiet:
            return True
    return False


VERDICT_SYMBOL = {"PASS": "✅", "ADAPTED": "🟡", "FAIL": "❌"}
VERDICT_COLOR = {"PASS": "\033[32m", "ADAPTED": "\033[33m", "FAIL": "\033[31m"}
VERDICT_LABEL_WIDTH = max(len(tag) for tag in VERDICT_SYMBOL) + len("[]")


def print_verdict(tag, name, detail=""):
    """One live-log line per verdict, aligned on the widest tag.

    Colour is a terminal courtesy only: a redirected log (CI, `tee`, an
    evidence capture) must stay byte-clean, so escapes are emitted solely
    for a TTY and suppressed under NO_COLOR.
    """
    label = f"[{tag}]".ljust(VERDICT_LABEL_WIDTH)
    if sys.stdout.isatty() and "NO_COLOR" not in os.environ:
        label = f"{VERDICT_COLOR[tag]}{label}\033[0m"
    indent = "  " * len(SUITE_STACK)
    suffix = f"  ({detail})" if detail else ""
    print(f"{indent}{VERDICT_SYMBOL[tag]} {label} {name}{suffix}", flush=True)


def check_absence(name, ok, settled, detail=""):
    """Absence assertion under the settled-turn contract (ADR-035).

    An absence (a marker that must never appear) is only provable once the
    turn settled and the TUI went quiet. An unsettled turn FAILS the check
    with that reason recorded — it can never pass by accident.
    """
    suite = suite_path(name)
    if not settled:
        detail = f"turn never settled — absence not proven ({detail})"
        results.append(
            {
                "check": name,
                "suite": suite,
                "pass": False,
                "detail": detail,
                "kind": "assert",
                "harness": CURRENT_HARNESS,
            }
        )
        print_verdict("FAIL", name, detail)
        return
    check(name, ok, detail)


def check(name, ok, detail="", kind="assert"):
    suite = suite_path(name)
    results.append(
        {
            "check": name,
            "suite": suite,
            "pass": bool(ok),
            "detail": detail,
            "kind": kind,
            "harness": CURRENT_HARNESS,
        }
    )
    tag = "PASS" if ok else "FAIL"
    if ok and kind == "adapted":
        tag = "ADAPTED"
    print_verdict(tag, name, detail)


def sh(*args, ok=(0,)):
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode not in ok:
        raise RuntimeError(f"{' '.join(args)} rc={r.returncode}: {r.stderr[-500:]}")
    return r


def materialize_marketplace(cfg):
    """Returns the shell fragment that creates one disposable Lab market.

    The checked-in conformance marketplace is the complete product input for
    every vertical. Only its MCP executable and proof are run-specific.
    """
    return f"""
cp -r {cfg.marketplace} /work/market
sed -i 's|__UZE_MCP_FIXTURE_BINARY__|{cfg.mcp_fixture_bin}|g; s|__UZE_MCP_CONFORMANCE_PROOF__|{cfg.mcp_proof}|g' /work/market/plugins/mcp-plugin/mcp.json
"""


def validate_marketplace(cfg):
    """Fails before Docker starts if the shared final-resource fixture drifts."""
    with open(os.path.join(cfg.marketplace_source, "marketplace.json")) as f:
        manifest = json.load(f)
    if manifest.get("name") != "uze-lab":
        raise RuntimeError("conformance marketplace must retain the uze-lab identity")
    plugins = {plugin["name"]: plugin["source"] for plugin in manifest["plugins"]}
    expected = {
        "flow": "./plugins/flow",
        "mcp-plugin": "./plugins/mcp-plugin",
        "hook-plugin": "./plugins/hook-plugin",
        "hook-order-plugin": "./plugins/hook-order-plugin",
        "hook-fail-plugin": "./plugins/hook-fail-plugin",
    }
    if plugins != expected:
        raise RuntimeError(f"invalid conformance marketplace inventory: {plugins}")

    required = (
        "plugins/flow/skills/commit/SKILL.md",
        "plugins/flow/skills/review/SKILL.md",
        "plugins/flow/skills/analyze/SKILL.md",
        "plugins/flow/agents/reviewer.md",
        "plugins/mcp-plugin/mcp.json",
        "plugins/hook-plugin/hooks.json",
        "plugins/hook-plugin/scripts/guard",
        "plugins/hook-plugin/scripts/mark",
        "plugins/hook-order-plugin/hooks.json",
        "plugins/hook-order-plugin/scripts/order-1",
        "plugins/hook-order-plugin/scripts/order-2",
        "plugins/hook-fail-plugin/hooks.json",
        "plugins/hook-fail-plugin/plugin.json",
    )
    for relative_path in required:
        path = os.path.join(cfg.marketplace_source, relative_path)
        if not os.path.isfile(path):
            raise RuntimeError(
                f"missing conformance marketplace resource: {relative_path}"
            )

    with open(os.path.join(cfg.marketplace_source, "plugins/mcp-plugin/mcp.json")) as f:
        mcp = json.load(f)
    server = mcp.get("mcpServers", {}).get("uze-conformance", {})
    if server.get("command") != "__UZE_MCP_FIXTURE_BINARY__" or server.get("args") != [
        "--proof",
        "__UZE_MCP_CONFORMANCE_PROOF__",
    ]:
        raise RuntimeError("invalid conformance MCP fixture placeholders")


def generate_certs(cfg):
    os.makedirs(cfg.cert_dir, exist_ok=True)
    ca_key = os.path.join(cfg.cert_dir, "ca.key")
    ca_crt = os.path.join(cfg.cert_dir, "ca.crt")
    leaf_key = os.path.join(cfg.cert_dir, "leaf.key")
    leaf_csr = os.path.join(cfg.cert_dir, "leaf.csr")
    leaf_crt = os.path.join(cfg.cert_dir, "leaf.crt")
    if not os.path.exists(ca_crt):
        sh(
            "openssl",
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            ca_key,
            "-out",
            ca_crt,
            "-days",
            "30",
            "-subj",
            "/CN=UZE Synthetic CA",
            "-sha256",
        )
    if not os.path.exists(leaf_crt):
        sh(
            "openssl",
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            leaf_key,
            "-out",
            leaf_csr,
            "-subj",
            "/CN=api.example.com",
        )
        ext = os.path.join(cfg.cert_dir, "ext.cnf")
        with open(ext, "w") as f:
            f.write(
                f"subjectAltName={HARNESS_SANS[cfg.harness]}\nextendedKeyUsage=serverAuth\n"
            )
        sh(
            "openssl",
            "x509",
            "-req",
            "-in",
            leaf_csr,
            "-CA",
            ca_crt,
            "-CAkey",
            ca_key,
            "-CAcreateserial",
            "-out",
            leaf_crt,
            "-days",
            "30",
            "-extfile",
            ext,
            "-sha256",
        )
    return ca_crt, leaf_crt, leaf_key


def start_provider(cfg, mode, extra_env=None):
    """Runs the synthetic provider container on the internal net.

    antigravity: fake_gemini (plain HTTP 9999); claude: fake_anthropic
    (TLS 443, Anthropic hosts); codex: fake_openai (TLS 443, api.openai.com).
    `extra_env` adds `-e K=V` pairs (e.g. the hook-scenario tool name/args).
    """
    subprocess.run(["docker", "rm", "-f", cfg.prov_name], capture_output=True)
    env = [
        "-e",
        f"PROVIDER_MODE={mode}",
        "-e",
        "PROVIDER_STRUCT=/app/struct.json",
        "-e",
        f"MCP_PROOF={cfg.mcp_proof}",
    ]
    for name, value in (extra_env or {}).items():
        env += ["-e", f"{name}={value}"]

    # Hook scenarios script the intercepted tool themselves via extra_env
    # (Bash on claude/codex, the MCP tool on opencode, run_command on
    # antigravity) and the MCP toolcall phases rely on the provider's own
    # defaults — so a runner-level override must never be injected by
    # default, and must never clobber the scenario's choice: extra_env is
    # merged below and wins (docker: last -e wins).
    hook_tool = os.environ.get("HOOK_TOOL")
    if hook_tool:
        env += [
            "-e",
            f"TOOL_NAME={hook_tool}",
            "-e",
            f"TOOL_ARGS={os.environ.get('HOOK_ARGS', '{}')}",
        ]
    provider = os.path.join(cfg.repo, "harnesses", cfg.harness)
    shared_mounts = [
        "-v",
        f"{cfg.repo}/shared/variation.py:/app/variation.py:ro",
        "-v",
        f"{cfg.repo}/shared/capture.py:/app/capture.py:ro",
        "-v",
        f"{cfg.repo}/shared/websocket.py:/app/websocket.py:ro",
    ]
    if cfg.harness == "antigravity":
        mounts = ["-v", f"{provider}/provider.py:/app/fp.py:ro", *shared_mounts]
        if mode == "static":
            mounts += [
                "-v",
                f"{cfg.fix}/simple_turn.sse:/app/resp.sse:ro",
                "-e",
                "PROVIDER_RESP=/app/resp.sse",
            ]
        else:
            # Hook scenarios pass their own FC_ARGS (a `run_command` call
            # carrying the marker); the MCP default applies otherwise.
            if not extra_env or "FC_ARGS" not in extra_env:
                env += [
                    "-e",
                    'FC_ARGS={"serverName":"uze-conformance","toolName":"uze_conformance","arguments":{}}',
                ]
            env += ["-e", "FINAL_TEXT=UZE_CONFORMANCE_PASS"]
        sh(
            "docker",
            "run",
            "-d",
            "--name",
            cfg.prov_name,
            "--network",
            cfg.net,
            *mounts,
            *env,
            PROVIDER_IMG,
            "python",
            "/app/fp.py",
            "9999",
        )
    elif cfg.harness == "opencode":
        env += [
            "-e",
            "RESPONSE_TEXT=UZE_CONFORMANCE_OK",
            "-e",
            "FINAL_TEXT=UZE_CONFORMANCE_PASS",
        ]
        sh(
            "docker",
            "run",
            "-d",
            "--name",
            cfg.prov_name,
            "--network",
            cfg.net,
            "-v",
            f"{provider}/provider.py:/app/fp.py:ro",
            *shared_mounts,
            *env,
            PROVIDER_IMG,
            "python",
            "/app/fp.py",
            "9999",
        )
    elif cfg.harness == "claude":
        _, leaf_crt, leaf_key = generate_certs(cfg)
        env += [
            "-e",
            "LEAF_CERT=/app/leaf.crt",
            "-e",
            "LEAF_KEY=/app/leaf.key",
            "-e",
            "RESPONSE_TEXT=UZE_CONFORMANCE_OK",
            "-e",
            "FINAL_TEXT=UZE_CONFORMANCE_PASS",
        ]
        sh(
            "docker",
            "run",
            "-d",
            "--name",
            cfg.prov_name,
            "--network",
            cfg.net,
            "-v",
            f"{provider}/provider.py:/app/fp.py:ro",
            "-v",
            f"{leaf_crt}:/app/leaf.crt:ro",
            "-v",
            f"{leaf_key}:/app/leaf.key:ro",
            *shared_mounts,
            *env,
            PROVIDER_IMG,
            "python",
            "/app/fp.py",
        )
    else:
        _, leaf_crt, leaf_key = generate_certs(cfg)
        env += [
            "-e",
            "LEAF_CERT=/app/leaf.crt",
            "-e",
            "LEAF_KEY=/app/leaf.key",
            "-e",
            "RESPONSE_TEXT=UZE_CONFORMANCE_OK",
        ]
        sh(
            "docker",
            "run",
            "-d",
            "--name",
            cfg.prov_name,
            "--network",
            cfg.net,
            "-v",
            f"{provider}/provider.py:/app/fp.py:ro",
            "-v",
            f"{leaf_crt}:/app/leaf.crt:ro",
            "-v",
            f"{leaf_key}:/app/leaf.key:ro",
            *shared_mounts,
            *env,
            PROVIDER_IMG,
            "python",
            "/app/fp.py",
        )
    time.sleep(2)
    return subprocess.check_output(
        [
            "docker",
            "inspect",
            "-f",
            "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
            cfg.prov_name,
        ],
        text=True,
    ).strip()


def observed_markers(struct, field):
    """Whether each marker reached the provider in *any* request of a turn.

    A turn is several requests — the model call, then settings polls and
    telemetry batches — and only their union answers "did this ever reach
    the model". Last-write-wins let a marker-free trailing request erase
    the one model call that carried a hook's denial.
    """
    seen = {}
    for r in struct:
        for marker, present in r.get("summary", {}).get(field, {}).items():
            seen[marker] = seen.get(marker, False) or bool(present)
    return seen


def provider_struct(cfg):
    try:
        out = subprocess.run(
            ["docker", "exec", cfg.prov_name, "cat", "/app/struct.json"],
            capture_output=True,
            text=True,
        ).stdout
        return json.loads(out) or []
    except Exception:
        return []


def pull_captures(cfg):
    """Pulls the provider's raw request log (--discovery) beside the run
    evidence. Absent log / provider already gone = no-op — captures are best
    effort by design; raw captures never enter the repository."""
    try:
        r = subprocess.run(
            [
                "docker",
                "cp",
                f"{cfg.prov_name}:/app/raw-requests.log",
                os.path.join(cfg.outdir, "raw-requests.log"),
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )
        return r.returncode == 0
    except Exception:
        return False


# ============================================================================
# Version provenance (ADR-035): every run probes the harness's real version
# with the vendor's own `--version` flag — the same probes the product
# integrations use for detection — so a channel bump is an explicit report
# event, never a silent behavior change. Probes run inside the Lab image
# with `--network none`; a failure records `unknown`, never a crash.
# ============================================================================

VERSION_PROBES = {
    "claude": ["claude", "--version"],
    "codex": ["codex", "--version"],
    # The Lab image ships the real binary as `opencode2` (the `opencode`
    # name is the uze shim inside scenario setups) — same resolution order
    # as uze-integrations' resolve_opencode_binary.
    "opencode": ["opencode2", "--version"],
    "antigravity": ["agy", "--version"],
    "uze": ["uze", "--version"],
}


def _image_command(cmd, timeout=30):
    try:
        out = subprocess.run(
            [
                "docker",
                "run",
                "--rm",
                "--network",
                "none",
                "-e",
                "HOME=/home/node",
                HARNESS_IMAGE,
                *cmd,
            ],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        first = out.stdout.strip().splitlines()[0].strip() if out.stdout.strip() else ""
        return first or "unknown"
    except Exception:
        return "unknown"


def probe_harness_version(cfg):
    """The probed harness version, or `unknown` when the probe could not run."""
    probe = VERSION_PROBES.get(cfg.harness)
    return _image_command(probe) if probe else "unknown"


def uze_version(cfg):
    """The `uze` binary version baked into the Lab image."""
    return _image_command(["uze", "--version"])


def image_id(cfg):
    try:
        out = subprocess.run(
            ["docker", "image", "inspect", "-f", "{{.Id}}", HARNESS_IMAGE],
            capture_output=True,
            text=True,
            timeout=15,
        )
        return out.stdout.strip().replace("sha256:", "")[:16] or "unknown"
    except Exception:
        return "unknown"


def repo_revision(cfg):
    """The fixture tree revision baked into this run (repo HEAD)."""
    try:
        out = subprocess.run(
            ["git", "-C", cfg.repo, "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        return out.stdout.strip()[:16] or "unknown"
    except Exception:
        return "unknown"


def previous_harness_version(cfg):
    """The harness version recorded by the last committed summary, if any —
    the baseline for the version-drift report event."""
    try:
        with open(os.path.join(cfg.repo, "evidence", f"{cfg.harness}.json")) as f:
            return json.load(f).get("harness_version")
    except Exception:
        return None


def run_manifest(cfg, harness_version, started_at, crash=None):
    """The per-run provenance record (ADR-035): harness probe, uze version,
    fixture revision, image id, timestamps, and an explicit version-drift
    event vs. the previous committed summary."""
    previous = previous_harness_version(cfg)
    drift = None
    if (
        previous
        and harness_version
        and previous != harness_version
        and harness_version != "unknown"
    ):
        drift = {"from": previous, "to": harness_version}
    return {
        "harness": cfg.harness,
        "harness_version": harness_version,
        "previous_harness_version": previous,
        "version_drift": drift,
        "uze_version": uze_version(cfg),
        "fixture_revision": repo_revision(cfg),
        "image_id": image_id(cfg),
        "started_at": started_at,
        "finished_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "outdir": cfg.outdir,
        "crash": crash,
    }


def write_evidence_summary(cfg, manifest, outcome, retry=0):
    """Writes the per-harness evidence summary (ADR-035) — versions,
    per-kind counts, gate verdict. Written next to the run evidence by
    default (`UZE_EVIDENCE_DIR`), or into `conformance/evidence/` for
    local runs; CI stores summaries as Actions artifacts, never pushed."""
    target = os.environ.get("UZE_EVIDENCE_DIR") or os.path.join(cfg.repo, "evidence")
    os.makedirs(target, exist_ok=True)
    path = os.path.join(target, f"{cfg.harness}.json")
    summary = {
        "harness": cfg.harness,
        "harness_version": manifest["harness_version"],
        "uze_version": manifest["uze_version"],
        "fixture_revision": manifest["fixture_revision"],
        "recorded_at": manifest["finished_at"],
        "gate": {
            "passed": outcome["passed"],
            "total": outcome["total"],
            "known_adapted": outcome["known_adapted"],
            "retry": retry,
            "failures": [
                {
                    "check": r["check"],
                    "suite": r["suite"],
                    "adjudication": r["gate"]["adjudication"],
                    "detail": r["detail"],
                    "gate_reason": r["gate"]["reason"],
                }
                for r in outcome["failures"]
            ],
        },
    }
    with open(path, "w") as f:
        json.dump(summary, f, indent=1)
        f.write("\n")
    return path


def docker_base(cfg, prov_ip, final_cmd, tty=True):
    cmd = (
        [
            "docker",
            "run",
            "--rm",
            # The codex harness sandboxes its own tool execution with
            # bubblewrap, which needs user namespaces; the Lab's default
            # seccomp blocks CLONE_NEWUSER, so the sandbox errors out and the
            # allow path never executes. The topology is already disposable
            # and rootless (`--internal` net, tmpfs, no socket) — relaxing
            # userns for the harness's own sandbox is the documented
            # prerequisite, not an escape hatch.
            "--security-opt",
            "seccomp=unconfined",
        ]
        + (["-it"] if tty else [])
        + ["--network", cfg.net]
    )
    for h in HARNESS_HOSTS.get(cfg.harness, []):
        cmd += ["--add-host", f"{h}:{prov_ip}"]
    # Compatibility-matrix cells (matrix.py) overlay a host-built variant
    # market onto the in-image fixture path; canonical runs never set this.
    matrix_mount = os.environ.get("UZE_MARKETPLACE_MOUNT")
    if matrix_mount:
        cmd += ["-v", f"{matrix_mount}:/opt/uze-conformance-fixtures/marketplace:ro"]
    cmd += [
        "--tmpfs",
        "/tmp:rw,exec,nosuid,nodev,size=128m,uid=1000,gid=1000,mode=700",
        # /work is exec-capable on purpose: portable-hook handlers are shell
        # scripts executed from the derived Store under /work (ADR-033), and
        # the Lab's isolation comes from the `--internal` network, not from
        # a noexec mount.
        "--tmpfs",
        "/work:rw,exec,nosuid,nodev,size=512m,uid=1000,gid=1000,mode=700",
        "-e",
        "HOME=/work/home",
        "-e",
        "UZE_HOME=/work/home/.uze",
        "-v",
        f"{cfg.fix}:/app/fixtures:ro",
        HARNESS_IMAGE,
        "sh",
        "-c",
        final_cmd,
    ]
    return cmd


def spawn_tui(cfg, cmd, tag):
    """Spawns the harness TUI under `script` so the raw PTY session (with
    timing) is recorded into the run's outdir — `scriptreplay` then
    reproduces the interactive TUI with correct rendering, exactly as it
    happened, ANSI and all.

    The returned pexpect child behaves identically (the recorder sits
    between the child and the PTY); only the extra recording files are new.
    """
    rec = os.path.join(cfg.outdir, f"{tag}.typescript")
    timing = os.path.join(cfg.outdir, f"{tag}.timing")
    if not os.path.exists(os.path.join(cfg.outdir, "script-ok")):
        if subprocess.run(["which", "script"], capture_output=True).returncode != 0:
            print("WARN: `script` not found; TUI recording disabled")
            open(os.path.join(cfg.outdir, "script-ok"), "w").write("no")
        else:
            open(os.path.join(cfg.outdir, "script-ok"), "w").write("yes")
    if open(os.path.join(cfg.outdir, "script-ok")).read() != "yes":
        return pexpect.spawn(
            cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
        )
    wrapped = [
        "script",
        "-q",
        "--timing",
        timing,
        "-c",
        " ".join([f'"{c}"' if " " in c else c for c in cmd]),
        rec,
    ]
    child = pexpect.spawn(
        wrapped[0], wrapped[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    return child


class CastRecorder:
    """Grows a `scriptreplay`-compatible recording from the raw PTY stream
    WITHOUT wrapping the child (a `script` wrapper buffers output and broke
    the interactive drive): every chunk the driver reads is appended to the
    typescript file and accounted in the timing file, so `make lab-replay`
    can replay the exact TUI session with correct rendering.
    """

    def __init__(self, outdir, tag):
        self.typescript = open(os.path.join(outdir, f"{tag}.typescript"), "w")
        self.timing = open(os.path.join(outdir, f"{tag}.timing"), "w")
        self.last = time.time()

    def write(self, data):
        now = time.time()
        delay = max(0.000001, now - self.last)
        self.last = now
        self.typescript.write(data)
        self.typescript.flush()
        self.timing.write(f"{delay:.6f} {len(data)}\n")
        self.timing.flush()

    def flush(self):
        self.typescript.flush()
        self.timing.flush()

    def close(self):
        self.typescript.close()
        self.timing.close()


def ansi_strip(text):
    """Removes ANSI escape sequences from a raw TUI stream, state-machine
    style, returning plain text safe for contiguous-marker matching.

    Handles the full xterm grammar: CSI (ESC [ ... final byte, with
    parameters and intermediates), OSC (ESC ] ... BEL or ESC \\ — codex
    renders spinner lines with OSC 8 hyperlinks terminated by ST, which a
    partial regex left as corrupted interleaved characters), DCS/PM/APC/SOS
    (ESC P/^/_/X ... ST), charset selection (ESC ( ...), and lone ESC.
    """
    plain = []
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch == "\x1b":
            if i + 1 >= n:
                i += 1
                continue
            nxt = text[i + 1]
            if nxt == "[":
                # CSI: consume through the final byte (0x40-0x7E).
                j = i + 2
                while j < n and (text[j] < "\x40" or text[j] > "\x7e"):
                    j += 1
                i = j + 1
            elif nxt in "P^_X]":
                # DCS/PM/APC/SOS/OSC: consume until ST (ESC \) or BEL.
                j = i + 2
                while j < n:
                    if text[j] == "\x07":
                        j += 1
                        break
                    if text[j] == "\x1b" and j + 1 < n and text[j + 1] == "\\":
                        j += 2
                        break
                    j += 1
                i = j
            elif nxt in "()#%*+":
                # Charset/width selection: two-char sequence.
                i += 3
            else:
                i += 2
        else:
            plain.append(ch)
            i += 1
    return "".join(plain)


def make_screen(child):
    def screen(wait=2.2):
        time.sleep(wait)
        try:
            t = child.read_nonblocking(size=250000, timeout=6)
        except Exception:
            t = ""
        p = ansi_strip(t)
        return t, p

    # Expose the child so the waiter can abort early when the harness
    # process died (e.g. a setup error inside the container): without this,
    # every wait_for burns its full try budget staring at a dead child.
    screen.child = child
    return screen


def make_waiter(screen):
    def wait_for(markers, tries=12, gap=2.0, stop_on_death=False):
        for attempt in range(tries):
            t, p = screen(gap)
            for m in markers:
                if m in p:
                    return t, p, m
            if (
                stop_on_death
                and getattr(screen, "child", None) is not None
                and not screen.child.isalive()
            ):
                return t, p, None
            # Long waits must not look frozen: show progress while the
            # harness process is still alive but none of the markers have
            # appeared yet. (Stdout is flushed so redirected runs stream.)
            if tries > 6 and attempt >= 2 and attempt % 2 == 1:
                print(
                    f"    … waiting for progress (try {attempt + 1}/{tries})",
                    flush=True,
                )
        return t, p, None

    return wait_for
