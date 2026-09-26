# <marketplace name>

A marketplace of agent plugins: agent skills, portable hooks, MCP servers,
instruction contributions. UZE reads this repository through its own
registry — the same files every harness's mechanism is generated from.

    uze market add <this directory>
    uze install <plugin>@<this market>

The `plugins/` tree holds one directory per plugin, each with a
`plugin.json` naming it and whatever capabilities it carries. `uze install
-m <plugin>@<this-market>` delivers it; editing the files is what an update
re-reads.
