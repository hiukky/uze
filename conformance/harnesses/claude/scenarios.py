#!/usr/bin/env python3
import json

"""Claude Code scenario (latest channel) — Real Harness + Synthetic World.

Phase A (TUI): onboarding drive -> prompt; /plugin, /mcp (server connected,
1 tool), deterministic turn, model-visible skill present and user-only skill
ABSENT from the PRIMARY model request (disable-model-invocation — genuine
policy preservation), MCP registration + connection via /mcp.

Preflight: TLS interception of the hardcoded Anthropic hosts via /etc/hosts
+ injected CA (NODE_EXTRA_CA_CERTS). `ANTHROPIC_BASE_URL` is ignored by the
interactive TUI — the TLS interception is the required hook.
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


def claude_setup(cfg, prov_ip, final_cmd, plugins="flow mcp-plugin"):
    return f"""
set -e
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin
export HOME=/work/home CLAUDE_CONFIG_DIR=/work/home/.claude UZE_HOME=/work/home/.uze
export ANTHROPIC_API_KEY=uze-conformance-invalid-by-design
export ANTHROPIC_BASE_URL=https://api.anthropic.com
export NODE_EXTRA_CA_CERTS=/app/ca.crt
mkdir -p /work/home/.claude
cp /app/fixtures/claude.json /work/home/.claude.json
{materialize_marketplace(cfg)}
uze market add /work/market >/dev/null 2>&1
for p in {plugins}; do uze plugin install $p@uze-lab >/dev/null 2>&1; done
{final_cmd}
"""


def claude_container(cfg, prov_ip, final_cmd, plugins="flow mcp-plugin"):
    cmd = docker_base(
        cfg, prov_ip, claude_setup(cfg, prov_ip, final_cmd, plugins=plugins)
    )
    ca_crt, _, _ = generate_certs(cfg)
    i = cmd.index(common.HARNESS_IMAGE)
    cmd = (
        cmd[:i]
        + [
            "-v",
            f"{ca_crt}:/app/ca.crt:ro",
            "-e",
            "CLAUDE_CONFIG_DIR=/work/home/.claude",
        ]
        + cmd[i:]
    )
    return cmd


def drive_onboarding(child):
    """Dialogs: Welcome/Security guide (2.1.250 first-run) -> API key (Yes)
    -> theme -> security notes -> trust (Yes) -> Tips/What's-new popup ->
    prompt. Returns (screen, plain, marker).

    Every match here is space-insensitive (`squash`): 2.1.260 paints the
    first-run screens with cursor-forward moves instead of spaces, so a
    marker spelled the way a person reads it ("Yes, I trust this folder")
    is absent from the transcript while being plainly on screen. Matching
    on the spelled form let the trust screen fall through to the generic
    "❯" match, which returned as if the prompt were up — and the next
    Enter landed on "No, exit" and quit the TUI, taking all 24 checks with
    it."""
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    DIALOGS = [
        "Welcome to Claude Code",
        "Security guide",
        "Yes, I trust this folder",
        "Detected a custom API key",
        "theme",
        "Security notes",
        "Quick safety check",
        "Accessing workspace",
        "login method",
        "Opus5",
        "Opus",
        "API Usage Billing",
        "❯",
    ]

    def prompt_is_up(t, p):
        j = common.squash(p)
        return bool(t) and ("Opus" in j or "APIUsageBilling" in j)

    t, p, m = wait_for(DIALOGS, tries=16, stop_on_death=True, squash_spaces=True)
    for i in range(10):
        j = common.squash(p)
        if m == "Detected a custom API key":
            child.send("\x1b[A")
            time.sleep(0.3)
            child.send("\r")  # Yes
        elif m in ("theme", "Security notes"):
            child.send("\r")
        elif m in (
            "Welcome to Claude Code",
            "Security guide",
            "Quick safety check",
            "Accessing workspace",
        ):
            # 2.1.250 first-run: the folder-trust screen defaults to
            # "❯ No, exit" — plain Enter would quit the TUI. Select
            # "Yes, I trust this folder" first.
            if "No,exit" in j and "Yes,Itrust" in j:
                child.send("\x1b[B")
                time.sleep(0.3)
            child.send("\r")
        elif "Yes,Itrustthisfolder" in j and "No,exit" in j:
            # 2.1.250: keep "Yes" selected (down from "No, exit") before
            # confirming, and re-confirm if the guide stays up.
            if "❯No,exit" in j:
                child.send("\x1b[B")
                time.sleep(0.3)
            child.send("\r")
        elif m == "login method":
            child.send("\x1b[B")
            time.sleep(0.3)
            child.send("\r")
        else:
            break
        t, p, m = wait_for(DIALOGS, tries=14, stop_on_death=True, squash_spaces=True)
        if m in ("Opus5", "Opus", "API Usage Billing", "❯"):
            break
        # The 2.1.250 prompt frame carries the model/status bar content;
        # "❯" alone is invalid here (it appears on every option screen).
        if prompt_is_up(t, p):
            break
    # The Tips/What's-new overlay popup does not block the prompt; the first
    # slash-command keystroke dismisses it. Never send Esc here (Esc on an
    # empty prompt exits the TUI). 2.1.250 labels the model "Opus 5 (1M
    # context)" and paints the prompt once — the last good frame may carry
    # it while the matched marker was an earlier dialog, so return on
    # CONTENT (Opus/status bar/prompt arrow), never on a stale or empty
    # read.
    for _ in range(10):
        if prompt_is_up(t, p):
            return t, p, m
        t, p, m = wait_for(
            ["Opus5", "Opus", "API Usage Billing", "❯"],
            tries=4,
            gap=2.0,
            stop_on_death=True,
            squash_spaces=True,
        )
    return t, p, m


def phase_tui(cfg, prov_ip):
    cmd = claude_container(cfg, prov_ip, "exec claude")
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

    t, p, m = drive_onboarding(child)
    # 2.1.250 renders the prompt in spaced bursts; a race can hand the
    # phase an empty first frame even though the prompt is up. Re-wait for
    # a real prompt frame before snapshotting/asserting.
    if not t or ("Opus" not in p and "API Usage Billing" not in p):
        t, p, m = wait_for(
            ["Opus", "API Usage Billing", "❯"], tries=8, gap=2.0, stop_on_death=True
        )
        if not t:
            t, p, m = wait_for(["❯"], tries=4, gap=2.0, stop_on_death=True)
    snap("01_prompt", t)
    joined = p.replace(" ", "")
    check(
        "tui-reached-prompt",
        ("Opus" in joined and "❯" in p) or ("APIUsageBilling" in joined and "❯" in p),
        "claude TUI reached its prompt"
        if "Opus" in joined or "APIUsageBilling" in joined
        else p[-120:].replace("\n", " "),
    )
    check(
        "synthetic-credential",
        "APIUsageBilling" in joined,
        "billing row shows API usage (API key mode)",
    )

    # /plugin
    for ch in "/plugin":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t, p, m = wait_for(["Installed", "Plugins"], tries=8, stop_on_death=True)
    snap("02_plugin", t)
    check(
        "plugin-surface-in-tui",
        "Plugins" in p and "Installed" in p,
        "/plugin opens the plugins surface",
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /agents is Claude's native custom-subagent manager. This proves the
    # harness consumes the canonical Agent, not merely that UZE wrote a path.
    for ch in "/agents":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t, p, _ = wait_for(["reviewer", "Agents"], tries=10, stop_on_death=True)
    snap("02a_agents", t)
    check(
        "agent-visible-in-tui",
        "reviewer" in p,
        "Claude /agents lists the UZE reviewer agent"
        if "reviewer" in p
        else p[-200:].replace("\n", " "),
    )
    child.send("\x1b")
    time.sleep(1.0)

    # /mcp
    for ch in "/mcp":
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t, p, m = wait_for(["connected", "tool"], tries=10, stop_on_death=True)
    snap("02b_mcp", t)
    joined = p.replace(" ", "")
    check(
        "mcp-server-connected-in-tui",
        "connected" in joined and "1tool" in joined,
        "/mcp shows the UZE MCP server connected with 1 tool",
    )
    child.send("\x1b")
    time.sleep(1.0)

    # deterministic turn
    for ch in "hi":
        child.send(ch)
        time.sleep(0.07)
    child.send("\r")
    t3, p3, _ = wait_for(["UZE_CONFORMANCE_OK"], tries=20, gap=2.5, stop_on_death=True)
    snap("03_after_prompt", t3)
    check(
        "deterministic-response-rendered",
        "UZE_CONFORMANCE_OK" in p3,
        "UZE_CONFORMANCE_OK rendered in TUI"
        if "UZE_CONFORMANCE_OK" in p3
        else p3[-160:].replace("\n", " "),
    )

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/04_provider_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    if struct:
        # The model-facing contract is the PRIMARY request (the one carrying
        # the Skill tool). Auxiliary no-tools calls (title/context) may
        # include the full skill listing — a documented secondary leak, never
        # the primary contract.
        primary = [
            r for r in struct if "Skill" in r.get("summary", {}).get("tools", [])
        ]
        base = primary or struct
        markers = common.observed_markers(base, "skill_markers")
        check(
            "model-visible-skill-present",
            any(markers.get(m) for m in ("flow:commit", "commit")),
            "flow:commit in the primary request claude sent to its provider",
        )
        check(
            "user-only-skill-hidden",
            not any(markers.get(m) for m in ("flow:review", "Review code")),
            "flow:review absent from the primary model request (disable-model-invocation preserved)",
        )
        check(
            "provider-request-captured",
            any(r.get("summary", {}).get("tools") for r in struct),
            "request body structurally recorded (tools/skill markers)",
        )
    else:
        check("model-visible-skill-present", False, "no provider request captured")
        check("provider-request-captured", False, "provider never contacted")

    # MCP execution inside the conversation: PARTIAL — claude defers MCP tools
    # behind ToolSearch (deferred-tool protocol); a direct mcp__ tool_use fails
    # with "No such tool available". Registration + connection are proven via
    # the /mcp TUI surface. Documented, never a pass.
    child.send("\x03")
    time.sleep(0.6)
    child.send("\x03")
    time.sleep(0.6)
    child.close(force=True)


def phase_hooks(cfg, prov_ip, kind):
    """Portable-hook evidence inside the REAL Claude Code TUI (ADR-033).

    The provider scripts a `Bash` tool_use whose input the hook guard
    examines; `kind` selects the scenario:

      deny  : input contains `secrets` -> the hook denies (reason
              "blocked by protect-env") and the second handler never runs;
              the Bash command itself never executes.
      allow : input is a plain echo -> the hook allows, the real Bash runs.
      order : a two-handler group whose first handler always denies -> the
              second handler's marker must never appear (first-deny-wins).

    Evidence = what the REAL harness relayed: hook marker presence/absence
    in the provider-observed conversation plus the TUI denial surface.

    Two vacuities hid here until 2026-09-02, and both are why the presence
    check `hooks-*-denial-relayed` gates the absence checks: the provider
    once put the tool input on `content_block_start`, which Claude Code
    ignores in favour of `input_json_delta`, so every scripted `Bash` call
    was rejected as `Invalid tool parameters` before a hook ran; and the
    marker aggregation was last-write-wins over every provider request, so
    a trailing telemetry batch erased the denial the model call carried.
    """
    scenarios = {
        "deny": {
            "plugin": "hook-plugin",
            "args": '{"command":"echo API secrets"}',
            "prompt": "run the API check",
            "tui_markers": ["blocked by protect-env"],
            "deny_present": "blocked by protect-env",
            "deny_absent": ["second-handler-reached"],
            # The vocabulary row the harness delivered: the guard echoes the
            # portable alias it was handed, so the relayed reason proves the
            # handler read `shell` and this harness's own command field.
            "context_present": "tool=shell",
        },
        "allow": {
            "plugin": "hook-allow-plugin",
            "args": '{"command":"echo plain output"}',
            "prompt": "run the API check",
            "tui_markers": [],
            "deny_present": None,
            "deny_absent": ["blocked by protect-env"],
        },
        "order": {
            "plugin": "hook-order-plugin",
            "args": '{"command":"echo any"}',
            "prompt": "run the API check",
            "tui_markers": ["first-handler-denied"],
            "deny_present": "first-handler-denied",
            "deny_absent": ["second-handler-ran"],
        },
    }
    spec = scenarios[kind]
    common.start_provider(
        cfg, "toolcall", {"TOOL_NAME": "Bash", "TOOL_ARGS": spec["args"]}
    )
    time.sleep(1)
    cmd = claude_container(
        cfg,
        prov_ip,
        "exec claude",
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

    t, p, m = drive_onboarding(child)
    for ch in spec["prompt"]:
        child.send(ch)
        time.sleep(0.06)
    child.send("\r")
    t3, p3, m3 = wait_for(
        ["UZE_CONFORMANCE_PASS"] + spec["tui_markers"],
        tries=24,
        gap=2.5,
        squash_spaces=True,
    )
    # Absence checks below may only evaluate once the turn settled AND the
    # TUI went quiet (ADR-035): "never appeared" is not provable while the
    # surface can still be rendering.
    settled = m3 is not None and common.settle_and_quiet(screen)
    snap = f"{cfg.outdir}/hooks_{kind}.raw"
    with open(snap, "w") as f:
        f.write(t3)
    check(
        f"hooks-{kind}-turn-settled",
        m3 is not None,
        "the turn settled (final text or hook denial rendered)"
        if m3 is not None
        else p3[-160:].replace("\n", " "),
    )

    struct = provider_struct(cfg)
    with open(f"{cfg.outdir}/hooks_{kind}_struct.json", "w") as f:
        json.dump(struct, f, indent=1)
    markers = common.observed_markers(struct, "hook_markers")
    has_output = bool(markers.get("plain output"))
    has_tool_result = any(r.get("summary", {}).get("has_tool_result") for r in struct)
    if spec["deny_present"]:
        # The denial reason relayed to the model is the evidence that the
        # hook ran and Claude honored it. Without it the absence checks
        # below hold for a turn where no hook ran at all.
        relayed = bool(markers.get(spec["deny_present"]))
        check(
            f"hooks-{kind}-denial-relayed",
            relayed,
            f"`{spec['deny_present']}` reached the conversation as the tool outcome"
            if relayed
            else ", ".join(f"{m}={markers.get(m)}" for m in sorted(markers)),
        )
        common.check_absence(
            f"hooks-{kind}-denial-blocks-tool",
            relayed and not has_output,
            settled,
            "the intercepted tool never executed — the native denial blocked it"
            if not has_output
            else "the tool executed despite the deny — blocking is broken",
        )
    if spec.get("context_present"):
        carried = bool(markers.get(spec["context_present"]))
        check(
            f"hooks-{kind}-context-relayed",
            carried,
            f"the handler read the portable vocabulary (`{spec['context_present']}`) "
            f"from this harness's own payload"
            if carried
            else ", ".join(f"{m}={markers.get(m)}" for m in sorted(markers)),
        )
    for absent in spec["deny_absent"]:
        common.check_absence(
            f"hooks-{kind}-marker-absent-{absent}",
            not markers.get(absent, False),
            settled,
            f"`{absent}` never reached the conversation (first-deny-wins)",
        )
    if kind == "allow":
        # A tool_result alone is not execution: a rejected call or a hook
        # error answers with one too. Only the command's own stdout marker
        # in the conversation proves Bash ran.
        check(
            "hooks-allow-tool-executed",
            has_output,
            "the Bash tool actually executed after the hook allowed it"
            if has_output
            else "no tool_result observed"
            if not has_tool_result
            else "a tool_result arrived without the command's stdout",
        )
    child.send("\x03")
    time.sleep(0.6)
    child.send("\x03")
    time.sleep(0.6)
    child.close(force=True)


def run(cfg, prov_ip):
    with describe("tui"):
        phase_tui(cfg, prov_ip)
    with describe("hooks"):
        for kind in ("deny", "allow", "order"):
            with describe(kind):
                phase_hooks(cfg, prov_ip, kind)
