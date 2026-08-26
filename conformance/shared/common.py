#!/usr/bin/env python3
"""Shared machinery for the per-harness conformance scenarios.

Everything a scenario needs that is not harness-specific: the `--internal`
Docker topology (provider + harness containers), per-run TLS certs for the
TLS-intercepted providers, PTY screen/waiter helpers, and the evidence
`check()` accumulator.
"""
import json
import os
import re
import subprocess
import time

import pexpect

PROVIDER_IMG = "python:3.12-slim"
HARNESS_IMAGE = "conformance-harness:latest"

HARNESS_HOSTS = {
    "claude": ["api.anthropic.com", "platform.claude.com", "console.anthropic.com",
               "statsig.anthropic.com", "api.statsig.com", "sentry.io",
               "telemetry.anthropic.com"],
    "codex": ["api.openai.com"],
}
HARNESS_SANS = {
    "claude": "DNS:api.anthropic.com,DNS:*.anthropic.com,DNS:platform.claude.com,"
              "DNS:*.claude.com,DNS:console.anthropic.com,DNS:statsig.anthropic.com,"
              "DNS:api.statsig.com,DNS:sentry.io,DNS:telemetry.anthropic.com",
    "codex": "DNS:api.openai.com,DNS:*.openai.com",
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
            "AGY_OUTDIR", f"/tmp/harness-conformance/{harness}/run{run}")
        self.net = "uze-harness-offline"
        self.prov_name = "fake-provider"
        self.cert_dir = os.path.join(self.outdir, "certs")
        self.mcp_proof = "UZE_MCP_CONFORMANCE_PROOF_1"
        self.mcp_fixture_bin = "/usr/local/bin/uze-mcp-conformance-fixture"
        os.makedirs(self.outdir, exist_ok=True)


results = []


