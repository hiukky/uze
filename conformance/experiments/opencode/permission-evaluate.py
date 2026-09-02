"""Does OpenCode's `permission.evaluate` carry the intercepted tool's input?

The generated OpenCode plugin can observe a tool call but cannot block it:
V2's tool hooks see the input and have no block signal, and the only
decision point — `ctx.permission.hook("evaluate")` — carries an `action`
and its `resources`, not the tool input. The portable `deny`/`ask` effects
are therefore declared Unsupported on OpenCode. Whether that is permanent
depends on one fact this experiment measures rather than assumes: **does
`resources` carry the command line for the shell action?**

A probe plugin (nothing to do with the delivered artifact) records every
`permission.evaluate` event verbatim and every `execute.before` event's
tool name, then a real turn runs. Evidence is the recorded event, not a
reading of the docs.

  yes -> the deny/ask ADAPTED entries on OpenCode can retire, and the
         generated plugin gains a permission.evaluate branch.
  no  -> the declarations stand, with the observed event shape as their
         reason.

Run: python3 conformance/lab.py --harness opencode \
       --experiment opencode/permission-evaluate
"""

import json
import os
import subprocess
import time

import pexpect

from harnesses.opencode.scenarios import (
    make_screen,
    make_waiter,
    opencode_container,
)
from shared import common
from shared.common import check

# The probe registers both V2 hook points and appends one JSON line per
# event to a file the container's final command prints back.
PROBE = r"""
mkdir -p /work/home/.config/opencode/plugins
cat > /work/home/.config/opencode/plugins/permission-probe.ts <<'PROBE_EOF'
import { Plugin } from "@opencode-ai/plugin";
const LOG = "/tmp/permission-events.jsonl";
async function record(kind, event) {
  try {
    await Bun.write(
      LOG,
      (await Bun.file(LOG).text().catch(() => "")) +
        JSON.stringify({ kind, event }) + "\n",
    );
  } catch (error) {
    console.error("probe failed", error);
  }
}
export default Plugin.define({
  id: "permission-probe",
  async setup(ctx) {
    if (ctx.permission?.hook) {
      await ctx.permission.hook("evaluate", async (event) => {
        await record("permission.evaluate", event);
      });
    } else {
      await record("permission.evaluate", { unavailable: true });
    }
    await ctx.tool.hook("execute.before", async (event) => {
      await record("tool.execute.before", { tool: event.tool, input: event.input });
    });
  },
});
PROBE_EOF
"""


def run(cfg, prov_ip):
    mcp_tool = "uze-mcp-conformance-uze-conformance_uze_conformance"
    common.start_provider(
        cfg,
        "toolcall",
        {
            "TOOL_NAME": mcp_tool,
            "TOOL_ARGS": '{"serverName":"uze-conformance","toolName":"uze_conformance",'
            '"arguments":{"command":"echo API secrets"}}',
        },
    )
    time.sleep(1)
    cmd = opencode_container(
        cfg,
        prov_ip,
        PROBE + "UZE_HOME=/usr/local/.uze PATH=/usr/local/.uze/shims:$PATH "
        "exec opencode --standalone",
        plugins="flow mcp-plugin hook-plugin",
    )
    # Named so the probe's log can be read from outside while the session
    # is alive: the container dies with the TUI.
    container = f"permission-evaluate-{os.getpid()}"
    cmd[2:2] = ["--name", container]
    child = pexpect.spawn(
        cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", timeout=300
    )
    child.setwinsize(50, 160)
    try:
        child.logfile_read = common.CastRecorder(cfg.outdir, "tui-permission-evaluate")
    except Exception:
        pass
    screen = make_screen(child)
    wait_for = make_waiter(screen)
    _, tail, marker = wait_for(["Ask anything"], tries=16, stop_on_death=True)
    check(
        "permission-tui-reached-prompt",
        marker == "Ask anything",
        "opencode TUI reached its prompt"
        if marker == "Ask anything"
        else tail[-120:].replace("\n", " "),
    )
    # The prompt renders long before plugin loading finishes (observed in
    # the canonical vertical); the same warmup, or typed input is lost.
    time.sleep(25)
    for character in "run the API check":
        child.send(character)
        time.sleep(0.04)
    time.sleep(1)
    child.send("\r")
    screenful, tail, marker = wait_for(
        ["UZE_CONFORMANCE_PASS", "denied", "blocked by protect-env"],
        tries=24,
        gap=2.5,
    )
    with open(f"{cfg.outdir}/permission_evaluate.raw", "w") as handle:
        handle.write(screenful)
    common.settle_and_quiet(screen)

    events = read_events(container)
    with open(f"{cfg.outdir}/permission_events.json", "w") as handle:
        json.dump(events, handle, indent=1)
    evaluations = [e for e in events if e.get("kind") == "permission.evaluate"]
    check(
        "permission-evaluate-fired",
        bool(evaluations),
        f"{len(evaluations)} permission.evaluate event(s) recorded"
        if evaluations
        else "the permission hook never fired for this turn — no decision point exists here",
    )
    carries_command = any(
        "echo API secrets" in json.dumps(entry.get("event", {}))
        for entry in evaluations
    )
    check(
        "permission-evaluate-carries-the-tool-input",
        carries_command,
        "the event carries the intercepted call's own arguments — deny/ask are expressible"
        if carries_command
        else "no recorded permission.evaluate event carries the call's arguments: "
        + json.dumps([entry.get("event") for entry in evaluations])[:400],
    )
    child.send("\x03")
    time.sleep(0.6)
    child.close(force=True)


def read_events(container):
    """Reads the probe's log out of the container the session ran in."""
    probe = subprocess.run(
        ["docker", "exec", container, "cat", "/tmp/permission-events.jsonl"],
        capture_output=True,
        text=True,
    )
    events = []
    for line in probe.stdout.splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return events
