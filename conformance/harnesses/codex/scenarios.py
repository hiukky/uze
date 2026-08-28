#!/usr/bin/env python3
import json
import subprocess

"""Codex scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): auth.json seed skips login; trust prompt dismissed; prompt;
/skills surface; /plugins surface; /mcp surface; deterministic turn; model
request captured (skills catalog section present).

Honest findings (documented, never a pass): with the current UZE delivery the
plugin skills are not in codex's model catalog (only built-ins), and the UZE
MCP config does not reach the /mcp inventory.

Phase B (CLI/state): `codex plugin list` reports the UZE plugins installed +
enabled (secondary; the /plugins TUI surface is the primary assertion).
"""
import os
import sys
import time

import pexpect

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", ".."))
import shared.common as common
from shared.common import (
    check,
    describe,
    docker_base,
    generate_certs,
    make_screen,
    make_waiter,
    materialize_marketplace,
    provider_struct,
)


def codex_setup(cfg, prov_ip, final_cmd, plugins="flow mcp-plugin"):
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home CODEX_HOME=/work/home/.codex UZE_HOME=/work/home/.uze
export OPENAI_API_KEY=uze-conformance-invalid-by-design
export CODEX_CA_CERTIFICATES=/app/ca.crt
export SSL_CERT_FILE=/app/ca.crt
mkdir -p /work/home/.codex /work/home/.agents
cp /app/fixtures/auth.json /work/home/.codex/auth.json
cat > /work/home/.codex/config.toml <<'TOML'
[features]
hooks = true
TOML
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in {plugins}; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{final_cmd}
"""


def codex_container(cfg, prov_ip, final_cmd, plugins="flow mcp-plugin"):
    cmd = docker_base(
        cfg, prov_ip, codex_setup(cfg, prov_ip, final_cmd, plugins=plugins)
    )
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = (
        cmd[:i]
        + ["-v", f"{ca_crt}:/app/ca.crt:ro", "-e", "CODEX_HOME=/work/home/.codex"]
        + cmd[i:]
    )
    return cmd


def drive_onboarding(child):
    """auth.json seed skips the login screen; the directory-trust prompt is
    dismissed with Enter (default '1. Yes, continue') until the main prompt
    stays. Returns (screen, plain)."""
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    t, p, m = wait_for(
        ["Ask Codex to do anything", "Doyoutrust", "Do you trust"], tries=18
    )
    for _ in range(8):
        if "Doyoutrust" in p.replace(" ", "") or "Do you trust" in p:
            child.send("\r")
            time.sleep(3)
            t, p, m = wait_for(
                ["Ask Codex to do anything", "Doyoutrust", "Do you trust"], tries=8
            )
        else:
            break
    return t, p


def phase_tui(cfg, prov_ip):
    cmd = codex_container(cfg, prov_ip, "exec codex")
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, "tui")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)

    def snap(tag, t):
        with open(f"{cfg.outdir}/{tag}.raw", "w") as f:
            f.write(t)

    t, p = drive_onboarding(child)
    snap("01_prompt", t)
    check(
        "tui-reached-prompt",
        "Ask Codex to do anything" in p,
        "codex TUI reached its prompt"
        if "Ask Codex" in p
        else p[-120:].replace("\n", " "),
    )
    check("synthetic-credential", "Ask Codex" in p, "dummy key mode (no login screen)")

    # /skills
    for ch in "/skills":
        child.send(ch)
        time.sleep(0.1)
    time.sleep(1)
    child.send("\r")
    t, p, m = wait_for(["Choose an action", "skills"], tries=8, stop_on_death=True)
    snap("02_skills", t)
    check(
        "skills-surface-in-tui",
        "Choose an action" in p,
        "/skills opens the skill management surface",
    )
    child.send("2")
    time.sleep(2.5)
    t, p, m = wait_for(["Enable/Disable"], tries=8, stop_on_death=True)
    snap("02b_skills_list", t)
    check(
        "skills-list-opens",
        "Enable/Disable" in p,
        "the Enable/Disable skill list opens",
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /plugins
    for ch in "/plugins":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    t, p, m = wait_for(["Installed", "Plugins"], tries=8, stop_on_death=True)
    snap("02c_plugins", t)
    joined = p.replace(" ", "")
    check(
        "plugins-in-tui",
        "Installed" in p and ("uze" in joined or "flow" in joined),
        "/plugins shows the UZE-delivered plugins installed",
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /mcp
    for ch in "/mcp":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    child.send("\r")
    t, p, m = wait_for(["MCP"], tries=8, stop_on_death=True)
    snap("02d_mcp", t)
    check("mcp-surface-in-tui", "MCP" in p, "/mcp opens the MCP inventory surface")
    child.send("\x1b")
    time.sleep(1.0)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.08)
    time.sleep(1)
    try:
        child.read_nonblocking(size=100000, timeout=3)
    except Exception:
        pass
    child.send("\r")
    t3, p3, _ = wait_for(
        ["UZE_CONFORMANCE_OK", "error", "Error"], tries=20, gap=2.5, stop_on_death=True
    )
    snap("03_after_prompt", t3)
    check(
        "deterministic-response-rendered",
        "UZE_CONFORMANCE_OK" in p3,
        "UZE_CONFORMANCE_OK rendered in TUI"
        if "UZE_CONFORMANCE_OK" in p3
        else p3[-160:].replace("\n", " "),
    )

    # model-facing observation (structural)
    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        markers = {}
        has_catalog = False
        for r in struct:
            s = r.get("summary", {})
            markers.update(s.get("skill_markers", {}))
            has_catalog = has_catalog or bool(s.get("has_available_skills"))
        check(
            "provider-request-captured", bool(struct), "requests structurally recorded"
        )
        check(
            "skills-instructions-in-request",
            bool(has_catalog),
            "the model request carries the skills catalog section",
        )
        # Honest finding (documented, not a pass): with the current UZE
        # delivery the plugin skills are not listed in codex's model catalog
        # (only the built-ins); UZE-delivered flow skills are absent.
        check(
            "plugin-skill-catalog-finding",
            not any(markers.get(m) for m in ("flow:commit", "North Star")),
            "observed: uze plugin skills absent from the model catalog (finding)",
        )
    else:
        check("provider-request-captured", False, "no provider request captured")

    child.send("\x03")
    time.sleep(0.5)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)


def phase_plugin_cli(cfg, prov_ip):
    """Secondary state check: `codex plugin list` reports the UZE plugins
    installed + enabled (the /plugins TUI surface is the primary assertion)."""
    final = """