def check(name, ok, detail="", kind="assert"):
    results.append({"check": name, "pass": bool(ok), "detail": detail, "kind": kind})
    tag = "PASS" if ok else "FAIL"
    if ok and kind == "adapted":
        tag = "ADAPTED"
    symbol = {"PASS": "✅", "ADAPTED": "🟡", "FAIL": "❌"}[tag]
    print(f"{symbol} [{tag:7s}] {name}" + (f"  ({detail})" if detail else ""))


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
    )
    for relative_path in required:
        path = os.path.join(cfg.marketplace_source, relative_path)
        if not os.path.isfile(path):
            raise RuntimeError(f"missing conformance marketplace resource: {relative_path}")

    with open(os.path.join(cfg.marketplace_source, "plugins/mcp-plugin/mcp.json")) as f:
        mcp = json.load(f)
    server = mcp.get("mcpServers", {}).get("uze-conformance", {})
    if server.get("command") != "__UZE_MCP_FIXTURE_BINARY__" or server.get("args") != [
        "--proof", "__UZE_MCP_CONFORMANCE_PROOF__"
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
        sh("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
           "-keyout", ca_key, "-out", ca_crt, "-days", "30",
           "-subj", "/CN=UZE Synthetic CA", "-sha256")
    if not os.path.exists(leaf_crt):
        sh("openssl", "req", "-newkey", "rsa:2048", "-nodes",
           "-keyout", leaf_key, "-out", leaf_csr, "-subj", "/CN=api.example.com")
        ext = os.path.join(cfg.cert_dir, "ext.cnf")
        with open(ext, "w") as f:
            f.write(f"subjectAltName={HARNESS_SANS[cfg.harness]}\nextendedKeyUsage=serverAuth\n")
        sh("openssl", "x509", "-req", "-in", leaf_csr, "-CA", ca_crt, "-CAkey", ca_key,
           "-CAcreateserial", "-out", leaf_crt, "-days", "30", "-extfile", ext, "-sha256")
    return ca_crt, leaf_crt, leaf_key


def start_provider(cfg, mode, extra_env=None):
    """Runs the synthetic provider container on the internal net.

    antigravity: fake_gemini (plain HTTP 9999); claude: fake_anthropic
    (TLS 443, Anthropic hosts); codex: fake_openai (TLS 443, api.openai.com).
    `extra_env` adds `-e K=V` pairs (e.g. the hook-scenario tool name/args).
    """
    subprocess.run(["docker", "rm", "-f", cfg.prov_name], capture_output=True)
    env = ["-e", f"PROVIDER_MODE={mode}",
           "-e", "PROVIDER_STRUCT=/app/struct.json",
           "-e", f"MCP_PROOF={cfg.mcp_proof}"]
    for name, value in (extra_env or {}).items():
        env += ["-e", f"{name}={value}"]

    # The hook scenarios parameterize the scripted tool call via the same
    # envs the MCP toolcall phases use; defaults keep MCP behavior unchanged.
    env += ["-e", f"TOOL_NAME={os.environ.get('HOOK_TOOL', 'Bash')}",
            "-e", f"TOOL_ARGS={os.environ.get('HOOK_ARGS', '{}')}"]
    provider = os.path.join(cfg.repo, "harnesses", cfg.harness)
    if cfg.harness == "antigravity":
        mounts = ["-v", f"{provider}/provider.py:/app/fp.py:ro"]
        if mode == "static":
            mounts += ["-v", f"{cfg.fix}/simple_turn.sse:/app/resp.sse:ro",
                       "-e", "PROVIDER_RESP=/app/resp.sse"]
        else:
            # Hook scenarios pass their own FC_ARGS (a `run_command` call
            # carrying the marker); the MCP default applies otherwise.
            if not extra_env or "FC_ARGS" not in extra_env:
                env += ["-e", 'FC_ARGS={"serverName":"uze-conformance","toolName":"uze_conformance","arguments":{}}']
            env += ["-e", "FINAL_TEXT=UZE_CONFORMANCE_PASS"]
        sh("docker", "run", "-d", "--name", cfg.prov_name, "--network", cfg.net,
           *mounts, *env, PROVIDER_IMG, "python", "/app/fp.py", "9999")
    elif cfg.harness == "opencode":
        env += ["-e", "RESPONSE_TEXT=UZE_CONFORMANCE_OK",
                "-e", "FINAL_TEXT=UZE_CONFORMANCE_PASS"]
        sh("docker", "run", "-d", "--name", cfg.prov_name, "--network", cfg.net,
           "-v", f"{provider}/provider.py:/app/fp.py:ro",
           *env, PROVIDER_IMG, "python", "/app/fp.py", "9999")
    elif cfg.harness == "claude":
        _, leaf_crt, leaf_key = generate_certs(cfg)
        env += ["-e", "LEAF_CERT=/app/leaf.crt", "-e", "LEAF_KEY=/app/leaf.key",
                "-e", "RESPONSE_TEXT=UZE_CONFORMANCE_OK",
                "-e", "FINAL_TEXT=UZE_CONFORMANCE_PASS"]
        sh("docker", "run", "-d", "--name", cfg.prov_name, "--network", cfg.net,
           "-v", f"{provider}/provider.py:/app/fp.py:ro",
           "-v", f"{leaf_crt}:/app/leaf.crt:ro",
           "-v", f"{leaf_key}:/app/leaf.key:ro",
           *env, PROVIDER_IMG, "python", "/app/fp.py")
    else:
        _, leaf_crt, leaf_key = generate_certs(cfg)
        env += ["-e", "LEAF_CERT=/app/leaf.crt", "-e", "LEAF_KEY=/app/leaf.key",
                "-e", "RESPONSE_TEXT=UZE_CONFORMANCE_OK"]
        sh("docker", "run", "-d", "--name", cfg.prov_name, "--network", cfg.net,
           "-v", f"{provider}/provider.py:/app/fp.py:ro",
           "-v", f"{leaf_crt}:/app/leaf.crt:ro",
           "-v", f"{leaf_key}:/app/leaf.key:ro",
           *env, PROVIDER_IMG, "python", "/app/fp.py")
    time.sleep(2)
    return subprocess.check_output(
        ["docker", "inspect", "-f", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
         cfg.prov_name], text=True).strip()


def provider_struct(cfg):
    try:
        out = subprocess.run(["docker", "exec", cfg.prov_name, "cat", "/app/struct.json"],
                             capture_output=True, text=True).stdout
        return json.loads(out) or []
    except Exception:
        return []


def docker_base(cfg, prov_ip, final_cmd, tty=True):
    cmd = ["docker", "run", "--rm"] + (["-it"] if tty else []) + ["--network", cfg.net]
    for h in HARNESS_HOSTS.get(cfg.harness, []):
        cmd += ["--add-host", f"{h}:{prov_ip}"]
    cmd += ["--tmpfs", "/tmp:rw,exec,nosuid,nodev,size=128m,uid=1000,gid=1000,mode=700",
            "--tmpfs", "/work:rw,noexec,nosuid,nodev,size=512m,uid=1000,gid=1000,mode=700",
            "-e", "HOME=/work/home", "-e", "UZE_HOME=/work/home/.uze",
            "-v", f"{cfg.fix}:/app/fixtures:ro",
            HARNESS_IMAGE, "sh", "-c", final_cmd]
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
        return pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8",
                             codec_errors="replace", timeout=300)
    wrapped = ["script", "-q", "--timing", timing, "-c",
               " ".join([f'"{c}"' if " " in c else c for c in cmd]), rec]
    child = pexpect.spawn(wrapped[0], wrapped[1:], encoding="utf-8",
                          codec_errors="replace", timeout=300)
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


def make_screen(child):
    def screen(wait=2.2):
        time.sleep(wait)
        try:
            t = child.read_nonblocking(size=250000, timeout=6)
        except Exception:
            t = ""
        p = re.sub(r"\x1b\][^\x07]*\x07", "", t)
        p = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", p).replace("\x1b", "")
        return t, p
    return screen


def make_waiter(screen):
    def wait_for(markers, tries=12, gap=2.0):
        for _ in range(tries):
            t, p = screen(gap)
            for m in markers:
                if m in p:
                    return t, p, m
        return t, p, None
    return wait_for
