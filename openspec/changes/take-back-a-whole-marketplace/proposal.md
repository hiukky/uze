## Why

`market remove` refuses while any of the marketplace's packages is installed, so taking a marketplace off the machine is a manual journey: list the packages, remove each with the machine verb, then remove the record — and a harness the removal context could not detect keeps its delivered bytes, because the detach followed detection instead of the receipts ledger. Tested end to end removing the test marketplace `boo`: the vendor-side copies survived a successful removal.

## What Changes

- **`market remove <name>` is the teardown**: every package from that marketplace is removed first, each through the same machine-removal path a per-package `remove` runs (inspect-before-detach, receipts, drift safety); the registry entry goes last, only when none remains. A blocked package keeps the marketplace registered and is named in the answer.
- **Detach follows the receipt ledger, not detection**: the removal undoes every receipt the ledger owns for the package, whether or not the vendor binary is detected in the current context.
- A project that still declares one of the packages keeps working as today: the machine removal is explicit, the project's lock re-reproduces later — the answer names what came off.

## Capabilities

### Modified Capabilities

- `marketplace`: the remove requirement is modified (see specs/marketplace/spec.md).

## Impact

- `crates/uze-application`: `Marketplace::remove` composes `Plugins::remove` per installed package (mutation lock held once — `detach_and_remove` is the lock-free inner path); read model for the purge answer.
- `src/main.rs`: render the purge answer (removed / blocked / record).
- Tests: `tests/cli/market.rs`-style integration for purge, partial-block, and empty-marketplace cases; the ledger-detach case with an undetected harness.
