## Context

ADR 018 set the contract every read-path cache here follows: two tiers,
fingerprint where something is cheaply stat-able, a bounded TTL where it
is not, mutation invalidation, fail-open. This change adds a third cache
under that contract and removes the costs that had grown around the first
two. See `proposal.md` for the measurements.

## Decisions

### 1. A marketplace catalogue is cached by TTL, not by asking the remote

A remote has no fingerprint. `git ls-remote` would be the honest freshness
check, and it is one SSH round trip — hundreds of milliseconds, over the
whole budget, on every listing. Serving the last catalogue seen for an
hour, then cloning once, is the same shape ADR 018 accepted for a harness
whose installer preserves mtimes. One hour rather than the detection
cache's day: a marketplace has a person pushing to it, and nothing here can
see that happen. `market add` of the same source is the on-demand refresh,
because it already clones to learn the name; a dedicated `market update`
verb was left for the day someone asks for it.

Stale-while-revalidate in the TUI — answer from the cache, refill on a
thread, refresh again — was considered and deferred: it needs a second
unsolicited `Refreshed` answer, which the management memory's in-flight
accounting does not model today. The worker already runs off the render
thread, so the once-an-hour refill is paid behind the screen either way.

### 2. Reading in place for a local source, and never for a Git one

A local marketplace's author is editing it; there is no remote to spare
and a stale copy would be a lie about the directory next to them. A Git
source is cloned into `cache/marketplaces/<name>/checkout` without its
`.git`, the same bytes `acquire` produces; keeping the repository and
fetching would have been a second Git convention under `~/.uze` next to
the one `acquisition::git` holds for untrusted remotes.

### 3. An expired entry answers when the refill fails

Fail-open in ADR 018 means a cache problem never fails a command. For a
remote it also has to mean: offline, the listing is what it was last time,
not empty. The refill is retried on the next read.

### 4. The embedded snapshot needs no directory to be compared

`has_update` compared two directory trees, one of them extracted from the
binary for the purpose and deleted afterwards — several times per refresh,
once per read model that carries `update_available`. The comparison is
between bytes the binary already holds and files in the Store; the
manifest's `source` path is contained the same way `resolve_plugin_source`
contains it, against a root that is never on disk.

### 5. A bootstrap that changes nothing writes nothing

Republishing was unconditional so a fresh home always had its catalogues
before the first attach. `publication` already says whether a view
matches; gating on it keeps that ordering. The one thing the gate would
have lost — a hand-removed generated envelope healed by the next command
— is closed by having `publication` stat the envelope, which is cheaper
than the rewrite it replaces. `state::record` compares before writing for
the same reason.

### 6. A harness lives on this machine's filesystems

Fifteen of the maintainer's forty-seven `PATH` entries are on `/mnt/c`, a
`9p` mount; a stat there is a network round trip. Every name not found
paid all of them, on every command, in the detection fingerprint and in
the shim check. The rule is stated in filesystem terms, read from
`/proc/self/mounts`, and limited to `9p`: an NFS home is a place a Linux
harness genuinely lives, a Windows drive is not — a harness UZE integrates
keeps its state under `$HOME` on this side. The runtime shim resolves with
the same walk, so a tool present only on the Windows side is no longer
what `uze`'s shim execs; that was interop by accident, not a supported
route.

### 7. The lock is the file, not its contents

`MutationLock::acquire` fsynced a file whose only load-bearing property is
that `create_new` made it. Eleven milliseconds on WSL, per lock, and the
doctor's maintenance pass and every profile listing took one. The pid
written inside is a courtesy to whoever finds a stale lock; it does not
need to be durable.

### 8. The refresh is one read model

`load_refresh_data` in the TUI called seven services in a fixed order.
That order is product knowledge (context is read at the detected
workspace's root, not the cwd), and being in `src/ui` it could not be
timed in isolation: `tui_application` composes the real registry.
`MachineSnapshot` moves the composition into the application, where the
budget test lives, and the worker maps it into its own `RefreshData`.

### 9. Ceilings: 25 ms, best of three, unoptimized, in-process

The spec promises 50 ms to a person on a release build. The tests run in
a debug build and measure single-digit milliseconds on a 4-vCPU WSL VM,
so a ceiling at the promise would let a fourfold regression through. Best
of three states what the path can do — a scheduler hiccup is not a
regression — and a live probe in the world costs half a second, so one
probe fails the ceiling on its own, before the probe counter does. A
mutation cannot be repeated in one world, so each of its attempts builds
a world of its own; the first version timed it once and failed under the
load of the whole suite running beside it.
Binary-level runs are not timed: process start and the test runner's
load dominate, and the two claims worth holding there — no writes, no
clone — are about the machine, not the clock.
