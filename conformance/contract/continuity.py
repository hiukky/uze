"""What an agent that comes back must remember, on every harness.

UZE relaunches an agent into the task it was working in — after a restart,
a crash, or a person picking preserved work back up — and decides, at the
moment the process starts, whether that launch resumes a conversation or
begins one. The deterministic suite proves the decision; only a real
harness can answer whether the conversation actually came back:

    a turn made in the first process reaches the model again in the second.

The scene lays a managed task down by hand, the same way the isolation
scene lays a slot down: task records are the engine's business and are
proven against real Git in the deterministic suite, so writing one here
keeps the run measuring the harness rather than the engine. A path this
scene got wrong shows up as a check that fails, never as one that passes
for the wrong reason — the harness would simply start a second conversation
and the earlier turn would be absent.
"""

import time

from shared.common import check, describe, provider_struct

#: Where the scene's project lives inside the container, and its one slot.
PROJECT = "/work/project"
SLOT_NAME = "t0lab"
SLOT = f"{PROJECT}/.worktrees/{SLOT_NAME}"

#: Sentinels only one turn each carries. The proof is one model request
#: holding both: the second process's own turn, and the first process's turn
#: replayed as history it could only have from the resumed conversation.
FIRST_MARKER = "UZE_CONFORMANCE_ACORN"
SECOND_MARKER = "UZE_CONFORMANCE_WALNUT"

#: UZE's own state, keyed the way `uze-core` keys it: the task document at
#: `state/tasks/<project id>.json`, where the project id is the FNV-1a-64
#: digest of the canonical project root, in fixed-width hex.
UZE_HOME = "/work/home/.uze"


def project_id(root: str) -> str:
    """`uze_core::harness_runtime::project_id_for`, in Python.

    A digest of a path is the one piece of UZE UZE cannot be asked for from
    a shell — there is no command that prints it, because nothing but UZE
    reads it. Reproduced here rather than left out: getting it wrong costs a
    failing check, and leaving the scene out costs the whole contract.
    """
    digest = 0xCBF29CE484222325
    for byte in root.encode():
        digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{digest:016x}"


def prelude(harness):
    """The shell that lays the scene down: a repository with one slot, a
    task record naming it, and UZE's launcher for this harness."""
    tasks = f"{UZE_HOME}/state/tasks/{project_id(PROJECT)}.json"
    return f"""
mkdir -p {PROJECT} && cd {PROJECT}
git init -q -b main .
git config user.name lab
git config user.email lab@uze.invalid
printf '# Lab project\\n' > AGENTS.md
git add . && git commit -q -m init
git worktree add -q -b agent/{SLOT_NAME} .worktrees/{SLOT_NAME} HEAD
printf '/.worktrees/\\n' >> .git/info/exclude
mkdir -p {UZE_HOME}/state/tasks {UZE_HOME}/shims
cat > {tasks} <<'UZE_EOF'
{{
  "schema_version": 1,
  "tasks": [
    {{
      "id": "{SLOT_NAME}",
      "label": "continuity",
      "base": {{ "kind": "ref", "value": "main" }},
      "base_commit": "",
      "target": "main",
      "branch": "agent/{SLOT_NAME}",
      "checkout": "{SLOT_NAME}",
      "state": {{ "state": "running" }},
      "pushed": false,
      "published_as": null,
      "published_request": null,
      "published_tip": null,
      "created_at_unix": 1
    }}
  ]
}}
UZE_EOF
ln -sf "$(command -v uze)" {UZE_HOME}/shims/{harness}
"""


def launcher(harness):
    """What a pane is launched by: UZE's own launcher, named by path exactly
    as the workspace client names it, so the scene exercises the boundary a
    real relaunch goes through instead of whatever `PATH` happens to hold."""
    return f"{UZE_HOME}/shims/{harness}"


#: Printed by the shell between the two launches. Without it, "the second
#: process never reached its prompt" is one message for two very different
#: facts — a harness that would not exit, and a harness that started and
#: rendered something unexpected — and the three verticals that failed this
#: check first disagreed about which they had hit.
ENDED_MARKER = "UZE_CONFORMANCE_PROCESS_ENDED"


