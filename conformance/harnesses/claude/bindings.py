"""How Claude Code is driven. No assertions live here."""

import time

from contract.bindings import Bindings
from contract.tui import Tui

from .scenarios import claude_container, drive_onboarding


class ClaudeBindings(Bindings):
    harness = "claude"
    launch = "exec claude"
    ready_markers = ("Opus", "API Usage Billing", "❯")
    warmup = 6.0

    def session(self, cfg, prov_ip):
        return Tui(cfg, claude_container(cfg, prov_ip, self.launch), "claude-contract")

    def session_in(self, cfg, prov_ip, cwd, prelude):
        final = f"{prelude}\ncd {cwd} && {self.launch}"
        return Tui(cfg, claude_container(cfg, prov_ip, final), "claude-isolation")

    def prepare(self, tui):
        """Claude opens on a chain of first-run dialogs — welcome, security
        guide, API key, theme, folder trust — before the prompt exists."""
        _, plain, _ = drive_onboarding(tui.child)
        if not plain or not any(m in plain for m in self.ready_markers):
            plain, _ = tui.until(self.ready_markers)
        tui.snapshot("ready", plain)
        return plain, any(marker in plain for marker in self.ready_markers)

    def skill_catalog(self, tui):
        """The `/` menu is the surface a person invokes from: typing a
        namespace prefix opens its completions. `/skills` is the management
        view and lists every Skill whatever its policy, so it cannot tell a
        model-only Skill from an invocable one."""
        time.sleep(self.warmup)
        tui.type("/flow:")
        catalog = tui.collect(reads=4)
        tui.child.send("\x1b")
        time.sleep(0.5)
        return catalog

    def lists(self, catalog, skill):
        """Claude names a Skill by its namespaced invocation label."""
        return f"flow:{skill}" in catalog.replace(" ", "")

    def mcp_inventory(self, tui):
        """`/mcp` lists every configured server and its connection state."""
        tui.type("/mcp")
        time.sleep(1)
        tui.submit()
        inventory, _ = tui.until(["uze-conformance", "MCP"], tries=8)
        return inventory

    def unsupported(self, prop):
        """Claude Code documents both halves of the invocation policy:
        `disable-model-invocation: true` and `user-invocable: false` (the
        latter hides a Skill from the `/` menu and refuses `/name`), and UZE
        emits both — nothing to declare."""
        return None
