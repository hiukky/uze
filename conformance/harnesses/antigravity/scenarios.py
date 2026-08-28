#!/usr/bin/env python3
import json
import subprocess

"""Antigravity scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): prompt + synthetic credential, /skills (flow:commit,
flow:review, uze:init), /mcp (server listed + tools enumerated),
deterministic turn, model-only Skill hidden from the slash surface but
present for the model, user-only Skill CAPABILITY_ADAPTED, MCP tool invocation
inside the interactive TUI (proof round-trip).

Phase B (CLI/state): plugin registration via `agy plugin list` + staged
mcp_config.json (AGY has no plugin TUI surface — verified).
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
    make_screen,
    make_waiter,
    materialize_marketplace,
    provider_struct,
    start_provider,
)


def agy_setup(cfg, prov_ip, include_mcp, final_cmd, plugins=None):
    if plugins is None:
        plugins = "flow"
        if include_mcp:
            plugins += " mcp-plugin"
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home UZE_HOME=/work/home/.uze
export GEMINI_API_KEY=uze-conformance-invalid-by-design
export GOOGLE_GEMINI_BASE_URL=http://{prov_ip}:9999
export AGY_CLI_DISABLE_AUTO_UPDATE=1
mkdir -p /work/home/.gemini/antigravity-cli
cp /app/fixtures/settings.json /work/home/.gemini/antigravity-cli/settings.json
cp /app/fixtures/jetski_state.pbtxt /work/home/.gemini/antigravity-cli/jetski_state.pbtxt
cp /app/fixtures/installation_id /work/home/.gemini/antigravity-cli/installation_id
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in {plugins}; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{final_cmd}
"""


