//! CLI layer tests (L1/L2): the real `uze` binary against isolated
//! environments — grammar/precedence (ADR-019) and machine-scoped
//! commands. Project-scoped CLI semantics live in `workspace/` and
//! `acceptance/`.

mod grammar;
mod machine;
