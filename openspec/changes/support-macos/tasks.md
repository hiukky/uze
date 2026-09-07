## 1. Ask the kernel the same questions everywhere

- [x] 1.1 `crates/uze-terminal/src/process_probe.rs`: peer pid, executable,
      cwd, environment value — `/proc` on Linux, `LOCAL_PEERPID` /
      `proc_pidpath` / `PROC_PIDVNODEPATHINFO` / `proc_name` /
      `KERN_PROCARGS2` on macOS, `None` everywhere else.
- [x] 1.2 `runtime.rs` composes them once: `listener_running_this_executable`,
      `foreground_status` and `shim_launched_name` lose their per-platform
      copies.
- [x] 1.3 The eight terminal tests pinned to Linux run wherever the probe
      answers.
- [x] 1.4 `shell_path.rs`: bash reads `.bash_profile` on macOS, with a test
      per platform.
- [x] 1.5 The `/proc` sweep in `kill_process_group` documents that it finds
      nothing off Linux, and why that is correct.
- [x] 1.6 `docs/architecture/invariants.md` records the property and names
      the tests holding it.

## 2. Make the suites hold there

- [x] 2.1 `uze-testkit`: canonicalize the scratch root (`/private/var`), and
      `socket_scratch` for the tests that bind a Unix socket (`SUN_LEN` is
      104 bytes on macOS and the system temp dir spends half of it).
- [x] 2.2 `fake_harness`: `grep -vxF` instead of GNU `sed -i`.
- [x] 2.3 `/bin/sh -c` instead of `/bin/true` and `/bin/printf`, which macOS
      keeps under `/usr/bin`.
- [x] 2.4 The foreground-status test canonicalizes `/tmp`, which is a
      symlink on macOS and comes back from the kernel as `/private/tmp`.

## 3. Journeys on both platforms

- [x] 3.1 `journey.py`: `process_environ`/`process_cwd` per platform, and a
      `process:` check that stops the run where it cannot observe instead of
      answering "not running".
- [x] 3.2 `JOURNEY_WORLDS` resolved through `realpath`.
- [x] 3.3 Journeys run natively on both, from one composite action.

## 4. Prove it in CI

- [x] 4.1 `.github/workflows/macos.yml`: build, run, clippy, test, journeys.
- [x] 4.2 Path-filtered on pull requests; every step runs even after a
      failure, and the suite is `--no-fail-fast`.
- [x] 4.3 Fold into `ci.yml` as a runner matrix and delete the workflow —
      done once the first run settled green (7m03s, everything passed).
      `Test` and `E2E - UZE` carry a `platform` axis; the path filter the
      workflow was carrying moved to `ci.yml`'s `changes` job, which is
      also where a Windows row will go.

## 5. Ship it

- [x] 5.1 `release.yml`: `aarch64-apple-darwin` and `x86_64-apple-darwin`,
      both on one Apple Silicon runner.
- [x] 5.2 `install.sh`: accept Darwin, map `arm64`, resolve `sha256sum` or
      `shasum -a 256`, and name the asset `<arch>-macos`.
- [x] 5.3 `installer-test.sh` proves both macOS architectures resolve, and
      keeps a fail-closed case for an OS that genuinely has no build.
- [x] 5.4 Release notes and the installation page state the Gatekeeper
      caveat rather than leaving it to be discovered.
- [ ] 5.5 First release carrying macOS assets, installed by hand on a real
      Mac. Nothing here has run on hardware anybody owns.

## 6. Not in this change

- Windows.
- Notarization, and the Apple Developer account it needs.
- The Conformance Lab on macOS: its isolation is Docker's, and macOS runners
  have none.