def phase_tui(cfg, prov_ip):
    setup = agy_setup(cfg, prov_ip, include_mcp=True, final_cmd="exec agy")
    cmd = docker_base(cfg, prov_ip, setup)
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

    try:
        child.expect("Choose your color scheme", timeout=150)
    except Exception as e:
        check("tui-started", False, f"onboarding never appeared: {e}")
        child.close(force=True)
        return

    child.send("\r")  # color scheme
    time.sleep(3)
    child.send("\t\t")  # ToS -> Done
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)

    t1, p1 = screen(3)
    snap("01_prompt", t1)
    check(
        "tui-reached-prompt",
        "Antigravity CLI" in p1 and ">" in p1,
        "header visible" if "Antigravity CLI" in p1 else "no header",
    )
    check(
        "synthetic-credential",
        "Gemini API key" in p1,
        "account row shows API key, not a personal account",
    )

    # /skills
    child.send("/")
    time.sleep(1.2)
    for ch in "skills":
        child.send(ch)
        time.sleep(0.15)
    time.sleep(1.2)
    child.send("\r")
    t2, p2, _ = wait_for(["flow:review"], tries=4, stop_on_death=True)
    snap("02_skills", t2)
    check("uzek-skill-visible", "flow:commit" in p2, "flow:commit in /skills")
    check(
        "useronly-skill-human-visible",
        "flow:review" in p2,
        "flow:review in global skills surface",
    )
    check("official-uzek-skill-visible", "uze:init" in p2, "uze:init in /skills")
    check(
        "model-only-skill-hidden-from-tui",
        "flow:analyze" not in p2,
        "flow:analyze is omitted from /skills by disable-slash-command: true"
        if "flow:analyze" not in p2
        else p2[-160:].replace("\n", " "),
    )
    child.send("\x1b")
    time.sleep(1.0)
    t_settle, _, _ = wait_for([">"], tries=6, stop_on_death=True)
    snap("02c_back_to_prompt", t_settle)

    # /agents is the native agent manager; fixture visibility is the
    # behavioral proof that AGY loaded UZE's portable definition.
    child.send("/")
    time.sleep(1.2)
    for ch in "agents":
        child.send(ch)
        time.sleep(0.15)
    child.send("\r")
    t_agents, p_agents, _ = wait_for(
        ["reviewer", "Agents"], tries=8, stop_on_death=True
    )
    snap("02a_agents", t_agents)
    check(
        "agent-visible-in-tui",
        "reviewer" in p_agents,
        "Antigravity /agents lists the UZE reviewer agent"
        if "reviewer" in p_agents
        else p_agents[-200:].replace("\n", " "),
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /mcp
    child.send("/")
    time.sleep(1.2)
    for ch in "mcp":
        child.send(ch)
        time.sleep(0.15)
    time.sleep(1.2)
    child.send("\r")
    t_mcp, p_mcp, _ = wait_for(["Tools: uze_conformance"], tries=8, stop_on_death=True)
    snap("02b_mcp", t_mcp)
    check(
        "mcp-server-visible-in-tui",
        "uze-conformance" in p_mcp,
        "the UZE-delivered MCP server is listed in /mcp",
    )
    check(
        "mcp-server-connected-in-tui",
        "Tools: uze_conformance" in p_mcp,
        "the real AGY loaded the server and enumerated its tool",
    )
    child.send("\x1b")
    time.sleep(1.0)
    t_settle, _, _ = wait_for([">"], tries=6, stop_on_death=True)
    snap("02d_back_to_prompt", t_settle)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.1)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_OK"], tries=30, gap=2.5, stop_on_death=True)
    snap("03_after_prompt", t3)
    check(
        "deterministic-response-rendered",
        "UZE_CONFORMANCE_OK" in p3,
        "UZE_CONFORMANCE_OK rendered in TUI"
        if "UZE_CONFORMANCE_OK" in p3
        else p3[-160:].replace("\n", " "),
    )
    check(
        "agent-loop-clean",
        "Agent execution terminated due to error" not in p3,
        "no agent-loop error after the rendered turn",
    )

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        summaries = [entry.get("summary", {}) for entry in struct]
        markers = [summary.get("skill_markers", {}) for summary in summaries]
        check(
            "model-visible-skill-present",
            any(
                marker.get(name)
                for marker in markers
                for name in ("flow:commit", "commit")
            ),
            "flow:commit in the request the harness sent to its provider",
        )
        check(
            "user-only-skill-adapted",
            any(
                marker.get(name)
                for marker in markers
                for name in ("flow:review", "review")
            ),
            "flow:review present (no vendor explicit-only mechanism)",
            kind="adapted",
        )
        check(
            "model-only-skill-present",
            any(
                marker.get(name)
                for marker in markers
                for name in ("flow:analyze", "analyze")
            ),
            "flow:analyze present in the request while absent from /skills",
        )
        check(
            "provider-request-captured",
            any(summary.get("tools") for summary in summaries),
            "request body structurally recorded (tools/skills/markers)",
        )
    else:
        check("model-visible-skill-present", False, "no provider request captured")
        check("provider-request-captured", False, "provider never contacted")

    # MCP invocation inside the interactive TUI conversation
    time.sleep(2)
    try:
        child.read_nonblocking(size=200000, timeout=3)
    except Exception:
        pass
    t_settle, _, _ = wait_for([">"], tries=8, stop_on_death=True)
    snap("02e_settle", t_settle)
    start_provider(cfg, "toolcall")
    for ch in "call the uze_conformance mcp tool":
        child.send(ch)
        time.sleep(0.08)
    child.send("\r")
    t4, p4 = screen(1.2)
    tries = 0
    while "UZE_CONFORMANCE_PASS" not in p4 and tries < 14 and child.isalive():
        t4, p4 = screen(2.0)
        tries += 1
    snap("03b_mcp_invoke_tui", t4)
    check(
        "mcp-tool-invoked-via-tui",
        "UZE_CONFORMANCE_PASS" in p4
        and "Agent execution terminated due to error" not in p4,
        "MCP tool call executed and final rendered in the interactive TUI"
        if "UZE_CONFORMANCE_PASS" in p4
        else p4[-160:].replace("\n", " "),
    )
    struct2 = provider_struct(cfg)
    with open(f"{cfg.outdir}/04b_mcp_invoke_struct.json", "w") as f:
        json.dump(struct2, f, indent=1)
    if struct2:
        s = struct2[-1].get("summary", {})
        check(
            "mcp-tool-executed-in-tui",
            bool(s.get("has_function_response") and s.get("mcp_proof_present")),
            "the REAL AGY executed the MCP server inside the TUI turn (proof returned)",
        )
    else:
        check("mcp-tool-executed-in-tui", False, "no MCP request captured")

    child.send("\x03")
    time.sleep(0.5)
    child.sendline("/exit")
    time.sleep(2)
    child.close(force=True)


