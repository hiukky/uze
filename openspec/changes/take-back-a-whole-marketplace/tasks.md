## 1. The teardown

- [x] 1.1 `Marketplace::remove` composes the per-package machine removal: every package the Store holds from the marketplace (by provenance) goes through `detach_and_remove` — inspect-before-detach, receipt-owned teardown, drift safety — with one `MutationLock` held for the whole sweep (`detach_and_remove` is the lock-free inner path)
- [x] 1.2 The registry entry and link go last, only when every package came off; a blocked package keeps the marketplace registered and is named in the answer
- [x] 1.3 Read model `MarketplaceRemovalReport` (removed, blocked with reasons, record_removed); the CLI renders it and exits non-zero on an incomplete teardown, telling the caller to run `market remove` again
- [x] 1.3a The detachment walks the receipts ledger (already the case in `detach_owned_receipts`) — an undetected harness's delivered bytes are torn down through its integration

## 2. Tests

- [x] 2.1 `removing_a_marketplace_takes_its_packages_with_it` — purge + record, Store empty afterwards
- [x] 2.2 `a_blocked_package_keeps_the_marketplace_registered` — a drifted attachment blocks its package, the marketplace stays registered naming the block, the removed packages stay off
- [x] 2.3 `removing_a_marketplace_that_installs_nothing_removes_in_one_step` — the record removal alone
- [x] 2.4 `cargo test --workspace`, clippy, fmt green
