## Context

See `proposal.md - Why` for the measured problem. Relevant existing
building blocks:

- `HarnessDetection { present, version }` (`crates/uze-core/src/
  integration.rs`) already derives `Serialize`/`Deserialize` — it is
  naturally persistable as-is.
- `IntegrationPort::detect()` is documented today as "Read-only; performs
  no filesystem writes" — that contract has to be preserved for the trait
  method itself; caching must live in a wrapper layer, not inside each
  integration's `detect()` implementation.
- `UzeHome::cache_dir()` already exists (`crates/uze-core/src/home.rs`) and
  is explicitly reserved for exactly this kind of thing — a comment in
  `crates/uze-core/src/acquisition.rs` deliberately avoids reusing it for
  package acquisition scratch space *because* "a cache would be a second
  place packages live," i.e. `cache_dir()` is where non-authoritative,
  reconstructable state belongs. It is currently unused for any real cache.
- `ProcessSpec`/`ProcessRunner` (`crates/uze-core/src/provisioning.rs`)
  already abstracts process execution behind a trait for provisioning
  flows, including a fake runner used in tests. `detect()` itself does not
  go through `ProcessRunner` today (each integration's `detect_binary`
  calls `std::process::Command` directly) — bringing detection onto the
  same seam is what makes it fakeable for a performance-budget test.
- `.detect()` is called from seven+ sites across `uze-application`
  (`application.rs`, `doctor.rs`, `context.rs`, `lifecycle/install.rs`)
  and from within each integration's own `attach`/`provision` logic. Some
  of these are within the same logical command execution (e.g. three calls
  inside one `context inspect`).
- Critically, `application.rs`'s `prepare_detected_integrations()` (the
  function backing `ensure_default_plugins()`) calls `.detect()` **twice**
  per integration in one call, and `ensure_default_plugins()` is invoked
  unconditionally by `src/main.rs`'s `run()` (line 364, plus a second call
  at line 311 inside `add`'s handler) **before dispatching to any
  subcommand**, and by `src/ui/worker.rs` (line 192) on every TUI
  startup. This single shared bootstrap path is why the measured slowness
  is not confined to `status`/`list`/`doctor`: `marketplace list` (8.52s),
  `plugin list` (9.08s), and `inspect <id>` (9.1-9.7s) — all purely local
  reads — are equally affected, as is TUI startup (already flagged by an
  existing code comment at the `ui/worker.rs` call site). Fixing this one
  call site is what gives the fast path its broad reach; the additional
  call sites in `context.rs` and `doctor.rs` are on top of it, not
  instead of it.

## Goals / Non-Goals

**Goals:**
- One external probe (`<harness> --version`) per distinct harness per
  logical command invocation, not per call site.
- Warm-cache reads add no subprocess spawn and no more than a `stat()`-class
  filesystem check — see `proposal.md - Before/After Latency Projection`
  for the projected numbers this targets (low tens of milliseconds warm,
  vs. 8-12s today).
- Cache correctness with zero required operator action: installing,
  updating, or removing a harness — whether done through UZE or outside
  it — is reflected without the operator having to know a cache exists,
  clear one, or pass a flag.
- A concrete, enforced performance budget: a cache-warm run of every
  budget-bound command completes in under 50ms (see performance-budget
  test, tasks.md §4.6), chosen with generous margin over the projected
  single-digit-ms warm path so the test is a real regression guard, not a
  flaky timing assertion.
- A test that fails CI if a call site regresses to bypassing the cache.
- A durable guardrail against this class of regression recurring: a new
  CLI command SHALL NOT be addable without the test suite forcing a
  decision about whether it is budget-bound or justified-slow (decision
  6) — this is the direct answer to "how do we make sure this never
  happens again," not just a fix for the commands measured today.

**Non-Goals:**
- Caching every expensive operation in UZE (package acquisition, plugin
  install, registry fetches) — those are explicitly the "justified to be
  slow" cases the proposal carves out. This change only covers harness
  detection; the spec frames the general budget principle, but the
  implementation here is scoped to `detect()`.
- A network-backed or cross-machine cache — this is a local, per-`UZE_HOME`
  cache only.
- Changing what `detect()` returns or how any individual integration
  determines presence/version.

## Decisions

**1. Two-tier cache: in-process memoization over an on-disk, invalidated store.**

