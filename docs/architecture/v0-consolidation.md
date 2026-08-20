# UZE v0 consolidation classification

This is a codebase classification, not a new architectural decision. ADR-008
and ADR-009 remain the current invariants.

| Surface | Classification | Current reason |
|---|---|---|
| `application.rs`, Store, Engine, Router, receipts, reconciliation | PRODUCT | Implements package-centric install, inspect, doctor, and safe removal. |
| `tui.rs` | PRODUCT | Thin Ratatui view/controller over `UzeApplication`; it owns no lifecycle or harness logic. |
| `integrations/{claude,codex,opencode}.rs` | PRODUCT | Peer harness delivery authority. |
| `report.rs` / `CompatibilityReport` | REMOVED | Superseded by package-centric inspection DTOs in the application layer. |
| `bundle.rs` / Agent Plugin importer evidence | PRODUCT INTERNAL | Store uses it to validate external Agent Plugin inputs without rewriting them. |
| Claude legacy importer / generic `import_bundle` entry point | FUTURE RESERVED | Not on the current package lifecycle, but retained as isolated foreign-format import research. |
| `runtime.rs`, `RuntimeBridge`, `FilesystemProjection` | CONFORMANCE/TEST SUPPORT | Retained only for explicit project/session conformance paths, not package lifecycle. |
| `cache/` | FUTURE RESERVED | Created by `UzeHome`, but no v0 product feature depends on it. |
| `runtime/` | CONFORMANCE/TEST SUPPORT | Holds isolated session artifacts; no harness runtime proxy exists. |
| `PackageExposureMechanism::DecomposeCapabilities` | REMOVED | Decomposition is represented by the absence of a package-native plan. |

Any removal must retain the explicit conformance coverage before deleting its
support code. No feature code should depend on a classified legacy surface.