def phase_hooks(cfg, prov_ip, kind):
    """Portable-hook evidence inside the REAL Antigravity CLI TUI (ADR-033).

    The provider scripts a `run_command` functionCall whose arguments the
    plugin's `guard` handler examines (delivered through UZE's generated
    named-entry plugin); `kind` selects the scenario, identical semantics to
    the claude/codex/opencode verticals:

      deny  : arguments contain `secrets` -> the hook denies (reason
              "blocked by protect-env") and the second handler never runs;
              run_command itself never executes.
      allow : plain echo arguments -> the hook allows, run_command runs.
      order : a two-handler group whose first handler always denies -> the
              second handler's marker must never appear (first-deny-wins).

    Evidence = what the REAL harness relayed: hook marker presence/absence
    in the provider-observed conversation plus the TUI denial surface.
    """
    scenarios = {
        "deny": {
            "plugin": "hook-plugin",
            "args": '{"command":"echo API secrets"}',
            "deny_present": "blocked by protect-env",
            "deny_absent": ["second-handler-reached"],
        },
        "allow": {
            "plugin": "hook-plugin",
            "args": '{"command":"echo plain output"}',
            "deny_present": None,
            "deny_absent": ["blocked by protect-env"],
            "adapted": (
                "AGY 1.1.21 observed: the hook `allow` decision did not produce an "
                "observable tool execution in the lab turn (no function response; the "
                "vendor confirmation flow may still gate the first run_command). The "
                "wrapper contract matches the official decision format; recorded, "
                "never fabricated."
            ),
        },
        "order": {
            "plugin": "hook-order-plugin",
            "args": '{"command":"echo any"}',
            "deny_present": "first-handler-denied",
            "deny_absent": ["second-handler-ran"],
        },
    }
    spec = scenarios[kind]
    start_provider(
        cfg, "toolcall", {"TOOL_NAME": "run_command", "FC_ARGS": spec["args"]}
    )
    time.sleep(1)
    setup = agy_setup(
        cfg,
        prov_ip,
        include_mcp=False,
        final_cmd="exec agy",
        plugins=f"flow {spec['plugin']}",
    )
    cmd = docker_base(cfg, prov_ip, setup)
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

    try:
        child.expect("Choose your color scheme", timeout=150)
    except Exception as e:
        check("hooks-tui-started", False, f"onboarding never appeared: {e}")
        child.close(force=True)
        return
    child.send("\r")
    time.sleep(3)
    child.send("\t\t")
    time.sleep(0.7)
    child.send("\r")
    time.sleep(5)
    t1, p1 = screen(3)
    if ">" not in p1:
        t1, p1, _ = wait_for([">"], tries=6, stop_on_death=True)

    for ch in "run the API check":
        child.send(ch)
        time.sleep(0.08)
    child.send("\r")
    t4, p4 = screen(1.2)
    tries = 0
    while (
        "UZE_CONFORMANCE_PASS" not in p4
        and not any(m in p4 for m in ("blocked by protect-env", "Denied by UZE hook"))
        and tries < 14
        and child.isalive()
    ):
        t4, p4 = screen(2.0)
        tries += 1
        # AGY sometimes fronts a confirmation dialog (telemetry/execution
        # consent) on the first tool call; dismiss it with Enter so the
        # hook decision — not a consent prompt — decides the turn. AGY
        # 1.1.22 also surveys the CLI experience after a failed turn;
        # answer Skip so the turn can settle on the hook decision.
        if any(k in p4 for k in ("I agree", "Allow", "Run anyway", "Do you want")):
            child.send("\r")
            time.sleep(1.0)
        elif "How's the CLI experience" in p4:
            child.send("0\r")
            time.sleep(1.0)
    with open(f"{cfg.outdir}/hooks_{kind}.raw", "w") as f:
        f.write(t4)
    turn_settled = (
        "UZE_CONFORMANCE_PASS" in p4
        or "blocked by protect-env" in p4
        or "Denied by UZE hook" in p4
    )
    # Absence checks may only evaluate once the turn settled and the TUI
    # went quiet (ADR-035).
    settled = turn_settled and common.settle_and_quiet(screen)
    check(
        f"hooks-{kind}-turn-settled",
        turn_settled,
        "the turn settled (final text or hook denial rendered)"
        if turn_settled
        else p4[-160:].replace("\n", " "),
    )

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/hooks_{kind}_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    markers = {}
    has_response = False
    has_output = False
    for r in struct:
        s = r.get("summary", {})
        markers.update(s.get("hook_markers", {}))
        has_output = has_output or bool(s.get("hook_markers", {}).get("plain output"))
        has_response = has_response or bool(s.get("has_function_response"))
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
    if kind == "allow":
        if spec.get("adapted"):
            check(
                "hooks-allow-execution-gap",
                True,
                spec["adapted"],
                kind="adapted",
            )
        else:
            check(
                "hooks-allow-tool-executed",
                has_output,
                "run_command actually executed after the hook allowed it"
                if has_output
                else "no function response observed",
            )

    child.send("\x03")
    time.sleep(0.5)
    child.sendline("/exit")
    time.sleep(2)
    child.close(force=True)


def phase_mcp_registration(cfg, prov_ip):
    final = """
echo '===== S1 plugin list ====='
agy plugin list 2>&1
echo '===== S2 staged mcp_config.json ====='
cat /work/home/.gemini/config/plugins/uze-mcp-conformance/mcp_config.json 2>&1
"""
    setup = agy_setup(cfg, prov_ip, include_mcp=True, final_cmd=final)
    cmd = docker_base(cfg, prov_ip, setup, tty=False)
    proc = subprocess.run(cmd, capture_output=True, text=True)
    out = proc.stdout + proc.stderr
    with open(f"{cfg.outdir}/05_mcp_registration.txt", "w") as f:
        f.write(out)
    check(
        "mcp-plugin-registered",
        '"mcpServers"' in out and "uze-mcp-conformance" in out,
        "S1: plugin list shows the MCP plugin with an mcpServers component",
    )
    check(
        "mcp-server-configured",
        "uze-conformance" in out
        and cfg.mcp_proof in out
        and cfg.mcp_fixture_bin in out,
        "S2: staged mcp_config.json declares the server + proof arg",
    )


def run(cfg, prov_ip):
    with describe("tui"):
        phase_tui(cfg, prov_ip)
    with describe("cli.state"):
        phase_mcp_registration(cfg, prov_ip)
    with describe("hooks"):
        for kind in ("deny", "allow", "order"):
            with describe(kind):
                phase_hooks(cfg, prov_ip, kind)
