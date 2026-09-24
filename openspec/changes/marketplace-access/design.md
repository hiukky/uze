## Context

Facts from today's code that shape everything below (see proposal.md — Why):

- `acquisition::git::run` spawns Git with `env_clear`, `GIT_CONFIG_GLOBAL=/dev/null`,
  `GIT_TERMINAL_PROMPT=0`, an empty `GIT_ASKPASS` and
  `GIT_ALLOW_PROTOCOL=file:https:ssh:git`. The invariant "`acquisition::git`
  clones untrusted remote repositories, so it strips the environment" is named
  in `docs/architecture/invariants.md`. What it protects against is the
  *repository* influencing the run (hooks, submodules, a filter it names) and
  the run hanging on a prompt; the operator's own credentials were stripped
  along with those, not because they were the threat. Nothing stops `ssh`
  itself from prompting: `GIT_TERMINAL_PROMPT` does not reach it, and
  `with_process_group` uses `setpgid`, not `setsid`, so `ssh` keeps the tty.
- `marketplace::repository_of` records a local checkout's identity as its
  `origin`, verbatim. `parse_marketplace_source` recognises a remote only by
  `scheme://`, so `git@host:owner/repo` is read as a relative path, and its
  `@ref` split takes the *last* `@`, which would cut an `scp` URL in two.
- A URL is compared byte for byte in seven places: the mirror's `origin` check
  (`mirror.rs`, which wipes and re-clones on a mismatch), the machine registry
  (`state::marketplace_add`), the project's registration
  (`project_environment::register_marketplace`), the Store's
  `Provenance::same_origin`, the link check (`state::same_repository`), the
  catalogue cache (`marketplace_catalogue`), and the lock's `answers`.

The `plugin-freshness-and-linked-marketplaces` change already separates the
author's loop (`uze market link`, machine scope) from what a project declares.
This change assumes it; where the two touch, D8 says how.

## Goals / Non-Goals

**Goals:**
- One project file works for everyone who can read the repository: anonymous
  for a public one, the operator's own credentials for a private one.
- No new key in `agents.yaml` or `agents.lock`, and no change to their shape.
- The same repository has the same identity on every machine, whatever that
  machine's configuration.
- The short locator is a convenience at the prompt and nothing more.
- Every scenario in the spec is exercised by a deterministic test with no
  network; the SSH and credential-helper paths also in a journey world.

**Non-Goals:**
- UZE storing or managing tokens. Credentials stay in the operator's Git
  helper and SSH agent; UZE never holds one.
- Honouring the operator's `url.<base>.insteadOf` rules. The ladder covers what
  they are used for; a port, user or hostname rewrite belongs in
  `~/.ssh/config`, which is honoured.
- An SSH endpoint per alias (`--ssh`). Designed and deferred — see D5.
- Git LFS, submodules, or a marketplace hosted outside Git.
- Resolving a short locator for a *plugin* (`uze <plugin>@<market>` keeps
  naming a marketplace by its registered name).

## Decisions

### D1. Identity is `https://<host>/<path>`, decided by shape alone
The canonical form is HTTPS because it is the one spelling every forge serves
anonymously for a public repository, so it is the only one that means "this
repository" without also meaning "and you need an account here". It is a
*name*: it does not promise the host serves HTTPS — the ladder (D2) decides
how the name is reached.

Canonicalization reads nothing but the URL, so every machine computes the same
identity and no machine's configuration can make collaborators rewrite each
other's lock:

- `https://H/P`, `https://H/P.git`, `https://H/P/` → `https://h/P`
- `git@H:P(.git)` and `ssh://git@H/P(.git)` (default user `git`, no port) →
  `https://h/P`
- an SSH URL with another user or an explicit port → **as written** (there is
  no honest HTTPS spelling for it)
- a local repository with no `origin` → its absolute path

