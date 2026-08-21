## Design

The workspace has four product layers and one test-only member:

```text
uze (CLI + TUI facade)
          │
uze-application
      │         │
uze-core   uze-integrations
      │
package/store/router/receipts

e2e (independent conformance tooling)
```

`uze-core` owns standards discovery, package/resource identities, Store,
EffectiveEnvironment, capability planning, generic integration contracts,
receipt/ledger state, and reconciliation. It never imports a concrete
harness, application façade, terminal UI, Docker, or model runner.

`uze-integrations` owns Claude, Codex, OpenCode, and Gemini-specific config,
CLI calls, planning strategies, and receipt inspection. It depends on Core.

`uze-application` owns `UzeApplication`, source acquisition/trust
orchestration, package-centric product operations, and the production
composition root. It depends on Core and integrations.

The root `uze` package remains installable and source-compatible. It exports
the established Core modules and `UzeApplication`, while CLI/TUI stay there as
the product presentation layer. The existing `e2e` crate becomes an explicit
workspace member but remains outside the product dependency graph.

This is deliberately not a microcrate split: storage, planning, receipts and
the integration contract remain together because they form one stable core.
The future provisioner belongs at the Application → integration boundary.

### Architectural decision

This establishes a durable compiler-enforced dependency direction, so ADR-011
records the crate boundaries and compatibility facade.