- *In-process*: a `DetectionCache` owned by `UzeApplication` (a
  `RefCell<HashMap<&'static str, HarnessDetection>>` keyed by
  `integration.id()`) memoizes for the lifetime of one command invocation.
  All internal call sites are updated to go through
  `self.detect_cached(integration)` instead of `integration.detect()`
  directly, collapsing the three in-`context.rs` calls (and every other
  duplicate) to at most one live probe per integration per process.
- *On-disk*: `DetectionCache` falls through to a JSON file at
  `UzeHome::cache_dir().join("harness_detection.json")` before doing a
  live probe, and writes back after any live probe. This is what makes
  `status`/`list`/`doctor` fast *across* separate CLI invocations, not
  just within one.
- Alternative considered: cache only on-disk, no in-process layer. Rejected
  — it still leaves the documented 3x redundant call inside
  `context.rs` paying repeated (de)serialization + `stat()` overhead per
  call, and does not remove the structural bug of call sites not sharing
  state within a run.
- Alternative considered: cache only in-process (no persistence).
  Rejected — this would still pay one full slow probe (e.g. ~11.5s for
  `gemini --version`) on *every* CLI invocation, which is the actual
  complaint; the cross-invocation persistence is the point.

**2. Invalidation: resolved-path + mtime fingerprint, with a bounded TTL safety net.**

- On read, the cache entry stores the resolved executable path (from
  `which`-style `PATH` resolution) and that file's mtime alongside the
  `HarnessDetection`. A read re-resolves the executable's path and mtime
  (cheap `stat()` calls, no subprocess) and treats the entry as valid only
  if both match; otherwise it falls through to a live probe and overwrites
  the entry.
- A bounded TTL (24h) is additionally enforced as a safety net for any
  fingerprint blind spot (e.g. a vendor installer that replaces file
  content without changing the resolved path in a way our resolution
  logic observes) — an entry older than the TTL is treated as stale
  regardless of fingerprint match.
- Alternative considered: TTL only (no fingerprint). Rejected — a harness
  updated minutes after a cache write would silently report the old
  version until the TTL lapses, which is a correctness regression the
  proposal explicitly rules out ("don't serve a stale... version forever").
- Alternative considered: fingerprint only (no TTL). Rejected as
  insufficiently defensive — the fingerprint depends on `PATH` resolution
  and mtime semantics holding in every installer's case, which is not a
  guarantee UZE controls; TTL bounds the worst case.
- This is an architecturally significant, hard-to-reverse choice (it
  defines the caching/staleness contract every future consumer of
  `detect()` will rely on) — see ADR: Cache harness detection with
  fingerprint + TTL invalidation.

**3. Automatic invalidation on UZE-driven writes; no manual refresh flag.**

- Whenever UZE itself changes a harness's installed state — `provision()`
  succeeding (install or update) — it writes the fresh `HarnessDetection`
  it already obtained while verifying that operation straight into both
  cache tiers for that integration, instead of leaving the pre-action
  entry to be caught later by a read-time fingerprint check. This is a
  write-through of a result UZE already computed, not an extra probe:
  `provision()` already calls `detect()` to verify success.
- No `--refresh` flag is exposed anywhere. Freshness for UZE-driven
  changes is guaranteed by the write-through above; freshness for
  out-of-band changes (the operator installs/updates/removes a harness
  outside UZE) is handled by the fingerprint check on read (decision 2),
  with the TTL as the only remaining backstop for the case fingerprinting
  can't observe — e.g. an installer (observed as plausible for npm-based
  installs, which is how `gemini` is distributed) that preserves the
  target file's original packaged mtime instead of stamping install time.
- Alternative considered: an explicit `--refresh` flag on `status`/`list`/
  `doctor`. Rejected on user feedback during design review — if
  write-through-on-write plus fingerprint-on-read is correct, a manual
  flag is redundant for the common cases and only adds CLI surface for an
  edge case the TTL already bounds to at most 24h; it would also
  reintroduce exactly the failure mode this change removes elsewhere: an
  operator needing to know an internal cache exists to get correct
  behavior. If the TTL bound proves too coarse in practice, the response
  is to tighten the TTL or improve the fingerprint, not to add a flag
  that shifts the burden back onto the operator.

