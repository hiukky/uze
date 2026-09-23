## Purpose

How a marketplace locator resolves to one canonical, transport-neutral
identity, and how each machine reaches the public or private repository
behind that identity without the project file ever saying how.

## ADDED Requirements

### Requirement: A marketplace has one canonical identity, decided by its URL alone
The system SHALL reduce a repository URL to `https://<host>/<path>` — host lowercased, path case kept, no `.git` suffix and no trailing slash — when it is spelled `https://`, `git@<host>:<path>` or `ssh://git@<host>/<path>` with no explicit port. An SSH URL with a user other than `git` or an explicit port SHALL be kept as written. The reduction SHALL depend on nothing but the URL, so that every machine computes the same identity. The system SHALL write only the reduced form into `agents.yaml`, `agents.lock` and the machine's marketplace registry, and SHALL compare two sources only by their reduced forms — in the registry, the Store, the marketplace mirror, the catalogue cache, the link check and the lock. `@ref` and `#subdirectory` SHALL survive the reduction unchanged.

#### Scenario: Every spelling of one repository records one identity
- **WHEN** the operator adds `hiukky/ai`, `github:hiukky/ai`, `https://github.com/hiukky/ai.git` and `git@github.com:hiukky/ai.git` in turn
- **THEN** each records `https://github.com/hiukky/ai`

#### Scenario: A host no table knows is reduced the same way
- **WHEN** the operator adds `git@git.acme.io:team/plugins.git` on a machine with no alias for `git.acme.io`
- **THEN** the recorded identity is `https://git.acme.io/team/plugins`, the same as on a machine that has one

#### Scenario: A local checkout's SSH origin does not reach the lock
- **WHEN** a project declares `path:` pointing at a checkout whose `origin` is `git@github.com:hiukky/ai.git` and `uze install` resolves it
- **THEN** `agents.lock` names `git: https://github.com/hiukky/ai`

#### Scenario: An SSH URL with a port is kept
- **WHEN** the operator adds `ssh://git@git.internal:2222/team/plugins`
- **THEN** the recorded identity is `ssh://git@git.internal:2222/team/plugins`

#### Scenario: A local checkout with no origin is resolvable here only
- **WHEN** the operator adds `./ai` and that checkout has no `origin`
- **THEN** the recorded identity is its absolute path and the command warns that a project declaring it resolves on this machine only

#### Scenario: Nested group paths are preserved
- **WHEN** the operator adds `gitlab:group/sub/plugins@v2#market`
- **THEN** the recorded identity is `https://gitlab.com/group/sub/plugins` with ref `v2` and subdirectory `market`

#### Scenario: An old spelling does not conflict with its canonical form
- **WHEN** the machine registry, the Store and the mirror hold a marketplace recorded as `git@github.com:hiukky/ai.git` and the operator adds `hiukky/ai` under the same name
- **THEN** no conflict is reported, the mirror is not re-cloned, and the entry is rewritten to `https://github.com/hiukky/ai`

### Requirement: A short locator resolves against one host and never another
A short locator SHALL resolve against exactly one host: the one its prefix names, or the machine's default host when it has none. The system SHALL NOT try the same name on another host when the repository is not found or not accessible on that one; the failure SHALL name the host that was asked and suggest the prefixed spelling for the other hosts in the table. On success the output SHALL name the full identity it resolved to. The short form SHALL NOT appear in any file UZE writes.

#### Scenario: Not found on the default host
- **WHEN** the operator adds `hiukky/ai`, the default host is `github`, and no transport reaches `https://github.com/hiukky/ai`
- **THEN** the command fails, nothing is recorded, no request is made to any other host, and the message suggests `gitlab:hiukky/ai` among the prefixed forms

#### Scenario: What a short locator resolved to is shown
- **WHEN** the operator adds `hiukky/ai` and it succeeds
- **THEN** the output names `https://github.com/hiukky/ai`, so a repository of the same name on the wrong host is visible at the moment it was chosen

#### Scenario: The default host is the operator's choice
- **WHEN** the operator has run `uze market host gitlab` and adds `hiukky/ai`
- **THEN** the recorded identity is `https://gitlab.com/hiukky/ai`

#### Scenario: An unknown prefix is refused
- **WHEN** the operator adds `gitlub:hiukky/ai` or `github.com:hiukky/ai`
- **THEN** the command fails naming the aliases the table holds, and nothing is fetched

### Requirement: The host table is machine-scoped and input only
The system SHALL ship the aliases `github` (github.com, the default), `gitlab` (gitlab.com), `codeberg` (codeberg.org) and `bitbucket` (bitbucket.org), and SHALL let the operator, through `uze market host`, list the table, choose the default, define an alias for an `https://` base (a loopback `http://` base is also accepted), and remove an alias they defined. A base MAY carry a path. Alias names SHALL match `[a-z0-9-]+` and SHALL NOT be `http`, `https`, `ssh`, `git` or `file`. A built-in alias SHALL NOT be redefined or removed. Removing the alias that is the default SHALL return the default to `github` and say so. The table SHALL live in the machine's own state, SHALL NOT be read from or written to any project file, and SHALL play no part in canonical identity or in how a repository is reached; no project SHALL need a machine to hold a particular alias in order to install.

#### Scenario: A self-hosted forge
- **WHEN** the operator runs `uze market host work https://git.acme.io` and then `uze market add work:platform/plugins`
- **THEN** the recorded identity is `https://git.acme.io/platform/plugins`