The host is lowercased; the path keeps its case. Canonicalization is one pure
function in `acquisition` (a new `forge` module, which is also where the
built-in host table lives, keeping `git.rs`'s "no host is named here" true),
and **every** comparison of two sources goes through it — the seven sites in
Context. That, not the lock alone, is what keeps an old `git@…` spelling from
colliding with its canonical form.

Alternatives: record the spelling as typed (today — the bug); canonicalize only
hosts in the host table (the first draft — rejected in review because the table
is per machine, so the identity would be too); a scheme-less `host/path` like a
Go module path (not a URL Git accepts, so every consumer of the lock would need
UZE's resolver).

### D2. A transport ladder on one host, remembered per repository
For an `https://` identity: anonymous HTTPS → HTTPS with the operator's
credentials → SSH `git@<host>:<path>.git`. An identity recorded over SSH (D1's
"as written") is fetched as written, once, with the operator's credentials.

It stops at the first success. When an HTTPS attempt cannot resolve or cannot
connect to the host, the other HTTPS attempt is skipped — it would meet the
same host and port — but SSH is still asked: 443 blocked with 22 open is
common on a company network, and an `~/.ssh/config` `Host github-work` is a
name only `ssh` can resolve (review: ending the ladder on a DNS failure broke
every such alias). The machine is reported offline only when no transport
resolved the host. A local write failure ends the ladder as itself; a
failure that is neither access nor offline is reported as an acquisition
failure, not blamed on credentials. SSH carries `ConnectTimeout=15`.

The winner is remembered inside the mirror it reached (`transport.json`,
beside the bare repository in the cache tier), so a private repository's next
fetch starts where the last one ended; the mirror is also what fetches a
missing blob later, and it has to reach the same place the same way, so
`origin` is set to the transport that answered and the identity lives in
`transport.json`. A one-shot clone that keeps no mirror remembers nothing; a failure on the remembered transport runs the full
ladder, since access changes (a key revoked, a repository made public). A
poisoned entry can cost only order, never destination: the ladder has one host.

Anonymous first, rather than "whatever worked for this host last time",
because public/private is a property of the repository, not the host, and
because a credential must not be offered to a request that did not need one.

**A failed refresh changes nothing.** `origin` is pointed at each transport as
it is tried and put back when none answers; refs a `--prune` removed on an
empty answer are restored.

**The mirror is judged by the identity it remembers.** The guard in
`mirror.rs` compares the identity in `transport.json` — or, for a mirror
written before it existed, its `origin` — with the one asked for, through
D1, so it never wipes a mirror because the transport or the spelling
differed. An attempt that "succeeds" with no commit at all — a forge
answering a login page with 200, which Git reads as an empty dumb-HTTP
repository — is a failed attempt, and the next transport is asked.

### D3. Two environments: anonymous, and the operator's
**Anonymous attempt** — today's stripped environment, plus the operator's
network (below). `HOME` stays cleared, or libcurl's `.netrc` would send a
credential to a request that must carry none.

**Authenticated attempts** — the operator's environment minus every `GIT_*`
variable, with UZE's own protections set on top: `GIT_CONFIG_NOSYSTEM=1`,
`GIT_CONFIG_GLOBAL=/dev/null`, hooks off, no submodules, no terminal prompt,
the same protocol allow-list. Scrubbing rather than whitelisting, because a
credential helper needs whatever its author needed — `gh` reads `GH_TOKEN` in
CI, libsecret needs `DBUS_SESSION_BUS_ADDRESS` and `XDG_RUNTIME_DIR`, the
`cache` helper `XDG_CACHE_HOME` — and a whitelist would be a list of helpers
that happen to work. What the stripped environment exists to exclude is
*config* (aliases, filters, `includeIf`, `core.sshCommand`), and config stays
excluded.

The operator's credential config is replayed, not their config file: every
`credential.*` key, **with its URL scope** (`credential.https://github.com.helper`
is what `gh auth setup-git` writes), read with `git config --includes
--get-regexp '^credential\.'` across all scopes from a directory that is not a
repository. Replayed through `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/
`GIT_CONFIG_VALUE_n`, never `-c`: an inline helper can carry a secret, and argv
is visible in `ps` and recorded in the acquisition span.

**SSH is non-interactive and verifies the host first.** Every attempt sets
`core.sshCommand` to `ssh -o BatchMode=yes -o StrictHostKeyChecking=yes`. A
passphrase or host-key prompt then fails instead of stopping the process on
the tty, and an unknown host key fails *before* any key is offered — otherwise
a host named in a checked-in `agents.yaml` could collect the operator's public
keys and identify them. The failure says how to trust the host
(`ssh -T git@<host>` once). OpenSSH finds `~/.ssh` through `getpwuid`, not
`$HOME`, so `~/.ssh/config` applies in both environments.

**Helpers never interact.** `credential.interactive=false` is pushed after the
operator's own settings, and `GCM_INTERACTIVE=never` set: a host named in
someone else's `agents.yaml` must not be able to open a browser login.

**The operator's config is read outside any repository.** `git config` runs
with `GIT_DIR` set to an empty directory, so a repository enclosing the
temporary directory cannot contribute a `credential.helper=!…`.

**Redirects do not carry credentials elsewhere.** The helper rung sets
`http.followRedirects=false`, so a redirect to another host fails instead of
handing that host a request the helper answers.

**The operator's network, on every attempt.** Proxy variables in both cases
libcurl reads (`http_proxy`, `https_proxy`, `all_proxy`, `no_proxy`, and the
uppercase forms), `SSL_CERT_FILE`, `SSL_CERT_DIR`, `GIT_SSL_CAINFO`, and the
config keys `http.proxy`, `http.sslCAInfo`, `http.sslCAPath` with their
`http.<url>.*` scopes — paths resolved with `--type=path`, since `~/` cannot
expand once `HOME` is cleared — replayed the same way as the credential keys.
They describe how this machine reaches the network, which the repository
cannot influence.

**`http://` only to loopback.** `GIT_ALLOW_PROTOCOL` gains `http` only when the
URL's host is `127.0.0.1`, `::1` or `localhost`; that is what lets tests and
journey worlds stand a real server up without TLS, and it cannot reach another
machine.

### D4. No cross-host fallback for a short locator
The request was "GitHub primary, GitLab fallback". Rejected, for a security
reason and a reproducibility one:

- **Private looks like absent.** Over smart HTTP, GitHub and GitLab answer 401
  for a private repository *and* for one that does not exist, so the two cannot
  be told apart. A fallback would therefore trigger exactly when the operator's
  own private repository is not visible to the transport that asked — and
  resolve the same name on another forge, where somebody else may own it. That
  is dependency confusion, delivered by the convenience.
- **The answer would depend on the machine and the moment.**

What stays of the idea: the default host is configurable, prefixes name any
other host, a not-found error lists the prefixed forms to try, and a success
prints the full identity it resolved to.

### D5. The host table
A record in the state tier (`UzeHome::hosts_path`, `state/hosts.json`), shape 1
via `uze_document::Shaped`. Built-ins (`github`, `gitlab`, `codeberg`,
`bitbucket`) live in code in the `forge` module, so the file holds only what
the operator added plus `default`; deleting it costs the operator's own
aliases, which is why it is a record and not cache.

An entry is `{alias, base}`, the base an `https://` URL (loopback may be
`http://`). The base may carry a path, which makes an alias a shortcut to an
organisation (`https://codeberg.org/my-org`, so `oss:plugins` is
`https://codeberg.org/my-org/plugins`). The table is **input only**: it turns
what is typed into a URL and plays no part in canonicalization (D1) or access
(D2).

Alias names are `[a-z0-9-]+`, and may not be `http`, `https`, `ssh`, `git` or
`file` — those are what a URL's scheme looks like before the `:`.

**Deferred: an SSH endpoint per alias (`--ssh`).** Port, user and hostname
differences are expressed by `~/.ssh/config`; the one thing that cannot be is
a forge whose SSH *path* differs from its HTTPS path — Bitbucket Data Center
(`/scm/`) and Azure DevOps (`_git`). Both are reached over HTTPS, anonymously
or with their credential helper, which is how they are normally used, and an
SSH spelling of either is fetched as written. The entry's shape leaves room
for an `ssh` field when a concrete case asks for it.

CLI: one verb, whose arguments decide what it does, after `uze theme glyphs
[set]` and `git config <key> [value]`:

- `uze market host` — list, marking the default;
- `uze market host <alias>` — make it the default;
- `uze market host <alias> <https-base>` — define an alias;
- `uze market host <alias> --remove` — remove an alias the operator defined.
  A built-in cannot be removed. Removing the alias that is the default returns
  the default to `github` and says so: a default that names nothing would fail
  every short locator later instead of now. Registered marketplaces are
  unaffected — each holds its full URL, never the alias.

Machine-scoped (ADR-019), `Budgeted` (a file read/write).

"Host" over "registry" and "provider": `registry` already names the machine's
marketplace registry, and elsewhere means a package service with an index of
its own, which a Git forge is not; `provider` says nothing about what is
provided, and names the model provider in the Lab.

### D6. Locator grammar (input only)
In order:

1. `/`, `./`, `../`, `~`, a bare `.` or `..` → path;
2. `scheme://` → URL;
3. `user@host:path` → scp-style SSH URL;
4. `alias:path` where `alias` is in the table → that host;
5. any other `word:…` → refused (an unknown alias, or `host:path` with no
   user, which Git would read as SSH and the operator probably did not mean);
6. `owner/repo[/…]` → default host;
7. a single segment such as `ai` → refused, suggesting `./ai` when that
   directory exists.

Then `@ref` and `#subdir` — an `@` starts a ref only *after* the path, so the
`git@` of an scp URL is never one. An `owner/repo` that is also an existing
directory is refused as ambiguous, so the change of meaning is never silent.

A path must *look* like one, with no exception: every package tool has settled
it that way (`npm i foo` is a name, `npm i ./foo` a folder; `go get ./x`;
`pip install ./x`), and it keeps the bare word free for a future name-only
lookup. Once recognised, a path resolves as today: canonicalized, it must be a
Git work tree with a commit, it is read *at that commit* (a working tree is
`uze market link`'s job), and its identity is its `origin` by D1 — or its
absolute path, with a warning that the project will resolve it on this machine
only.

### D7. How the scenarios are validated
Three tiers, cheapest first, each owning what it alone can prove:

1. **Unit (`uze-core`)** — the locator grammar and D1: every spelling in the
   spec → its identity; ports and users kept; host lowercased; the refusals.
2. **Integration (`tests/packages/acquisition.rs`)**, no network, no sshd:
   - a testkit `GitHttpServer`: a `TcpListener` on `127.0.0.1` running
     `git http-backend` as CGI per request, with optional Basic auth, an
     optional "200 login page" mode, and an optional redirect to a second
     server. Pure std + the `git` binary already required — no new dependency.
   - a fake `ssh` first on `PATH` that serves `git-upload-pack` for a local
     bare repository, refuses without `SSH_AUTH_SOCK`, and records the options
     it was given (`BatchMode`, `StrictHostKeyChecking`).
   - a test `HOME` whose `.gitconfig` names the `store` helper under a URL
     scope, plus an alias and a filter that must not take effect.
   - a loopback forward proxy that records the requests it carried.
3. **Journey (`02-packages`)**: a world running a smart-HTTP forge on
   loopback (Python over `git http-backend`) and a stand-in `ssh` in the
   world's `bin`, a host alias pointing at the forge, and the flow end to
   end — `market host <alias> <base>`, `market add alias:owner/repo`,
   `<plugin>@<market>`, `install` — checked against `agents.yaml`,
   `agents.lock`, the harness tree and the mirror on disk. Not a real
   `sshd`: a non-root one can serve only its own user, and the endpoint UZE
   derives is always `git@<host>`; what OpenSSH does with `BatchMode` and
   `StrictHostKeyChecking` is OpenSSH's, and the journey proves UZE asks
   for both.

The Lab is not involved: nothing here is harness-specific.

### D8. Speed: one connection, and none to add a plugin

Measured on a private GitHub marketplace over SSH, where one handshake is
about 1.1s of round trips:

- **The mirror carries its small blobs** (`--filter=blob:limit=1m`): the
  catalogue and a plugin's files travel with the history, so neither needs
  a second connection. A binary over a megabyte still stays behind, and a
  checkout that needs one fetches every missing blob in one request.
- **Adding answers from the mirror while the catalogue stands** (one hour):
  what a listing showed is what adding installs; `update` asks the remote,
  and the output says when the mirror was used and how old it is. One
  operation fetches each mirror at most once.
- **SSH shares one connection per host for sixty seconds** (ControlMaster),
  its socket in a directory only this user reaches, or not at all.
- **Harnesses are delivered to, and detached from, at once**, and the
  Claude and Antigravity integrations read their vendors' own records
  (`installed_plugins.json` v2, `known_marketplaces.json`, `settings.json`,
  `import_manifest.json`) before starting their CLIs, falling back to them
  on any record they do not recognise.

`uze <plugin>@<market>` went from 11.5s to 1.0s, `market add` of a known
marketplace from 5.0s to 0.8s, and `plugin remove` from 4.9s to 1.0s.

### D9. Where this meets `plugin-freshness-and-linked-marketplaces`
- Its "a marketplace this machine cannot reach is skipped, never fatal" is
  about a source that is unreachable *by declaration* (a path that does not
  exist here). This change's DNS failure is named **offline** and fails the
  fetch; the two words must not share a meaning.
- Its requirement that an access failure name "the URL it tried" becomes
  "each transport it tried" — edited in place in that change's spec, since it
  is open.
- Its "link to a missing path clones it" clones through the ladder, and its
  link check compares identities through D1.

## Candidate ADRs

- **A marketplace's identity is transport-neutral and decided by its shape;
  access is the machine's.** Fixes what may appear in a project file and moves
  the acquisition-environment boundary; expensive to reverse once locks carry
  canonical URLs.
- **No cross-host resolution of a short name.** A security stance the next
  "just add a fallback" request will need to find written down.

## Risks / Trade-offs

- [A forge answers anonymous HTTPS with a login page and 200 instead of 401]
  → Git fails with "not a git repository", which the ladder treats as a
  refusal and continues; tested with a server that does exactly that.
- [Three attempts cost latency for a private repository on first fetch] →
  remembered transport; the anonymous attempt fails fast with no prompt.
- [`StrictHostKeyChecking=yes` refuses a host the operator has never connected
  to] → intended; the message says how to accept it once, and HTTPS rungs are
  unaffected.
- [An identity on a host that serves only SSH is spelled `https://`] → it is a
  name, not a promise; the SSH rung reaches it. The cost is two failed HTTPS
  attempts on first fetch, then the remembered transport.
- [A credential helper that prompts graphically] → `GIT_TERMINAL_PROMPT=0`
  does not stop a GUI helper; accepted, because it is the operator's helper
  doing what the operator configured it to do.

## Migration Plan

No record changes shape; `hosts.json` is new at shape 1, and the remembered
transport is cache, inside the mirror. Existing manifests and locks keep working: every
comparison goes through D1, so an old `git@…` spelling matches its canonical
form in the registry, the Store, the mirror and the lock, and the next write
records it canonically. This repository replaces its tracked
`path: /home/hiukky/ai` with `git: https://github.com/hiukky/ai` and the author
uses `uze market link ai /home/hiukky/ai` locally. Rollback is reverting the
binary: an older build reads an `https://` identity like any other URL.