**4. Fakeability seam is `IntegrationPort`, not `ProcessRunner` — revised during implementation.**

- Originally planned: move `detect_binary` onto `&dyn ProcessRunner` (like
  `provision()`) so a fake runner could simulate a slow harness. Reverted
  after starting it: `detect()`'s trait signature (`fn detect(&self) ->
  HarnessDetection`, no runner parameter) is called from `install()` and
  from `UzeApplication`'s read paths, neither of which has a
  `ProcessRunner` in scope the way `provision()` does — threading one
  through would ripple the trait signature across every `IntegrationPort`
  implementation and every call site for a benefit this change does not
  need.
- Instead: `DetectionCache`/`detect_cached` are tested by faking at the
  `IntegrationPort` trait boundary directly — a test-only struct
  implementing `IntegrationPort` with a `detect()` that sleeps and counts
  calls, plugged into `UzeApplication::new(home, vec![Box::new(fake)])`
  exactly the way the codebase already substitutes integrations for tests
  elsewhere. This is sufficient because the caching layer's correctness
  (memoize, persist, invalidate) does not depend on *how* `detect()`
  obtains its answer — only on `detect_cached` calling it at most once per
  command and reusing the result thereafter. The real subprocess-spawning
  `detect_binary` implementations are unchanged by this decision and
  remain untouched by this change.

**5. Fail-open on cache corruption or unreadable cache file.**

- A missing, unreadable, or malformed cache file is treated as an empty
  cache (fall through to live probe for every entry), not an error —
  consistent with the existing fail-open convention documented on
  `HarnessRuntimeContribution` in `integration.rs`. A cache is a
  reconstructable optimization; it must never be a new failure mode for
  commands that worked before it existed.

**6. A classification registry plus an exhaustiveness test, not code review discipline, enforces coverage of new commands.**

- Every leaf command clap resolves for `Cli` (top-level `Command` variants
  in `src/main.rs` and each nested subcommand enum — `context`,
  `marketplace`, `plugin`) must appear in a small, explicit registry
  mapping command path → `PerformanceClass::Budgeted` or
  `PerformanceClass::JustifiedSlow(&'static str)` (the `&str` is the
  human-readable reason, e.g. "performs a network install", satisfying
  the spec's "stated reason" requirement).
- A test walks clap's own command tree via `Cli::command()`
  (`clap::CommandFactory`, already a dependency) to enumerate every leaf
  subcommand name/path *without* hand-maintaining a duplicate list next
  to the `Command` enum, and asserts every entry it finds has a matching
  registry entry — and, symmetrically, that the registry has no entry for
  a command that no longer exists (catches the registry going stale in
  the other direction). A command added to the `Command` enum without a
  registry entry makes this test fail immediately, by name.
- A second test asserts that every `PerformanceClass::Budgeted` entry has
  a corresponding performance-budget test in the suite (tracked by the
  same registry driving the loop in tasks.md §4.6, rather than a separate
  hand-maintained list) — so a command marked "should be fast" cannot
  silently ship with no test actually checking that.
- Alternative considered: rely on code review / CLAUDE.md-style
  guidance ("remember to add a perf test for new commands"). Rejected —
  this is exactly the failure mode that produced the problem this whole
  change exists to fix: `ensure_default_plugins()` becoming a hidden,
  unmeasured bottleneck on nearly every command without anyone deciding
  that was acceptable. A convention nobody's tooling checks is not a
  guardrail.
- Alternative considered: a blanket lint/clippy rule flagging any
  `std::process::Command` construction outside an allow-list. Rejected —
  too coarse (it would also flag legitimate, justified-slow provisioning
  code) and doesn't verify the *budgeted* side actually meets its budget;
  the registry + exhaustiveness + budget-test-presence combination checks
  both directions precisely.

**7. `IntegrationPort::install()` had its own uncached `detect()` call — discovered by measuring, not by code reading.**

- After wiring `detect_cached` into every known call site (decision 1's
  section 3 work), a real end-to-end timing check (`time ./target/debug/
  uze status`, repeated) still showed ~2s per invocation, all logged as
  cache *hits*. Instrumenting confirmed the remaining cost was entirely
  inside `prepare_detected_integrations`'s call to `integration.install(
  &self.home)`: every integration's `install()` (`claude.rs`, `codex.rs`,
  `gemini.rs`, `opencode.rs`) called `self.detect()` directly, again,
  uncached, purely to fetch a version string for `state::record`(...) —
  a second, independent live probe on top of the one `detect_cached`
  had just performed one line earlier in the same function.
- Fixed by changing `IntegrationPort::install`'s signature to
  `fn install(&self, home: &UzeHome, detection: &HarnessDetection)`,
  so the caller's already-obtained (cached) result is passed in rather
  than re-derived. Both real call sites (`provision_and_prepare`,
  `prepare_detected_integrations`) already had a `HarnessDetection` in
  scope at the point they called `install`, so this was a pure
  plumbing fix — no new probe anywhere, one fewer than before.
- **Consequence for how this class of bug gets caught going forward**:
  grep-driven auditing of `.detect()` call sites (this change's original
  plan) missed this one because `install()`'s `self.detect()` call is
  syntactically identical to a legitimate one — nothing marks it as
  redundant without knowing the caller already has an answer. The
  performance-budget test (tasks.md §4.6) is what actually catches this
  class of regression, because it measures wall-clock behavior rather
  than trusting that every call site was audited correctly. This is
  itself a concrete argument for decision 6 (the classification +
  exhaustiveness mechanism): audits find what you think to look for;
  a real measurement finds what's actually there.
- End-to-end result on the dev machine used throughout this change:
  `uze status`/`doctor`/`list`/`marketplace list`/`plugin list`/
  `inspect`/`context inspect` all went from 8-12s to ~50-58ms
  (debug build, full process including startup — not just the cached
  lookup itself) after this fix, versus still ~2s with only the
  `detect_cached` wiring and this bug still present.

## Risks / Trade-offs

- **[Risk] Fingerprint check adds a `stat()` per integration on every
  command, even cache hits.** → Mitigation: this is the entire point (a
  filesystem stat instead of a subprocess spawn) — expected to be
  microseconds vs. seconds; the performance-budget test asserts the actual
  bound, not just relative improvement.
- **[Risk] Concurrent `uze` invocations writing the cache file
  simultaneously could corrupt it.** → Mitigation: writes go through a
  write-to-temp-file-then-rename sequence (atomic on the same filesystem);
  a reader that still hits a corrupt file falls back to live probe
  (fail-open, decision 5) rather than erroring.
- **[Risk] A 24h TTL means a harness reinstalled with an unchanged
  resolved path *and* unchanged mtime (unlikely but possible with some
  packaging tools, e.g. npm preserving packaged timestamps) stays stale
  for up to 24h, with no manual override.** → Mitigation: 24h is a
  conservative starting point, not a hard architectural constant — if
  this surfaces in practice, tighten the TTL or strengthen the
  fingerprint (e.g. also hash a cheap identifying slice of the binary)
  rather than reaching for a manual flag, which was deliberately rejected
  (decision 3) in favor of keeping correctness fully automatic.
- **[Risk] The classification registry (decision 6) could become a
  rubber stamp** — someone marks a command `JustifiedSlow` to make the
  exhaustiveness test pass without a real justification. → Mitigation:
  the mechanism's job is narrowly to catch the "nobody decided" failure
  mode (which is what actually happened here), not to replace code
  review's judgment on whether a *stated* reason is a *good* reason; the
  required reason string at least makes an unjustified exemption visible
  and reviewable in the diff, rather than invisible as today.
- **[Trade-off] This only fixes `detect()`.** Other expensive local probes
  (if any exist elsewhere in the codebase) are out of scope for this
  change, per Non-Goals — `specs/cli-performance/spec.md` states the
  general budget principle so future work has a spec to extend against,
  but this change's tasks only touch detection.

## Migration Plan

- Additive: existing call sites are edited in place to call
  `self.detect_cached(...)` instead of `integration.detect()`; no data
  migration needed since the cache file does not exist yet (first run
  creates it).
- Rollback is simply reverting the change — no persisted state format is
  shared with any other feature, and a stale/missing cache file is always
  safely ignorable (decision 5).
- No LikeC4 model update needed: this does not add or remove a container,
  component, or a relationship between them — it changes internal
  behavior of the existing `uze-core`/`uze-application` boundary
  (`IntegrationPort::detect()` remains the same trait method signature;
  the cache sits in the application layer that already owns integration
  orchestration).
