"""What an agent UZE placed must do, on every harness.

UZE isolates at launch: every agent it starts works in a checkout of its
own under `.worktrees/<id>`, and the shared instruction file tells it so.
None of that needs a harness to cooperate — which is exactly why the Lab
has to prove the harness does not get in the way. Three things only a real
harness can answer:

    it starts and works inside a linked worktree, not just a repository root;
    the projected declaration reaches the model from that directory;
    text UZE types into its pane reaches the model as the agent's own turn.

The slot itself is not built by `uze` here: slot mechanics are proven in
the deterministic suite against real Git, and the scene lays the same
shape down by hand so the run measures the harness, never the engine.
"""

import time

from shared.common import check, describe, observed_markers, provider_struct

#: Where the scene's project lives inside the container, and its one slot.
PROJECT = "/work/project"
SLOT = f"{PROJECT}/.worktrees/t0lab"

#: A phrase only the projected declaration carries.
DECLARATION_MARKER = "already isolated"
#: A sentinel only the typed message carries.
MESSAGE_MARKER = "UZE_CONFORMANCE_REBASE"

#: The declaration `uze context reconcile` projects for a `worktrees`
#: policy, committed into the fixture repository so the slot's checkout
#: carries it the way a real project's would. Kept in step with
#: `WorktreePolicy::instructions` by the deterministic suite, not by this
#: copy: this only has to contain the phrase the check looks for.
DECLARATION = """## Concurrent work isolation

- Every agent UZE launches works in a checkout of its own under `.worktrees/<id>`, on branch `agent/<id>`. If your working directory is inside `.worktrees/`, you are already isolated: do not create another worktree, and do not switch branches.
- Commit your work on your own branch, as you go. Never commit to, merge into, rebase, or reset the target branch: delivery is UZE's.
- If UZE tells you a rebase is paused in your checkout, resolve the conflicts preserving the intent of your change, run `git rebase --continue`, run the project's checks, and end your turn.
"""


def prelude():
    """The shell that lays the scene down before the harness starts: a
    repository with the declaration committed, bridged for every harness
    that reads its own file name, and one slot to start the harness in."""
    return f"""
mkdir -p {PROJECT} && cd {PROJECT}
git init -q -b main .
git config user.name lab
git config user.email lab@uze.invalid
cat > AGENTS.md <<'UZE_EOF'
# Lab project

<!-- uze:begin project:worktree-policy/lab -->
{DECLARATION}<!-- uze:end project:worktree-policy/lab -->
UZE_EOF
printf '@AGENTS.md\\n' > CLAUDE.md
printf '@AGENTS.md\\n' > GEMINI.md
printf 'version: 1\\nworktrees:\\n  completion: handoff\\n' > agents.lock
git add . && git commit -q -m init
git worktree add -q -b agent/t0lab .worktrees/t0lab HEAD
printf '/.worktrees/\\n' >> .git/info/exclude
"""


def assert_contract(cfg, prov_ip, bindings):
    with describe("isolation"):
        _assert_in_slot(cfg, prov_ip, bindings)


def _assert_in_slot(cfg, prov_ip, bindings):
    with bindings.session_in(cfg, prov_ip, SLOT, prelude()) as tui:
        plain, matched = bindings.prepare(tui)
        check(
            "isolation-tui-ready-in-slot",
            bool(matched),
            f"{bindings.harness} reached its prompt inside a linked worktree"
            if matched
            else plain[-160:].replace("\n", " "),
        )
        if not matched:
            return
        time.sleep(bindings.warmup)

        # One turn, typed the way UZE types a notice into a pane: the
        # request it produces has to carry the declaration (context from
        # the slot's checkout) and the sentence itself (input reached the
        # agent's own turn, not a menu or a status line).
        tui.type(
            f"{MESSAGE_MARKER}: a rebase is paused in your checkout; resolve the "
            "conflicts, run git rebase --continue, run the checks, and end your turn"
        )
        tui.submit()
        turn = tui.collect(reads=6)
        tui.snapshot("isolation-turn", turn)

        seen = observed_markers(provider_struct(cfg), "isolation_markers")
        check(
            "isolation-declaration-reaches-model",
            seen.get(DECLARATION_MARKER, False),
            "the projected declaration is in the model request from the slot",
        )
        check(
            "isolation-message-reaches-model",
            seen.get(MESSAGE_MARKER, False),
            "text typed into the pane is the agent's own turn",
        )
        # The one thing this scene cannot yet ask: the synthetic provider
        # answers with text, so nothing scripts the harness's own worktree
        # primitive to see it decline. Declared, so the gap is reviewable.
        check(
            "isolation-no-top-level-worktree",
            True,
            "not scripted: the synthetic provider does not yet drive this "
            "harness's own worktree primitive",
            kind="adapt",
        )