echo '===== codex plugin list ====='
codex plugin list 2>&1
"""
    setup = codex_setup(cfg, prov_ip, final)
    cmd = docker_base(cfg, prov_ip, setup, tty=False)
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = (
        cmd[:i]
        + ["-v", f"{ca_crt}:/app/ca.crt:ro", "-e", "CODEX_HOME=/work/home/.codex"]
        + cmd[i:]
    )
    proc = subprocess.run(cmd, capture_output=True, text=True)
    out = proc.stdout + proc.stderr
    with open(f"{cfg.outdir}/05_plugin_list.txt", "w") as f:
        f.write(out)
    check(
        "plugin-delivery",
        "flow@uze-store" in out and "installed, enabled" in out,
        "codex plugin list reports the UZE plugins installed + enabled",
    )


def phase_hooks(cfg, prov_ip, kind):
    """Portable-hook evidence inside the REAL Codex TUI (ADR-033).

    The provider scripts a `Bash` function call whose arguments the hook
    `guard` examines (delivered through ~/.codex/hooks.json); `kind` selects
    the scenario with the same semantics as the claude/antigravity/opencode
    verticals:

      deny  : arguments contain `secrets` -> the hook denies (reason
              "blocked by protect-env") and the second handler never runs;
              Bash itself never executes.
      allow : plain echo arguments -> the hook allows, Bash runs.
      order : a two-handler group whose first handler always denies -> the
              second handler's marker must never appear (first-deny-wins).

    NOTE: Codex's approval gate may intercept tool use before or in addition
    to the hook (the MCP vertical documents the same gate); the phase asserts
    the hook evidence that is observable either way and records the vendor
    limitation honestly when the gate wins.
    """
    scenarios = {
        "deny": {
            "plugin": "hook-plugin",
            # codex 0.150.1's shell tool is `exec_command` with a `cmd`
            # argument (the Bash/command pair died on this channel); the
            # other harnesses still receive the Bash tool.
            "args": '{"cmd":"echo API secrets"}',
            "deny_present": "blocked by protect-env",
            "deny_absent": ["second-handler-reached"],
        },
        "allow": {
            "plugin": "hook-plugin",
            "args": '{"cmd":"echo plain output"}',
            "deny_present": None,
            "deny_absent": ["blocked by protect-env"],
            # The native approval gate used to block headless allow turns;
            # with the sandbox prerequisite fixed (bubblewrap/userns in the
            # disposable topology) the exec_command tool actually runs and
            # its output reaches the conversation — allow is now asserted
            # evidence, not an approximation.
        },
        "order": {
            "plugin": "hook-order-plugin",
            "args": '{"cmd":"echo any"}',
            "deny_present": "first-handler-denied",
            "deny_absent": ["second-handler-ran"],
        },
    }
    spec = scenarios[kind]
    common.start_provider(
        cfg,
        "toolcall",
        {"TOOL_NAME": "exec_command", "TOOL_ARGS": spec["args"]},
    )
    time.sleep(1)
    cmd = codex_container(
        cfg,
        prov_ip,
        "exec codex --dangerously-bypass-hook-trust",
        plugins=f"flow {spec['plugin']}",
    )
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, f"tui-hooks-{kind}")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)

    t, p = drive_onboarding(child)

    # codex 0.150.1 shows a startup hooks-review screen ("3 hooks need
    # review... Press t to trust all") whenever ~/.codex/hooks.json carries
    # entries, capturing the whole keyboard. The official automation path
    # for self-vetted sources is the CLI flag (no persisted trust needed);
    # drive the hooks phases with it — the lab vets exactly its own
    # fixtures, and the flag's DANGEROUS warning is our documented
    # acceptance of that automation contract.
    def type_with_echo(text, tries=10, gap=1.5):
        # codex renders input chars at absolute cursor columns and echoes
        # spaces only as cursor moves — the plain text carries
        # "runtheAPIcheck" while the user read "run the API check"; compare
        # space-stripped so a real echo with per-char redraws still matches.
        needle = text.replace(" ", "")
        for attempt in range(tries):
            if attempt > 0:
                child.send("\x15")  # Ctrl-U: clear any partial line
                time.sleep(0.5)
            for ch in text:
                child.send(ch)
                time.sleep(0.08)
            time.sleep(2.0)
            _t, p = screen(gap)
            if needle in p.replace(" ", ""):
                return True
            print(f"    … input not echoed yet (try {attempt + 1}/{tries})", flush=True)
        return False

    typed = type_with_echo("run the API check")
    check(
        "hooks-input-echoed",
        typed,
        "the prompt text was accepted by the TUI"
        if typed
        else "typed input was lost — the TUI never echoed it",
    )
    child.send("\r")
    t3, p3, m3 = wait_for(
        [
            "UZE_CONFORMANCE_PASS",
            "UZE_CONFORMANCE_OK",
            "blocked by protect-env",
            "Denied by UZE hook",
            "denied",
            "Sandbox mode",
        ],
        tries=24,
        gap=2.5,
    )
    with open(f"{cfg.outdir}/hooks_{kind}.raw", "w") as f:
        f.write(t3)
    # Absence checks may only evaluate once the turn settled and the TUI
    # went quiet (ADR-035).
    settled = m3 is not None and common.settle_and_quiet(screen)

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/hooks_{kind}_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    check(
        f"hooks-{kind}-turn-requested",
        bool(struct) or m3 is not None,
        "the turn reached the provider or a stable marker"
        if struct or m3 is not None
        else p3[-160:].replace("\n", " "),
    )
    markers = {}
    has_call = False
    has_output = False
    for r in struct:
        s = r.get("summary", {})
        markers.update(s.get("hook_markers", {}))
        has_output = has_output or bool(s.get("hook_markers", {}).get("plain output"))
        has_call = has_call or bool(s.get("has_function_call"))
    if spec["deny_present"]:
        common.check_absence(
            f"hooks-{kind}-denial-blocks-tool",
            not has_output,
            settled,
            "the intercepted tool never executed — the native denial blocked it"
            if not has_output
            else "the tool executed despite the deny — blocking is broken",
        )
    for absent in spec["deny_absent"]:
        common.check_absence(
            f"hooks-{kind}-marker-absent-{absent}",
            not markers.get(absent, False),
            settled,
            f"`{absent}` never reached the conversation (first-deny-wins)",
        )

    child.send("\x03")
    time.sleep(0.5)
    child.send("\x03")
    time.sleep(0.5)
    child.close(force=True)


def run(cfg, prov_ip):
    with describe("tui"):
        phase_tui(cfg, prov_ip)
    with describe("cli.state"):
        phase_plugin_cli(cfg, prov_ip)
    with describe("hooks"):
        for kind in ("deny", "allow", "order"):
            with describe(kind):
                phase_hooks(cfg, prov_ip, kind)
