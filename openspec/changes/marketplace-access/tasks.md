## 1. Canonical identity

- [ ] 1.1 A `forge` module in `acquisition` holding the pure canonicalization of design D1 (by shape only: `https`, `git@H:`, `ssh://git@H/` without port → `https://h/P`; other user or port kept; host lowercased; `.git` and trailing slash dropped)
- [ ] 1.2 Unit table: every spelling in the spec, ports and users kept, case, suffixes, nested groups, `@ref`/`#subdir` preserved
- [ ] 1.3 Every source comparison goes through it: `state::marketplace_add`, `project_environment::register_marketplace`, `Provenance::same_origin`, `state::same_repository` (link check), `marketplace_catalogue` cache key, `LockedMarketplace::answers` — one test per site showing an old `git@…` spelling matches its canonical form
- [ ] 1.4 `repository_of` canonicalizes a local checkout's `origin`; with no `origin`, the identity is the absolute path and `market add`/`install` warn it resolves on this machine only
- [ ] 1.5 A registry entry recorded in an old spelling is rewritten canonically on its next write, with no conflict

## 2. Host table and locator

- [ ] 2.1 `UzeHome::hosts_path()` (`state/hosts.json`), record type implementing `uze_document::Shaped` at shape 1
- [ ] 2.2 Built-in aliases in the `forge` module, merged with the record on read; `github` is the default when none is set
- [ ] 2.3 Validation: alias names `[a-z0-9-]+` and not `http|https|ssh|git|file`; built-ins cannot be redefined or removed; base must be `https://` (or `http://` on loopback)
- [ ] 2.4 Removing the default alias returns the default to `github` and says so
- [ ] 2.5 `uze-application` service: list / default / define / remove, under the mutation lock
- [ ] 2.6 CLI `uze market host [<alias> [<https-base>] [--remove]]` per design D5, classified `Budgeted` in `command_performance.rs`
- [ ] 2.7 Locator parser per design D6, including: `@` starts a ref only after the path; an unknown `word:` prefix and `host:path` without a user are refused
- [ ] 2.8 Refusals: a bare single segment fails suggesting `./<it>`; a short locator that is also an existing directory fails naming both readings
- [ ] 2.9 `parse_marketplace_source` uses the parser; `market add` records the canonical identity and prints it

## 3. Transport ladder

- [ ] 3.1 Attempt list for an identity: `https://` → anonymous HTTPS, credentialed HTTPS, SSH `git@<host>:<path>.git`; an SSH identity kept as written → that URL once, credentialed
- [ ] 3.2 Anonymous environment: today's stripped one with `HOME` still cleared (no `.netrc`), plus the operator's network (3.4)
- [ ] 3.3 Credentialed environment: the operator's environment minus `GIT_*`, with `GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL=/dev/null`, hooks off, no submodules, no prompt; `credential.*` keys with URL scopes read via `git config --includes --get-regexp` from a non-repository directory, replayed through `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n`, never argv; `http.followRedirects=false`
- [ ] 3.4 Network on every attempt: proxy variables in both cases, `SSL_CERT_FILE`, `SSL_CERT_DIR`, `GIT_SSL_CAINFO`; `http.proxy`, `http.sslCAInfo`, `http.sslCAPath` and their `http.<url>.*` scopes, paths resolved with `--type=path`, replayed the same way
- [ ] 3.5 `core.sshCommand` = `ssh -o BatchMode=yes -o StrictHostKeyChecking=yes` on every attempt
- [ ] 3.6 `GIT_ALLOW_PROTOCOL` gains `http` only for a loopback host
- [ ] 3.7 Failure classification: DNS failure → offline, stops the ladder; connect/TLS failure → next transport; "cannot reach" phrases checked before "refused" phrases
- [ ] 3.8 Remembered transport per identity: `UzeHome::transport_cache_path()` in the cache tier; tried first, full ladder on its failure
- [ ] 3.9 One error when nothing answers: identity, each transport tried, each reason; the SSH host-key case says how to accept the host; a short locator lists the prefixed forms
- [ ] 3.10 Mirror: cloned with the identity as `origin`, fetched with the transport URL explicitly; a mirror whose `origin` is an old spelling of the same identity is rewritten, not re-cloned
- [ ] 3.11 Every acquisition path (mirror fill/fetch, plugin materialize, the sibling change's link-clones-a-missing-path) goes through the ladder; no third spawn convention
- [ ] 3.12 Rewrite the acquisition-environment entry in `docs/architecture/invariants.md` and name its tests

## 4. Deterministic validation

- [ ] 4.1 Testkit `GitHttpServer`: loopback `TcpListener`, `git http-backend` as CGI, optional Basic auth, optional "200 login page" mode, optional redirect to a second server
- [ ] 4.2 Testkit fake `ssh` first on `PATH`: serves `git-upload-pack` for a local bare repository, refuses without `SSH_AUTH_SOCK`, records its options
- [ ] 4.3 Testkit loopback forward proxy that records the requests it carried
- [ ] 4.4 Public: no helper and no agent installs anonymously, including from a lock spelled `git@…`
- [ ] 4.5 Private via a URL-scoped `store` helper; the token appears in no UZE file, log, trace or argv
- [ ] 4.6 A redirect on the credentialed rung fails and the second server sees no request
- [ ] 4.7 Private via SSH; the next fetch goes to SSH first; `BatchMode` and `StrictHostKeyChecking` were passed
- [ ] 4.8 Private via SSH on a host no machine configured
- [ ] 4.9 HTTPS port closed, SSH accepted: the fetch succeeds over SSH
- [ ] 4.10 No access: one combined message, nothing recorded
- [ ] 4.11 Offline (a name that does not resolve): no credentialed attempt
- [ ] 4.12 Proxy and CA: the anonymous and credentialed attempts go through the proxy; an `http.sslCAInfo` path with `~/` is honoured
- [ ] 4.13 A global alias and `filter` in the test `HOME` have no effect
- [ ] 4.14 Plain `http://` off loopback is refused
- [ ] 4.15 A short locator not found on the default host makes no request to another host
- [ ] 4.16 An alias for an organisation resolves under its base; the default host set with `uze market host gitlab` is used
- [ ] 4.17 An old `git@…` spelling in registry, Store and mirror meets `market add hiukky/ai`: no conflict, no re-clone
- [ ] 4.18 Inline-credential URL still refused before any Git process

## 5. The sibling change, this repository and docs

- [ ] 5.1 In `plugin-freshness-and-linked-marketplaces` (open, edited in place): "unreachable by declaration" and this change's "offline" kept distinct; the access-failure message names each transport tried
- [ ] 5.2 Replace the tracked `path: /home/hiukky/ai` with `git: https://github.com/hiukky/ai`; regenerate `agents.lock`
- [ ] 5.3 `market add` help and the docs page for marketplaces: short locators, `market host`, public vs private, the SSH host-key step, and what never goes in a project file

## 6. Journeys (after hand validation)

- [ ] 6.1 Journey image gains `openssh-server`; a world verb that starts `sshd` and the HTTP server on loopback with a host alias pointing at them
- [ ] 6.2 `02-packages/08-a-private-marketplace-over-ssh.yml` — `market host <alias> <base>`, `market add alias:owner/repo`, `install`; checks on `agents.yaml`, `agents.lock`, the Store and the remembered transport
- [ ] 6.3 `02-packages/09-a-public-marketplace-with-no-keys.yml` — a lock spelled over SSH installs anonymously in a world with no key