#### Scenario: An alias for an organisation
- **WHEN** the operator defines `oss` as `https://codeberg.org/my-org` and adds `oss:plugins`
- **THEN** the identity is `https://codeberg.org/my-org/plugins`, and its SSH attempt is `git@codeberg.org:my-org/plugins.git`

#### Scenario: A built-in alias cannot be repointed
- **WHEN** the operator runs `uze market host github https://git.evil.example`
- **THEN** the command fails and the table is unchanged

#### Scenario: Removing the default alias
- **WHEN** `work` is the default and the operator runs `uze market host work --remove`
- **THEN** `work` is gone, the default is `github`, the output says so, and every registered marketplace keeps its URL

#### Scenario: A project is resolved the same way on every machine
- **WHEN** two machines with different default hosts and different aliases run `uze install` on the same `agents.yaml`
- **THEN** both reach the same repositories and write the same `agents.lock`

### Requirement: A public repository is reached without credentials
For an `https://` identity, the system SHALL attempt anonymous HTTPS first, with no credential helper, no agent, no `.netrc` and no prompt. A repository that answers anonymously SHALL be installed without any credential being consulted, whatever spelling the manifest or lock used.

#### Scenario: A public marketplace on a machine with no keys
- **WHEN** a machine with no SSH key and no credential helper installs a project whose lock names a public `https://github.com/…` marketplace
- **THEN** the install succeeds

#### Scenario: A lock spelled over SSH still installs a public repository anonymously
- **WHEN** a lock written before this change names `git@github.com:hiukky/ai.git`, the repository is public, and the machine has no SSH key
- **THEN** the install succeeds over anonymous HTTPS to `github.com/hiukky/ai`

### Requirement: A private repository is reached with the operator's own credentials, on the same host
When anonymous HTTPS fails for any reason other than the host not resolving, the system SHALL try HTTPS with the operator's configured Git credentials, and then SSH at `git@<host>:<path>.git`, against the same host and repository path. The operator's `~/.ssh/config` SHALL be honoured. SSH SHALL run non-interactively and SHALL refuse a host whose key the operator has not accepted before offering any key. The authenticated HTTPS attempt SHALL NOT follow a redirect. The system SHALL NOT store, print, trace, pass on a command line, or write into any file a credential it used, and SHALL NOT prompt. The transport that answered SHALL be remembered per repository in the cache tier and tried first on the next fetch; deleting that memory SHALL cost only the attempts it saved. When the host does not resolve, the system SHALL report the machine as offline and SHALL NOT try another transport.

#### Scenario: Private over SSH with a key in the agent
- **WHEN** the repository is private, the operator's key is loaded in `ssh-agent`, and no credential helper is configured
- **THEN** the fetch succeeds over SSH, and the next fetch goes to SSH first

#### Scenario: A self-hosted forge the machine never configured
- **WHEN** a lock names `https://git.acme.io/team/plugins`, the repository is private, the machine has no alias for `git.acme.io`, and the operator's key is accepted by `git@git.acme.io`
- **THEN** the fetch succeeds over SSH

#### Scenario: Private over HTTPS with a URL-scoped credential helper
- **WHEN** the repository is private and the operator's Git config sets `credential.https://<host>.helper` to a helper holding a token for the host
- **THEN** the fetch succeeds over HTTPS, and the token appears in no UZE file, log, trace or process argument list

#### Scenario: A redirect does not carry the credential elsewhere
- **WHEN** the authenticated HTTPS attempt is answered with a redirect to another host
- **THEN** the attempt fails and the other host receives no request

#### Scenario: An SSH host the operator never accepted
- **WHEN** the SSH attempt reaches a host absent from the operator's `known_hosts`
- **THEN** it fails without offering a key and without waiting on a prompt, and the message says how to accept the host

#### Scenario: HTTPS blocked, SSH open
- **WHEN** port 443 of the host refuses connections and SSH with the operator's key is accepted
- **THEN** the fetch succeeds over SSH

#### Scenario: No transport has access
- **WHEN** the repository is private and neither the helper nor SSH is accepted
- **THEN** the command fails, nothing is recorded, and one message names the identity, each transport tried, and the reason each gave

#### Scenario: Offline
- **WHEN** the host name does not resolve
- **THEN** the failure says the machine is offline and no authenticated transport is attempted

### Requirement: The acquisition environment admits authentication and the operator's network, and no configuration
Every attempt SHALL use the operator's proxy settings and certificate authority settings, from the environment and from their Git config. The authenticated attempts SHALL additionally use the operator's environment, without any `GIT_*` variable, and the operator's `credential.*` settings with their URL scopes. No other setting of the operator's system or global Git config SHALL apply to any attempt. Repository hooks SHALL stay disabled, submodules SHALL NOT be fetched, and no terminal prompt SHALL be shown. Plain `http://` SHALL be allowed only to a loopback host. A URL carrying inline credentials SHALL still be refused before use.

#### Scenario: Behind a company proxy with a private CA
- **WHEN** the operator's environment sets `https_proxy` and their Git config sets `http.sslCAInfo` for a self-hosted forge
- **THEN** the anonymous and authenticated attempts go through that proxy and trust that CA

#### Scenario: A global alias or filter does not run
- **WHEN** the operator's global Git config defines an alias and a `filter` driver
- **THEN** neither is in effect during any acquisition attempt

#### Scenario: Plain HTTP off loopback is refused
- **WHEN** a manifest declares `git: http://git.acme.io/team/plugins`
- **THEN** no attempt is made over plain HTTP, and the failure says why

#### Scenario: A token in the URL is refused
- **WHEN** the operator adds `https://user:token@github.com/hiukky/ai`
- **THEN** the command fails before any Git process starts and the token appears in no file, log or trace
