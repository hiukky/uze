## 1. The receipt

- [x] 1.1 `install.sh` writes `$UZE_HOME/state/install.json` (or
  `$HOME/.uze/...`) after the binary verifies: the physical path it placed and
  the version that binary reports; a failure to write is a warning, never a
  failed install.
- [x] 1.2 `tests/scripts/installer-test.sh` asserts the receipt names the file
  placed and the release it was.

## 2. The updater

- [x] 2.1 `src/self_update.rs`: SemVer precedence, the redirect-derived latest
  release, the asset name from the running binary's own target, the ledger
  (`state/update.json`) and the decision of what to say.
- [x] 2.2 Download, checksum, `--version` confirmation and the same-filesystem
  rename; the receipt follows the file.
- [x] 2.3 `UZE_AUTOUPDATE` / `CI` policy and `UZE_BASE_URL`.
- [x] 2.4 Unit tests: precedence, tag parsing, checksum lookup, the decision
  table, a pass with a fake release source (installs, refuses a foreign
  binary, notify-only, failed install, offline), and a refused replacement.

## 3. Both sidebars

- [x] 3.1 `ReleaseNotice` in `src/ui.rs`: two rows — the version with its
  closing mark, then what happened and what to do — drawn by both sidebars.
- [x] 3.2 Workspace: the notice on the steps (or the history), `OpenReleaseNotes` opens the
  notes off the frame thread, `DismissRelease` acknowledges; the `pump`
  follows the notice's revision.
- [x] 3.3 Management: the same, through `Hit`s and an `AcknowledgeRelease`
  intent.
- [x] 3.4 The watch starts once per process in `ui::run`.
- [x] 3.5 Render tests: the notice sits above the steps in both sidebars, whole
  at a real version's length with its mark on the version's row, its
  row and its mark are targets, and no notice draws nothing.

## 4. The CLI

- [x] 4.1 Hidden `uze self-update` runs one check, ahead of application setup;
  classified `JustifiedSlow` in `command_performance.rs`.
- [x] 4.2 After a successful command whose reader is a person, a stale answer
  is handed to a detached `uze self-update` in its own process group, and a
  release is mentioned once on stderr.

## 5. Around it

- [x] 5.1 Journey worlds set `UZE_AUTOUPDATE=off`.
- [x] 5.2 `docs/versioning.md`: what the updater does, what it will not
  replace, what happens to a running server, and how to stop it.
- [ ] 5.3 Verify by hand against a real release: an install from `install.sh`
  at the previous version replaces itself and announces it.
