"""How OpenCode is driven. No assertions live here."""

import time

from contract import continuity
from contract.bindings import Bindings
from contract.tui import Tui

from .scenarios import opencode_container


class OpenCodeBindings(Bindings):
    harness = "opencode"
    launch = "UZE_HOME=/usr/local/.uze PATH=/usr/local/.uze/shims:$PATH exec opencode --standalone"
    ready_markers = ("Ask anything",)
    #: One interrupt ends it — measured (`experiments/relaunch_probe`). A
    #: second would not be spare: the process is already gone by then, so
    #: the shell has it, and the shell is starting the next process.
    exit_keys = ("\x03",)
    #: What a *resumed* session shows. `Ask anything` is the empty prompt's
    #: placeholder, and a session opened with earlier turns in it has no
    #: empty prompt to place a holder in — measured on beta-19192
    #: (`experiments/relaunch_probe`): the second process rendered its
    #: status bar and nothing else this vertical was looking for.
    rejoin_markers = ("Build", "UZE Conformance Model")
    #: The prompt renders long before the skill and MCP surfaces finish
    #: loading, and input typed into that window is dropped. Measured, not
    #: guessed: 25s is what a working manual probe needed.
    warmup = 25.0

    def session(self, cfg, prov_ip):
        return Tui(
            cfg, opencode_container(cfg, prov_ip, self.launch), "opencode-contract"
        )

    def session_in(self, cfg, prov_ip, cwd, prelude):
        final = f"{prelude}\ncd {cwd} && {self.launch}"
        return Tui(cfg, opencode_container(cfg, prov_ip, final), "opencode-isolation")

    #: The `opencode` this scene's launches resolve to.
    #:
    #: Every other scene here launches `opencode --standalone`, because
    #: this container has no background service for a TUI to attach to.
    #: This one cannot: an argument on the command line is an argument the
    #: *caller* composed, and that is precisely the case where UZE
    #: contributes nothing — measured, the launch went bare and no
    #: conversation was ever recorded. So the flag goes behind the
    #: launcher rather than in front of it, where it is the container's
    #: business and the launch stays bare.
    #:
    #: A subcommand is passed through untouched: UZE reads this harness's
    #: session listing with `api GET /api/session`, and that one does want
    #: the service every other process is talking to.
    STANDALONE_WRAPPER = """
mkdir -p /tmp/lab-bin
cat > /tmp/lab-bin/opencode <<'WRAP_EOF'
#!/bin/sh
real=$(command -v opencode2)
case "$1" in
  ""|-*) exec "$real" --standalone "$@" ;;
  *) exec "$real" "$@" ;;
esac
WRAP_EOF
chmod +x /tmp/lab-bin/opencode
export PATH=/tmp/lab-bin:$PATH
"""

    def relaunch_in(self, cfg, prov_ip, cwd, prelude):
        """Two launches in one terminal, back to back, through the scene's
        own launcher rather than the image's — the task record this contract
        writes lives under the run's `UZE_HOME`, and the launcher has to read
        the same one."""
        relaunch = continuity.relaunch_command(self.launcher_name())
        final = f"{prelude}\n{self.STANDALONE_WRAPPER}\ncd {cwd} && {relaunch}"
        return Tui(cfg, opencode_container(cfg, prov_ip, final), "opencode-continuity")

    def skill_catalog(self, tui):
        time.sleep(self.warmup)
        tui.type("/skills")
        time.sleep(1)
        tui.submit()
        # This surface renders by region, so a name arrives split across
        # repaint frames; accumulating is the only way to see it whole.
        return tui.collect(reads=8)

    def lists(self, catalog, skill):
        """OpenCode names a Skill by its qualified invocation label."""
        return f"flow:{skill}" in catalog.replace(" ", "")

    def invoke(self, tui, skill):
        """OpenCode V2 invokes a Skill as a **mention**, not a slash command.

        Measured on beta-19192: the picker renders skills as `"@" + id` and
        selects them as `{type: "skill", value: {id, mention}}`, and the
        prompt payload carries `skills` as mentions beside `files` and
        `agents`. Typing `/flow:commit` here would prove nothing about this
        harness, which is exactly the confusion a listing-only check let
        stand.
        """
        tui.type(f"@flow:{skill}")
        time.sleep(1.2)
        tui.submit()
        time.sleep(1.0)
        tui.submit()
        return tui.collect(reads=10)

    def mcp_inventory(self, tui):
        """`/mcps` — plural here — opens the MCP toggle surface.

        The warmup applies to every surface, not just the first: each
        contract opens its own session, so each pays the same wait before
        the surfaces behind the prompt have loaded.
        """
        time.sleep(self.warmup)
        tui.type("/mcps")
        time.sleep(1)
        tui.submit()
        return tui.collect(reads=6)

    def unsupported(self, prop):
        """`/skills` lists every delivered Skill, whatever `slash` says.

        Re-asked at beta-19192 (2026-09-06). The old reason — "no
        documented control hides a Skill from explicit invocation" — is
        false: the skill parser reads `metadata."opencode/slash"` falling
        back to a top-level `slash`, and two catalog builders filter with
        `skills.filter((s) => s.slash !== false)`. UZE writes that control,
        and its own routing calls this Native.

        What is still true is narrower and was measured, not assumed:
        the surface this vertical reads renders `flow:analyze` alongside
        the others, so the property cannot be observed *here*. Removing
        the declaration made the check fail on exactly that.

        What would retire this: reading the surface those two filters
        build — the `/` invocation palette — rather than the `/skills`
        browser, and proving on the same capture that a default Skill is
        listed there while the model-only one is not.
        """
        if prop == "model-only-is-not-user-invocable":
            return (
                "OpenCode honours `slash: false` in its `/` palette builders but "
                "its `/skills` browser lists every delivered Skill regardless; "
                "the property is not observable on the surface read here"
            )
        if prop == "model-only-is-not-invocable":
            # Measured, not assumed: the invocation check typed
            # `@flow:analyze` and its body reached the model. V2 has two
            # explicit paths and `slash` gates only one — the picker offers
            # every discovered Skill as `@id`, and `SessionPrompt.prepare`
            # expands a mentioned Skill whatever its `slash` value. UZE's
            # own route for `invoke.user: false` was moved to Adaptable on
            # the same evidence.
            return (
                "OpenCode V2 invokes a Skill by mention (`@id`) as well as by "
                "`/id`, and `slash: false` gates only the second: a Skill "
                "withheld from the `/` catalog is still invocable by mention"
            )
        return None

    def names_server(self, inventory, server):
        """OpenCode's `/mcps` surface is a toggle list showing connection
        state; it was not observed to print the server id.

        So presence is read from the connected row rather than the name. A
        weaker signal than an id, and recorded as such in
        `conformance/DECISIONS.md` — the surface is the vendor's, and
        asserting an id it does not render would be asserting fiction.
        """
        squeezed = inventory.replace(" ", "")
        return "Connected" in inventory or "disconnectspace" in squeezed
