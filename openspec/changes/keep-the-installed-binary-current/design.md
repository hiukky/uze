## Context

`install.sh` places one file (`$XDG_BIN_HOME/uze` or `~/.local/bin/uze`) from
`releases/latest`, verified against `SHASUMS256.txt`, and forgets it did. The
same binary is the CLI, both TUI modes, the terminal server daemon and the
PATH shim a harness is launched through, so "the running uze" is several
processes at once, some of them long-lived. ADR-034 excluded a self-update
because nothing could tell the installer's file from one `cargo install` or a
package manager placed. See proposal.md for why this is worth doing now.

## Goals / Non-Goals

**Goals:**
- Replace the installer's file, and only it, without the operator asking.
- Never make a running process wait on, or be interrupted by, an update.
- Say what happened in the surface the operator is looking at.

**Non-Goals:**
- Updating a binary a package manager or `cargo` owns. It is told, not
  replaced.
- Signature verification beyond the checksum the installer already trusts.
  Signed provenance exists on every release; checking it needs `gh`, and the
  updater should not trust less or more than the installer does.
- Restarting the terminal server, or anything else, on the operator's behalf.
- Channels, pinning, or rollback. `UZE_VERSION` on the installer pins; a
  pinned install is still the installer's file, so pinning here would mean
  a receipt field nobody writes yet.

## Decisions

### Ownership is a receipt the installer writes

The installer records the physical path it wrote (`pwd -P`, so it compares
equal to `canonicalize` of `current_exe`) and the version that binary
reported. A binary replaces itself only when it *is* that file. Alternatives:
guessing from the path (`~/.local/bin` is also where `pipx` and others put
things — a guess that replaces someone else's file is the failure ADR-034
was written against), or asking the operator once (a prompt a first-run user
cannot answer meaningfully). This is the pattern `uv self update` and
cargo-dist's `axoupdater` use for the same reason. An install made before
this change has no receipt and is told about releases until it is reinstalled
once — no migration code, by project rule.

### In place, by rename, rather than versioned directories

Claude Code keeps each version in its own directory behind a symlink, which
buys rollback. Here the receipt names a file and the installer writes a file;
turning `~/.local/bin/uze` into a symlink would change what the installer
places and what every existing install looks like, for a rollback nothing
asks for yet. A rename on the same filesystem gives the property that matters
— no process ever `exec`s a half-written binary, and a running one keeps its
inode — at no layout cost. The staged file is made to report the version it
was downloaded as before it is renamed, the same last step the installer
takes.

### "Latest" is the redirect, not the API

`curl -w %{url_effective}` on `releases/latest` lands on `…/tag/v<version>`.
The REST API answers the same question but is rate-limited to 60 requests an
hour per address — shared by everyone behind one NAT — and needs JSON parsing
for one string. The redirect is also exactly what the installer resolves, so
the two can never disagree about what "latest" means.

### `curl` and `tar`, not an HTTP client crate

The installer and every harness installer UZE already runs reach the network
through `curl`. An HTTP client crate would be the largest dependency in the
tree for three GET requests, and would bring TLS configuration UZE would then
own. `sha2` verifies the archive; it is already in the tree through
`uze-core`.

### The asset is named from the running binary's own target

The installer asks `ldd` which C library the system has, because it has no
binary yet. A running binary knows the target it was built for
(`cfg!(target_env = "musl")`), which is the more accurate answer: a musl
binary on a glibc system should be replaced by a musl binary.

### The TUI checks on a thread; the CLI hands the check away

The TUI is long-lived, so one thread per process checks at launch and every
hour, and publishes the notice through a revision counter both modes compare
each tick — the same "resolve on a thread, draw what was resolved" shape
every other read in the workspace client has. A CLI command is budgeted in
milliseconds and a thread dies with it, so a stale answer is claimed in the
ledger (two commands a second apart start one check) and handed to a hidden
`uze self-update` in its own process group, detached from the terminal's
Ctrl+C. What it finds is what the next command says — the trade `gh` and
npm's notifier make.

### The notice is a section, pinned under the first steps

The first steps already established "chrome that belongs to uze sits at the
foot of the sidebar, in the section vocabulary". The notice uses the same
renderer, so it inherits the theme and the glyph set, with the closing mark
riding on the caption exactly as the steps' own does. It is never folded:
there is nothing under its header to fold, so the header's only target is the
mark.

### Three notices, one acknowledgement

`Installed` (the file on disk is newer than this process), `Updated` (this
process is a release the updater installed) and `Available` (a newer release
this binary will not install itself — foreign, `notify`, or a failed
download). Dismissing any of them records that version as acknowledged, which
also silences the others for that version: an operator who put away
"installed, restart to use it" does not need "updated" after the restart. The
CLI keeps its own `told` marker rather than sharing the acknowledgement: a
line printed after a command is not the operator choosing to put anything
away.

### Journeys and CI are off

A journey world is sealed and draws screens the journey wrote; a release
check would reach the Internet and could draw a notice no journey expects.
The runner sets `UZE_AUTOUPDATE=off`. `CI` defaults to off for the same
reason on any disposable machine.

### The architecture model

`docs/architecture/likec4/model.c4` gains GitHub Releases as an external
system and the relationship from UZE to it, since the binary now reaches it
on its own.

## Candidate ADRs

- **A binary replaces itself only when the installer's receipt names it** —
  supersedes ADR-034's non-goal, and the receipt becomes a contract between
  `install.sh` and the binary that is costly to change once installs rely on
  it.

## Risks / Trade-offs

- [A release changes the terminal protocol] → The next client replaces the
  running server; tabs restore and conversations resume (ADR-047), but a
  program mid-task in a pane restarts. This is what any upgrade does today;
  `docs/versioning.md` says so, and `UZE_AUTOUPDATE=notify` leaves the timing
  to the operator.
- [A compromised release host] → The checksum comes from the same host as the
  archive, so it detects corruption, not substitution — the installer's trust
  level exactly. Signature verification is the follow-up that raises both.
- [The receipt goes stale] → A binary moved or replaced by hand no longer
  canonicalizes to the receipt's path and is simply told about releases.
- [Two clients install at once] → Each stages under its own pid beside the
  target; both renames are atomic and write the same release.
- [No `curl` or `tar`] → The check fails quietly and is retried in an hour.
  Both are required by the installer that produced the receipt.

## Migration Plan

None in code. An existing install gains a receipt the next time the
installer runs; until then it is told about releases rather than updated.
Rollback is `UZE_AUTOUPDATE=off` plus `UZE_VERSION=<previous> install.sh`.
