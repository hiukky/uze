## Why

A `uze` installed by `install.sh` stays on the release it was installed at
until its operator remembers to run the installer again, and nothing tells
them there is anything to run it for. Every release since the first alpha
has shipped fixes an operator on the previous one never saw. The harnesses
UZE manages keep themselves current (Claude Code's native install updates
in the background and says so), and the tool that manages them should hold
itself to the same standard.

ADR-034 kept a self-update out of scope, and for a real reason: a binary
cannot know whether *it* is the thing to replace, or whether a package
manager, `cargo install` or a build tree put it there. This change answers
that question rather than setting it aside — the installer records the file
it placed, and that file is the only one ever replaced.

## What Changes

- `install.sh` leaves a receipt (`~/.uze/state/install.json`) naming the file
  it placed and the release it was.
- A binary running from the file a receipt names replaces itself when a newer
  release is published: downloaded from the same assets, verified against the
  same `SHASUMS256.txt`, made to report the release it claims to be, then
  renamed over the old file. Anything already running keeps the binary it
  started from; the next launch runs the new one.
- A binary running from anywhere else is never replaced. It is told that a
  newer release exists.
- Both sidebars of the terminal workspace carry a notice at their foot, in
  the same section vocabulary as the first steps: the update that was
  installed, the release now running, or the release available. Its row opens
  that release's notes on GitHub; its mark puts it away.
- A CLI command mentions a release once, on stderr, after its own output. It
  never waits on the network: a stale answer is handed to a detached, hidden
  `uze self-update`.
- `UZE_AUTOUPDATE=off|notify|on` (default `on`; `off` wherever `CI` is set
  unless the variable says otherwise). Journey worlds set it `off`.

## Capabilities

### New Capabilities
- `binary-updates`: how an installed `uze` learns of, installs and announces
  a newer release, and which binaries it may replace.

### Modified Capabilities

## Impact

- `install.sh` and its fixture suite (`tests/scripts/installer-test.sh`).
- New `src/self_update.rs` in the library crate; `src/main.rs` (hidden
  `self-update`, the notice after a command); `src/command_performance.rs`
  classifies the hidden command.
- Both TUI sidebars (`src/ui/management.rs`, `src/ui/orchestrator/render.rs`),
  their hits and the management worker's intents.
- `sha2` becomes a direct dependency of the root crate. It is already in the
  tree through `uze-core`; nothing new is compiled or vendored.
- Network: `github.com/hiukky/uze/releases`, through `curl`, the same way the
  installer and every harness installer UZE runs already reach the network.
- `journeys/journey.py` turns the check off inside a world.
- `docs/versioning.md` gains the section an operator reads to know what the
  updater does and how to stop it.
