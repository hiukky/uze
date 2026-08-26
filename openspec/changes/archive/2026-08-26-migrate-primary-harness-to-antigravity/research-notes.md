# Research notes

The full audit (comparison map per area, official-docs quotes, empirical
evidence log from `agy` 1.1.19 in an isolated `$HOME`, module map with
SHARE_HELPER / KEEP_SEPARATE / MOVE_TO_GENERIC_INTEGRATION_HELPER
dispositions, and stop-condition verdicts) lives in
**`docs/architecture/antigravity-compatibility.md`**, kept in `docs/` so it
can outlive this change's archive (it is the evidence record for ADR-027).

Highlights:

- Official docs pages used as the documentation source of truth:
  `antigravity.google/docs/cli/plugins/`, `/docs/cli/gcli-migration/`,
  `/docs/cli/install/`, `/docs/mcp/`, `/docs/skills/`, `/docs/cli/reference/`.
- Real-binary evidence (`agy` 1.1.19, isolated HOME): `agy --version` →
  `1.1.19`; `plugin validate/install/list/uninstall/enable/disable`; `mcp
  add/list/remove/disable/enable`; legacy-import conversion output; install
  copies bytes and merges stale files on same-name re-install; symlinks are
  dereferenced; `plugin list` JSON is machine-readable on stdout; `mcp
  list` is human-readable and excludes plugin/workspace servers headlessly.
- The legacy import command was evidence/reference only — UZE projects
  directly from canonical resources and never depends on it.
- A documented docs-vs-binary discrepancy (staged path:
  `~/.gemini/config/plugins/` in practice vs `antigravity-cli/plugins/` in
  the docs page) is resolved by trusting the binary.
