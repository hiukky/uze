"""How one harness is driven, and what it cannot express.

A binding answers mechanics — the command that launches this TUI, the text
that proves it is ready, how a person invokes a Skill here. It never
asserts: the moment a binding decides whether something is correct, the
contract has stopped being common and this file has become another
vertical.

`unsupported` is the one place a harness may decline part of a contract. It
returns a reason, and the run records `Unsupported` in the evidence beside
the passes. An omitted check is invisible; a declared one is reviewable —
the same reason the exposure model treats `Unsupported` as a route rather
than a gap.
"""


class Bindings:
    #: Registry id, matching the integration's own.
    harness = ""

    #: The command run inside the container to start the TUI.
    launch = ""

    #: Text that proves the TUI is ready for input. Any one is enough.
    ready_markers = ()

    #: Seconds to wait after `ready_markers` before typing. Prompts render
    #: before the surfaces behind them finish loading, and input sent into
    #: that window is lost — a per-harness fact, measured, not guessed.
    warmup = 0.0

    def prepare(self, tui):
        """Anything between launch and a usable prompt — an onboarding flow,
        a first-run consent. Returns the plain screen text.

        Default: wait for `ready_markers`. A harness that needs more
        overrides this, because "the process started" and "the prompt
        accepts input" are different facts, and treating them as one is how
        a loose marker starts passing checks against a splash screen.
        """
        return tui.ready(self.ready_markers)

    def session(self, cfg, prov_ip):
        """A live TUI for this harness, as a context manager."""
        raise NotImplementedError

    def skill_catalog(self, tui):
        """Opens this harness's Skill list and returns the screen text."""
        raise NotImplementedError

    def lists(self, catalog, skill):
        """Whether `catalog` offers the canonical Skill named `skill`.

        A harness decides this, because only it knows how its own surface
        spells a Skill — `commit (flow)` on one, `flow:commit` on another.
        It is still mechanics, not judgement: the contract decides what the
        answer means.
        """
        raise NotImplementedError

    def mcp_inventory(self, tui):
        """Opens this harness's MCP surface and returns the screen text."""
        raise NotImplementedError

    def names_server(self, inventory, server):
        """Whether `inventory` shows the MCP server `server` as present.

        Same split as `lists`: only the harness knows how its own surface
        spells a server — an id on one, a display name on another, a
        connection row on a third. The contract decides what the answer
        means, never how to read it.
        """
        return server in inventory.replace(" ", "")

    def invoke_skill(self, tui, label):
        """Invokes `label` the way a person would here, returning the turn's
        screen text."""
        raise NotImplementedError

    def unsupported(self, capability):
        """A reason this harness cannot express `capability`, or `None`.

        Answering with a reason is a result. Answering `None` when the
        harness in fact cannot is how a suite starts lying.
        """
        return None
