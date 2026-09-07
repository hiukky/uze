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

Unchanged. Docker there is the experiment, not the packaging: `--network
internal`, read-only root, `cap_drop ALL`, rootless, tmpfs-only writes are
what make "real harness binary, synthetic provider, zero Internet, zero
tokens" a claim rather than a hope. macOS runners have no Docker, so the Lab
stays a Linux tier.

### ⚑ Build the Lab image once — reversing a decision taken earlier on this branch

Earlier on this branch I raised "build the image once instead of four times"
and **withdrew it** after measuring, with this reasoning: the four legs start
in the same second, so the 14 minutes are runner-minutes rather than wall
clock, and on a public repository those are free. Building once would reduce
runner-minutes and increase the time you wait.

That cost analysis is correct, and it is not the reason this is now split
out. The argument it missed is about evidence, not minutes.

The Dockerfile provisions the harnesses with `uze setup`, whose policy is
channel-latest — `conformance.yml`'s own header says a vertical "goes red on
a vendor release with no commit of ours behind it". Four independent builds
are four independent installs. A vendor publishing during a run leaves the
Claude leg testing one version and the Codex leg another, and a matrix whose
legs stand on different ground cannot be read: a difference between two legs
has to be a difference between two harnesses.

So: one `Image` job, `docker save | zstd`, an artifact, four `docker load`.

- **Channel-latest is preserved.** The image is still built fresh on every
  run — it is built *once* per run, not cached across runs. The layer-cache
  idea I rejected earlier (it would freeze `uze setup` at a stale version) is
  still rejected, for the same reason.
- **The cost is what I measured then**: about +40s of wall clock on the
  longest leg, for the `docker load` that did not exist before. In exchange,
  −8.6 runner-minutes per run and four legs of one world.
- **It applies twice.** `conformance-stability.yml` did the same thing three
  times a night, and it is the promotion gate — the one place where "is this
  vertical stable" is being asked, and the least defensible place for the
  ground to move between legs.

Reversing this is deleting the `Image` job and putting the `docker build`
step back into the matrix.

### ⚑ Pin every runner image

Seventeen runners said `ubuntu-latest`, a label GitHub repoints at the next
LTS on a date nobody here picks. Every action is already SHA-pinned, gitleaks
version *and* checksum, git-cliff and cargo-release to exact releases. The
machine all of that runs on was the one unpinned thing.

`macos-latest` escaped the first pass — the only unpinned runner left, on the
one platform this branch is about. It is `macos-15` now in the gate and in
`release.yml`'s two Apple packaging legs, which is what "both Apple targets
on one Apple Silicon runner" was already assuming without saying.

Moving to a new image is now a diff.

### ⚑ Three stages, and only two edges

Eleven jobs in a flat list answering three questions, and six workflows with
no `needs:` between them anywhere. The grouping is now real rather than
comments: stage 1 verifies, stage 2 proves, stage 3 closes.

The temptation is six stages. It does not pay — under about ninety seconds a
gate costs more information than it saves time, and every stage 1 job answers
inside two minutes, so stage 1 keeps no internal ordering at all. Two edges
earn their keep: `lint` before the journeys tier, and `Image` before the four
verticals.

Journeys and Conformance become `workflow_call` and are invoked from
`ci.yml`, because `needs:` does not cross a workflow boundary. Both keep
their own `schedule:` and `workflow_dispatch:`, so the nightly is unchanged;
`ci.yml` skips the two calls on a schedule event so nothing runs twice.

### ⚑ One `gate`, so branch protection stops naming jobs

Eleven job names were listed one by one as required checks on `main`. I said
earlier on this branch that renaming a job "would break the merge button
until the protection was updated in the same window" — which is exactly what
folding macOS in does to `Test`, now `Test (linux)` and `Test (macos)`.

Rather than swap eleven names for twelve, `gate` is a job that needs every
other one and fails if any reports `failure` or `cancelled`. `skipped` passes
on purpose: that is what lets `changes` turn off an expensive tier without a
documentation change waiting forever on a check that will never report.

**This needs one action from you, and the pull request cannot merge without
it**: replace the eleven required contexts with the single context `gate`.

    gh api -X PATCH repos/hiukky/uze/branches/main/protection/required_status_checks \
      -F strict=true -f 'contexts[]=gate'

It also settles the older question of whether `Journeys` should be required.
It now is — through `gate`, along with Conformance, without either becoming
a name in repository settings that has to be maintained by hand.

### ⚑ A `changes` job instead of four `paths:` blocks

`paths:` can only be written at the top of a workflow, which is the only
reason `macos.yml` was a separate file. Folding it into a matrix needs the
filter to survive as data, so it moves into a job whose outputs the
consumers read — including the platform matrix itself, built as JSON.

Written with `git diff` and `grep -E` rather than `dorny/paths-filter`: it is
about twenty auditable lines, and this repository's rule for adding a
dependency is to ask whether the standard tools can do the job first.

### ⚑ Restore the cache everywhere, save only from `main`

The repository sat at **9.91 GB of its 10 GB** Actions cache allowance, which
means GitHub was evicting by LRU on every run — a "warm cache" was a coin
toss, and a branch writing its own 1.75 GB set evicted what the next branch
was about to read. `save-if: github.ref == 'refs/heads/main'` everywhere in
the gate.

`test` and the journeys job also now share one key: they build the same
workspace with the same toolchain and their two entries carried an identical
content hash (`6ff13d87`), 239 MB and 260 MB, per branch.

`release.yml` is deliberately left writing its caches: it runs rarely, and a
cold release build is a real cost on an operation somebody is waiting on.

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
- **Swap the eleven required contexts for `gate`.** This is the one thing
  that blocks the merge: `Test` is a required check and is now
  `Test (linux)` / `Test (macos)`, so the old name will never report again.
  The command is in "One `gate`" above. It also settles `Journeys` and
  Conformance, which become required through `gate` without either being a
  name anybody maintains by hand.
- **A real install on a Mac.** Nothing here has run on hardware anybody owns.
  It is the one gap CI cannot close and the reason the docs say
  *experimental*.
