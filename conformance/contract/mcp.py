"""What an MCP server must do, on every harness.

UZE delivers one MCP server declaration and each integration writes it into
whatever configuration its vendor reads. The configuration shape is the
integration's business and diverges by design; what must not diverge is the
outcome — the server is there, and the harness knows what it offers.

The old suites asserted this four different ways, from
`mcp-server-in-tui-inventory` (one harness) to `mcp-tool-invoked-via-tui`
(another), and one of them was satisfied by the heading "MCP Tools"
rendered above the words "No MCP servers configured". A surface check that
matches its own chrome proves the screen exists, not the server.
"""

from shared.common import check, describe

#: The server UZE delivers from the `mcp-plugin` fixture.
SERVER = "uze-conformance"


def _declined(bindings, prop):
    reason = bindings.unsupported(f"mcp-{prop}")
    if reason:
        check(
            f"mcp-{prop}",
            True,
            f"{bindings.harness} cannot: {reason}",
            kind="adapt",
        )
    return reason


def assert_contract(cfg, prov_ip, bindings):
    if _declined(bindings, "inventory"):
        return
    with describe("mcp"):
        _assert_inventory(cfg, prov_ip, bindings)


def _assert_inventory(cfg, prov_ip, bindings):
    with bindings.session(cfg, prov_ip) as tui:
        plain, ready = bindings.prepare(tui)
        check(
            "mcp-tui-ready",
            bool(ready),
            f"{bindings.harness} reached its prompt"
            if ready
            else plain[-160:].replace("\n", " "),
        )
        if not ready:
            return

        inventory = bindings.mcp_inventory(tui)
        tui.snapshot("mcp", inventory)

        # Named, not merely "a surface exists". The distinction is the whole
        # reason this check is worth running: a heading is not a server.
        present = bindings.names_server(inventory, SERVER)
        check(
            "mcp-server-in-inventory",
            present,
            f"the harness shows `{SERVER}` in its MCP inventory"
            if present
            else inventory[-200:].replace("\n", " "),
        )
