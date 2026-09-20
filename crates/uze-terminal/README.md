# uze-terminal

The local terminal runtime behind the workspace client: a server owning the
pseudoterminals and emulation state, and a versioned client protocol. A client
attaches, renders snapshots and forwards input — which is what keeps a pane
alive when the client leaves.

Depends on nothing in the workspace but `uze-document`, and resolves the path
to its own runtime directory itself — the one sanctioned exception to
`UzeHome` naming every path UZE owns.

```bash
cargo test -p uze-terminal
```
