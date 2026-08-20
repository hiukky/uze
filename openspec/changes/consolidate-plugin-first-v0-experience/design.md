## Design

`UzeApplication` is the product façade used by the CLI and future TUI. It
owns package-centric `setup`, `add`, `list`, `inspect`, `remove`, and `doctor`
operations. It asks integrations for package plans first, attaches only
remaining capability resources, and records each successful external effect
in `$UZE_HOME/state/attachments.json`. Presentation layers do not access the
Store, integrations, vendor files, or lifecycle primitives directly.

The ledger records package identity, integration, resource identity (when
applicable), mechanism kind, location/identifier, and expected target/value.
Removal reconciles while the Store still exists, uses `plan_remove`, detaches
only matched receipts, re-reconciles, removes resolved ledger records, then
removes Store content. Doctor consumes the same reconciliation reports and
does not repair state.

No compatibility rule reads source provenance. A vendor envelope remains an
optional native package-delivery enhancement; portable-only packages use
existing capability fallbacks.
