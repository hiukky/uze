# Decisions taken on `agent/ls10rd` (macOS support)

Decisions made while stabilising the macOS pipeline, for review. Each says
what was chosen, why, what was rejected, and how to undo it.

**This file is scaffolding for the pull request, not a document the
repository keeps.** Before merge it comes out; anything still true by then
belongs in `openspec/changes/support-macos/`, which is where this change is
recorded. Same rule as `TEMPORARY-MACOS-ONLY`.

Marked ⚑ where I decided alone and you have not seen it yet.

---

## Distribution

### ⚑ Ship macOS without notarization

The binaries are ad-hoc signed by the linker, which is all `curl | sh` needs.
A tarball fetched through a **browser** is quarantined by Gatekeeper and
needs one `xattr -d com.apple.quarantine uze`. Both the release notes and the
installation page say so.

Rejected: notarizing. It needs an Apple Developer account (US$99/yr) and
signing material in CI secrets — a recurring cost and a new class of secret,
for a path most installs do not take.

Reversing this adds; it breaks nothing.

### ⚑ Include Intel, cross-compiled on Apple Silicon

macOS ships one SDK carrying both architectures, so `--target
x86_64-apple-darwin` on an arm64 runner is an ordinary cross-compile. That
covers Intel Macs without depending on an Intel runner label, which GitHub is
in the middle of retiring.

The trade-off is real and stated in the docs: the `x86_64` build is **never
executed** before it ships.

Rejected: an Intel runner (label churn); shipping arm64 only (leaves Intel
Macs on "build from source").

### ⚑ Name the asset `<arch>-macos`

The existing rule is "the triple, minus the parts that say nothing". On Linux
that leaves `<arch>-linux-<libc>`. On macOS, `apple` and `darwin` are two
words for one fact, and there is no libc axis — a Mac has one.

Rejected: `<arch>-apple-darwin` (repeats itself), `<arch>-darwin` (a name
people do not use for their own machine).

Changing it later means changing `install.sh` and `release.yml` together;
they derive the same string from two ends and must not drift.

### ⚑ Reverse ADR-034's non-goal through an openspec change

ADR-034 lists "macOS/Windows installers" as an explicit non-goal. That is a
recorded decision, and the convention here is that an ADR is written when a
change is archived — so it is reversed by a new change,
`openspec/changes/support-macos/`, not by editing the archived ADR.

---

## The pipeline

### ⚑ Keep the Conformance Lab on Docker and on Linux

I raised "build the Lab image once instead of four times" and then withdrew
it after measuring: the four matrix legs start in the same second, so the
14 minutes are runner-minutes, not wall clock — and on a public repository
those are free. Building once would *reduce* runner-minutes and *increase*
the time you wait.

The two ways to make the build itself faster both cost something real:
caching the layer would freeze `uze setup` at a stale harness version and
silently break the channel-latest policy the nightly depends on; moving the
`cargo build` to the host would break `make lab-image` on macOS — the exact
developers this branch is for.

### ⚑ Pin every runner image

Seventeen runners said `ubuntu-latest`, a label GitHub repoints at the next
LTS on a date nobody here picks. Every action is already SHA-pinned, gitleaks
version *and* checksum, git-cliff and cargo-release to exact releases. The
machine all of that runs on was the one unpinned thing.

Moving to a new image is now a diff.

### ⚑ Group `ci.yml`, without renaming anything

Eleven jobs in a flat list answering three questions. Reordered and grouped:
is the code correct · is it safe to ship what we link · the parts that are
not the binary.

Order and comments only. **Every job name is a required status check on
`main`**, so renaming would break the merge button for everyone until the
protection was updated in the same window. If you want grouped *names*
(`code / Test`), that is a coordinated change, not a drive-by.

### ⚑ `Journeys` is not a required check, and its own header says it gates

`journeys.yml` says "That is why this gates a pull request rather than a
release". It does not — the required checks are the eleven `ci.yml` jobs
only. Conformance is not required either.

**Not changed**, because branch protection is yours. Either `Journeys`
becomes required, or the comment stops claiming it. I recommend the former:
it is 3m47s, vendor-independent, and already path-filtered.

---

## Product behaviour changed to make macOS work

These are not test fixes. Each was reproduced before it was changed, and each
holds on both platforms.

### The runtime endpoint steps over a directory too long for its socket

`sun_path` is 104 bytes on macOS. `XDG_RUNTIME_DIR` is somebody else's
variable, and the system temp dir there spends half the budget before UZE
adds anything. Length now disqualifies a candidate the way unwritability
does. Isolation is unaffected — the socket is named after a hash of
`UZE_HOME`.

### A `#!` script is named by itself, not by its interpreter

Linux sets `comm` from the file handed to `execve`; Darwin reports the image
that ended up running. Harness stand-ins are shell scripts, so every pane on
a Mac came back as `sh`, matched no harness, and the workspace saw no agents
at all. Measured, not assumed.

### A removed checkout is noticed from the disk, on a clock

The detection was driven by `/proc` renaming a removed cwd to `<path>
(deleted)` — a *changed* reading is the only thing that makes the server
speak. Where a kernel has no such spelling nothing changes and nothing is
sent.

### A pane is tied to its task through the slot, not the directory

`checkout_id` exists for exactly this ("whether or not its directory still
exists") and was not being used. Matching the resolved path meant a pane
stayed bound only if it had been bound *before* the removal. Reproduced as a
unit test on Linux first — it is not a macOS bug, it is one macOS made
likely.

---

## Open, for you

- **Remove `TEMPORARY-MACOS-ONLY`** (26 lines, 3 workflows) and let the full
  gate run once before merge. Not done yet — it is what makes each iteration
  ten minutes instead of a runner queue.
- **`Journeys` as a required check** — see above.
- **A real install on a Mac.** Nothing here has run on hardware anybody owns.
  It is the one gap CI cannot close and the reason the docs say
  *experimental*.
