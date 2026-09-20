# uze-git

The one transport for speaking to the Git binary: how the process is spawned,
what environment it inherits, and Git's exit code reported rather than
classified. Carries no domain.

- `read` — a command whose output you want
- `write` — a command that mutates the repository

A non-zero exit is an answer for `diff`, `rebase` and `rev-parse --verify`,
and a failure elsewhere; only the caller knows which. Never spawn `git`
directly anywhere else — the architecture suite fails a third spawn.

```bash
cargo test -p uze-git
```
