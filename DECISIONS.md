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

---

## The codebase review

Four audits ran in parallel — cross-platform readiness, architecture, security,
performance. Everything below is a decision I took about *what to act on now*.
The rule I applied: this branch adds a platform, so it fixes **live defects on
platforms we now support** and **the boundaries that make the next platform a
vertical**. Everything else is recorded, not done — a macOS branch that becomes
a codebase-wide refactor is reviewable by nobody.

### ⚑ Measured first: Windows is a vertical, not a rewrite

`cargo check --target x86_64-pc-windows-msvc`, per crate:

| clean | blocked |
|---|---|
| `uze-core`, `uze-git`, `uze-theme`, `uze-integrations`, `uze-application`, `uze-extensions` | `uze-terminal` (22), `uze-testkit` (5) |

Six of nine compile today. The layering is paying for itself: the domain does
not know what OS it is on. Windows is one vertical (Unix sockets, pty,
permissions) plus a testkit fix.

**Compiling is not working.** `0 errors` says nothing about `/bin/sh` at
runtime or `0o755` modes. It is a floor, not a claim.

### ⚑ Fixed here — four things

**A live macOS bug this branch shipped exposure to.** `open_in_browser`
(`src/ui/worker.rs`) tried `xdg-open`, `sensible-browser`, `explorer.exe` —
none of which exist on macOS, and not `open`, which does. Every link the
workspace offered died there unless `$BROWSER` was set. Written when Linux was
the only reader; found by asking a second platform.

**Path containment asked in a Unix dialect.** `normalize_declared_relative_path`
is *the* shared safety predicate for manifest-declared paths, and both its
tests were string surgery: `Path::is_absolute` needs a prefix *and* a root on
Windows, so `\etc\passwd` is not absolute there — the exact regression the
function was written to fix, returning through another door — and splitting on
`/` never sees the `..` in `..\..\Windows`. Now asked of `Path::components()`.
The Windows spellings are tested as strings, so they are checked without a
Windows runner.

**A case collision is a review-evasion primitive.** `copy_tree` wrote entry by
entry in sort order with no collision check. On macOS and Windows — both
case-insensitive by default — a package shipping `SKILL.md` *and* `skill.md`
reads as two files in review, in `git show`, and to the containment walk
itself, while exactly one installs and the package chose which by naming it to
sort last. Refused now before a byte is written, and refused on Linux too:
what a package may contain must not depend on where it is installed.

**The `resume` bug, whose root cause was not what I twice said it was.** Fixed
and explained in its own commit; the short version is that every question
`spawn_task_evaluation` asks is about a *repository*, and it was asking three
of them with a directory that had just been deleted.

### ⚑ Recorded, not done — and why

Each of these is real. None belongs in a branch about macOS.

**Security — the hook wrapper drops the declared timeout.** `hooks.json`
validates `timeout` into `1..=300`; the generated wrapper has no slot for it
and no watchdog. A handler that blocks wedges the harness, and for a `deny`
group that is a permanent block on every matched tool call. Its own docstring
claims the opposite. *Not here* because the fix changes the wrapper ABI, its
three goldens, and the Lab evidence — that is ADR-040's change, not this one.

**Performance — every command pays for a full republish.** Measured: `uze
theme list` is 184 ms with 20 plugins and scales linearly, because
`ensure_default_plugins` republishes everything before `run()` dispatches. The
`Budgeted` classification says these are "a small JSON read plus a directory
listing"; the test guarding it times a fake in an empty home, so **all 15
budgeted commands point at a test that cannot see the cost**. The guardrail is
as wrong as the number. *Not here* because it touches install lifecycle, and
because the honest fix is to make the budget test measure the real binary
first — otherwise we would be fixing a number nobody is checking.

**`hooks.rs` is 3396 lines and seven concerns**, and holds per-vendor
knowledge that AGENTS.md places in the vertical. The seams are already drawn
as banner comments. *Not here* — ~8 files, its own review.

**`CapturingRunner` in `src/main.rs` reimplements the core process runner and
drops the process-group kill**, so a timed-out installer's forked helpers
survive — while the comment in core explains why that must not happen. Small
fix, real consequence. *Not here* only because it is unrelated to platform;
it should be next.

**Two architecture rules are narrower than the doctrine they guard.** The
colour rule forbids `Color::Rgb(` and misses two hard-coded `Color::Black` on
themed backgrounds; the glyph rule scans `src/ui` and misses five chrome
glyphs in `src/main.rs`. *Not here* — widening a rule turns violations red,
and that is a change that deserves its own diff.

**`uze-core` concern drift**: package discovery lives in `delivery/engine.rs`,
so `package` depends on `delivery` to read its own bytes; and capability
discovery is implemented twice with no test that the two agree.

**Dishonest `not(unix)` stubs.** The honesty correlates with the return type,
not with intent: where the signature returns `Result` the stub returns `Err`
and is honest (all eight symlink helpers); where it returns a value the stub
invents one — `try_lock → Ok(())` says two processes both hold the repository
lock, `is_executable → true`, `local_day_start → UTC` called local. Exactly
the `None`-means-unknown discipline this branch established, applied nowhere
else. *Not here* because they are unreachable until `uze-terminal` builds on
Windows — but they must be fixed **in the same change that makes it build**,
or they go live all at once.

### ⚑ Not doing: a macOS vertical for the Conformance Lab

I raised this and then withdrew it. Its isolation is Docker's — `--network
internal`, read-only root, dropped capabilities — and macOS runners have no
Docker. More to the point, the question it would answer is already answered:
all four harnesses read the same paths on macOS as on Linux (`~/.claude`,
`~/.codex`, `~/.config/opencode`, `~/.gemini`), so the vendor-behaviour risk
that justifies the Lab does not vary by platform here.

## Open, for you

- ~~Remove `TEMPORARY-MACOS-ONLY`~~ — **done**, 65 lines, after the first
  fully green macOS run. The full gate runs on this push.
- **Whether the four recorded items above become issues, one change, or a
  milestone.** I would take the runner unification and the budget-test repair
  first: both are small, and the second is what makes the performance number
  worth measuring at all.
- **`Journeys` as a required check** — see above.
- **A real install on a Mac.** Nothing here has run on hardware anybody owns.
  It is the one gap CI cannot close and the reason the docs say
  *experimental*.
