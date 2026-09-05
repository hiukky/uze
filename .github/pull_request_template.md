<!-- What changed and why. The why is the part a reviewer cannot reconstruct
     from the diff, and the part that is still useful a year from now. -->

## Why

## What changed

## Gate

<!-- Delete what does not apply; state what you actually ran, not what you
     intended to. `ci.yml` is the source of truth — this is the local proxy. -->

- [ ] `make check` (fmt, clippy, cargo-deny, tests, ruff)
- [ ] `openspec validate --all --strict`
- [ ] Conformance verticals for every harness this touches (`make lab-run`)
- [ ] `CREDITS.md` regenerated (`make attributions`) if dependencies changed

<!-- Over ~400 changed lines of production code? Say what was grouped and why
     splitting would have been worse. See CONTRIBUTING.md. -->