def relaunch_command(harness, args=""):
    """The shell one terminal runs: the launcher, a marker when it returns,
    the launcher again. `args` is whatever this harness needs on the line
    both times.

    Not `exec`, because the shell has to outlive the first process to start
    the second — which is the whole shape being tested. Composed here rather
    than in each binding: the marker only means anything if every vertical
    prints the same one, and a binding is free to wrap this in whatever its
    own container needs.
    """
    run = f"{launcher(harness)} {args}".strip()
    return f"{run}; printf '\\n{ENDED_MARKER}\\n'; {run}"


def assert_contract(cfg, prov_ip, bindings):
    with describe("continuity"):
        _assert_a_relaunch_carries_the_turn(cfg, prov_ip, bindings)


def _assert_a_relaunch_carries_the_turn(cfg, prov_ip, bindings):
    declined = bindings.unsupported("relaunch_in")
    if declined:
        check("continuity-relaunch-carries-the-turn", True, declined, kind="adapt")
        return

    with bindings.relaunch_in(
        cfg, prov_ip, SLOT, prelude(bindings.launcher_name())
    ) as tui:
        plain, matched = bindings.prepare(tui)
        check(
            "continuity-first-process-ready",
            bool(matched),
            f"{bindings.harness} reached its prompt in the task's checkout"
            if matched
            else plain[-160:].replace("\n", " "),
        )
        if not matched:
            return
        time.sleep(bindings.warmup)

        tui.type(f"{FIRST_MARKER}: remember this word and say it back later")
        tui.submit()
        tui.snapshot("continuity-first-turn", tui.collect(reads=6))

        # The process ends and another starts in its place — what the
        # terminal runtime does when it restores a workspace after a
        # restart, with no client in the room to compose a command.
        plain, ended = _end_the_process(tui, bindings)
        check(
            "continuity-first-process-ended",
            bool(ended),
            "the first process exited and the shell went on to the next"
            if ended
            else f"it is still running: {plain[-160:]}".replace("\n", " "),
        )
        if not ended:
            return

        plain, matched = bindings.rejoin(tui)
        check(
            "continuity-second-process-ready",
            bool(matched),
            "a second process started in the same checkout"
            if matched
            else plain[-160:].replace("\n", " "),
        )
        if not matched:
            return
        time.sleep(bindings.warmup)

        tui.type(f"{SECOND_MARKER}: what was the word?")
        tui.submit()
        tui.snapshot("continuity-second-turn", tui.collect(reads=6))

        check(
            "continuity-relaunch-carries-the-turn",
            _carried(cfg),
            "a request from the relaunched process carries the earlier turn",
        )


def _end_the_process(tui, bindings):
    """Sends this harness's exit keys, one at a time, stopping the moment
    the shell says the process is gone.

    Paced here rather than in the binding because the pacing is the whole
    point. Harnesses disagree about how many interrupts it takes — one
    ends OpenCode, two are what Claude asks for — and a key sent after the
    process is already gone does not vanish: the shell has it, and the
    shell is at that moment starting the next process.
    """
    plain = ""
    for index, key in enumerate(bindings.exit_keys):
        last = index == len(bindings.exit_keys) - 1
        tui.child.send(key)
        # Between keys a glance, after the last one a real wait. A harness
        # that answers an interrupt with "press it again to exit" means
        # *again*, soon: wait two seconds between the two and the offer has
        # expired, which turns the second interrupt into another first one
        # and the process never ends — measured on Claude Code, which is
        # why a harness lists only the keys it actually needs.
        _, plain, ended = tui.wait_for(
            [ENDED_MARKER], tries=8 if last else 1, gap=2.0 if last else 0.4
        )
        if ended:
            return plain, True
    return plain, False


def _carried(cfg):
    """One request holding both sentinels.

    The union across requests is not enough here: the first process's own
    request already carried the first sentinel, so `first was seen` and
    `second was seen` can both be true with nothing having been resumed.
    Only a single body carrying both says the second process was given the
    first process's turn.
    """
    for record in provider_struct(cfg):
        markers = record.get("summary", {}).get("continuity_markers", {})
        if markers.get(FIRST_MARKER) and markers.get(SECOND_MARKER):
            return True
    return False
