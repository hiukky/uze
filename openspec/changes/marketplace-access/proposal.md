## Why

A marketplace is reachable today only by accident of how its URL was
spelled and of how the machine reading it is configured. Acquisition runs
Git in a stripped environment — no global config, no credential helper, no
`SSH_AUTH_SOCK` — so a private repository works only over SSH with an
unencrypted key, and a private HTTPS repository never works. Worse, the URL a
project records is whatever the author's checkout happened to use: this
repository's own `agents.lock` names `git@github.com:hiukky/ai.git`, taken
from the `origin` of a local `path:` checkout. The repository is public, and
still a collaborator without a GitHub SSH key — a CI runner, a fresh machine,
anyone without an account — cannot install it, because SSH always
authenticates even for public repositories.

The spelling is also the friction at the front door. `uze market add` wants
a full URL or an absolute path, where every comparable tool (`brew tap
owner/repo`, npm's `github:owner/repo`, a harness's own `/plugin marketplace
add owner/repo`) accepts the short form.

## What Changes

- **A marketplace's identity is transport-neutral.** Whatever the input
  spelling (`owner/repo`, `https://…`, `ssh://…`, `git@host:owner/repo.git`,
  or a local checkout's `origin`), UZE writes one canonical
  `https://<host>/<path>` into `agents.yaml` and `agents.lock`, decided by
  the URL's shape alone so every machine computes the same one (an SSH URL
  with a port or a non-`git` user is kept as written). Every comparison of
  two sources — registry, Store, mirror, catalogue, link, lock — uses it, so
  an old spelling meets its canonical form without a conflict. The project
  file never says *how* to authenticate.
- **Access is decided per machine, per fetch, on one host.** A fetch tries
  anonymous HTTPS first, then HTTPS with the operator's own credentials,
  then SSH (non-interactive, refusing a host key the operator never
  accepted) — always the same host and the
  same repository path, never a different one. The transport that answered
  is remembered, so a private repository does not pay the failed attempts
  on every refresh. When none answers, one error names the identity, what
  was tried, and what each said.
- **A short locator.** `uze market add owner/repo` resolves against the
  machine's default host (github.com unless the operator sets another);
  `gitlab:group/sub/repo` names a host explicitly. The short form is input
  only — it is resolved once and the full URL is what gets written.
- **A machine-scoped host table.** Built-in aliases (`github`, `gitlab`,
  `codeberg`, `bitbucket`), plus the operator's own (a self-hosted forge), and
  which one is the default. Managed by `uze market host …`. Input only: it
  plays no part in identity or access.
- **The acquisition environment admits the operator's network** (proxy, CA)
  on every attempt, and their credentials on the authenticated ones, without
  admitting any other Git configuration; plain `http://` only to loopback.
- **No cross-host fallback.** A short locator that is not found on the
  default host is an error that suggests the prefixed form; UZE never probes
  another host for the same name. (See design.md — a forge answers "not
  found" for a private repository you cannot see, so a fallback would fire
  exactly in the private case and resolve the name to whoever owns it
  elsewhere.)
- The `scp`-style spelling `git@host:owner/repo` is recognised as remote;
  today it is read as a relative local path.
- **BREAKING (input only):** a local path must be spelled as one (`/`, `./`,
  `../`, `~`, `.`, `..`). A bare `ai` is refused with a hint to write `./ai`,
  and a bare `owner/repo` that is also an existing directory is refused as
  ambiguous; neither is silently read as a directory any more. The `path:`
  key in `agents.yaml` is unaffected — it is always a path.

## Capabilities

### New Capabilities

- `marketplace-access`: how a marketplace locator resolves to a canonical
  identity, how a machine reaches a public or private repository behind it,
  and the host table that backs both.

### Modified Capabilities

- `marketplace`: registering a marketplace accepts the short and `scp`-style
  locators and records the canonical identity rather than the spelling typed.

## Impact

- `uze-core`: `package/acquisition` (a `forge` module for canonical identity
  and the built-in hosts, locator parsing, the transport ladder in
  `git.rs`/`mirror.rs`), every source comparison (`state`, `Provenance`,
  the lock's `answers`), a host-table record named in `UzeHome` (state tier,
  `Shaped`), a remembered-transport entry in the cache tier.
- `uze-application`: `marketplace.rs` (`parse_marketplace_source`,
  `repository_of`'s identity), `project_environment.rs` (lock writing),
  `market host` service methods.
- CLI: `uze market host [<alias> …]` (list, default, define, remove), classified in
  `command_performance.rs`; `market add` help text.
- `docs/architecture/invariants.md`: the acquisition-environment invariant
  changes from "strips everything" to "strips all Git configuration; admits
  the operator's network always, and their credentials on the authenticated
  attempts".
- Builds on `plugin-freshness-and-linked-marketplaces`: a linked checkout
  stays the author's loop, and is how this repository replaces its tracked
  `path: /home/hiukky/ai`. Two of its requirements are edited in place
  (design D8).
- No new dependency. No change to Engine, Router or any integration; the
  Store changes only in how it compares two provenances.
