//! Workspace tests: the project↔machine boundary — `agents.lock` consumer
//! semantics, marketplace resolution (incl. malformed inputs) and
//! project-root resolution.

pub(crate) mod consumer;
pub(crate) mod marketplace;

/// Shared helpers for the workspace test binary.
pub(crate) mod util {
    pub(crate) fn uze_bin() -> &'static std::path::Path {
        std::path::Path::new(env!("CARGO_BIN_EXE_uze"))
    }
}
