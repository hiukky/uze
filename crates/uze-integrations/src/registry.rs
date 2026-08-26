//! Built-in integration registry — the single production composition root.
//!
//! Everything about which harnesses exist and how they are constructed lives
//! here. `IntegrationRegistry::builtin` is the one production call site that
//! names a concrete integration type; application, CLI, the runtime shim,
//! and tooling all consume the registry or the `IntegrationPort` contract
//! instead. Adding a harness means adding one vertical under
//! `crates/uze-integrations/src/` and one entry in `builtin` (plus
//! conformance and docs) — nothing else in the product needs to know it
//! exists.

use std::path::Path;

use uze_core::{Result, home::UzeHome, integration::IntegrationPort};

use crate::{antigravity, claude, codex, opencode};

/// The built-in integration set, in registration order.
pub struct IntegrationRegistry {
    integrations: Vec<Box<dyn IntegrationPort>>,
}

impl IntegrationRegistry {
    /// Composition root: constructs every built-in integration from the
    /// environment. The one place that knows the harness set.
    pub fn builtin(home: &UzeHome) -> Result<Self> {
        Ok(Self {
            integrations: vec![
                Box::new(claude::ClaudeIntegration::from_env(home.clone())?),
                Box::new(codex::CodexIntegration::from_env(home.clone())?),
                Box::new(opencode::OpenCodeIntegration::from_env(home.clone())?),
                Box::new(antigravity::AntigravityIntegration::from_env(home.clone())?),
            ],
        })
    }

    /// Isolated composition for tooling and tests that must not touch the
    /// real machine: every harness home is rooted under `root` instead of
    /// `$HOME`, mirroring `builtin`'s environment construction exactly.
    pub fn isolated(root: &Path, home: &UzeHome) -> Self {
        Self {
            integrations: vec![
                Box::new(claude::ClaudeIntegration::new(
                    root.join("claude"),
                    home.clone(),
                )),
                Box::new(codex::CodexIntegration::new(
                    root.join("agents"),
                    home.clone(),
                )),
                Box::new(opencode::OpenCodeIntegration::new(
                    root.join("agents"),
                    root.join("opencode-config/opencode.json"),
                    home.clone(),
                )),
                Box::new(antigravity::AntigravityIntegration::new(
                    root.join("agents"),
                    home.clone(),
                )),
            ],
        }
    }

    /// Consumes the registry into its raw integration list for embedding
    /// (`UzeApplication::new`, tests, callers that own their own composition).
    pub fn into_inner(self) -> Vec<Box<dyn IntegrationPort>> {
        self.integrations
    }

    pub fn all(&self) -> &[Box<dyn IntegrationPort>] {
        &self.integrations
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn IntegrationPort> {
        self.integrations.iter().map(Box::as_ref)
    }

    pub fn get(&self, id: &str) -> Option<&dyn IntegrationPort> {
        self.iter().find(|integration| integration.id() == id)
    }

    /// Resolves a requested harness name against the registered set: an id
    /// or any alias the integration itself declares.
    pub fn resolve(&self, requested: &str) -> Option<&dyn IntegrationPort> {
        self.iter().find(|integration| {
            integration.id() == requested || integration.aliases().contains(&requested)
        })
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.iter().map(|integration| integration.id()).collect()
    }

    /// The shim symlink names this registry's runtime-integration opt-ins
    /// are created under (`ensure_runtime_shim`) — the argv[0] names the
    /// PATH shim dispatches on.
    pub fn shim_names(&self) -> Vec<&'static str> {
        self.iter()
            .filter(|integration| integration.supports_runtime_integration())
            .map(|integration| integration.shim_name())
            .collect()
    }

    /// The registered integration whose shim symlink uses `name`, if any.
    pub fn by_shim_name(&self, name: &str) -> Option<&dyn IntegrationPort> {
        self.iter().find(|integration| {
            integration.supports_runtime_integration() && integration.shim_name() == name
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use uze_core::home::UzeHome;

    use super::IntegrationRegistry;

    fn registry() -> (UzeHome, IntegrationRegistry) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("uze-registry-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let home = UzeHome::at(root.join("uze"));
        let registry = IntegrationRegistry::isolated(&root, &home);
        (home, registry)
    }

    #[test]
    fn builtin_and_isolated_expose_the_same_integration_set() {
        let (_home, registry) = registry();
        let ids = registry.ids();
        assert_eq!(ids.len(), 4, "registration order is the built-in order");
        assert!(ids.iter().all(|id| !id.is_empty()));
        assert_eq!(registry.get(ids[0]).map(|i| i.id()), Some(ids[0]));
    }

    #[test]
    fn resolve_matches_id_or_declared_alias() {
        let (_home, registry) = registry();
        let by_id = registry.resolve("claude-code").expect("id resolves");
        let by_alias = registry.resolve("claude").expect("alias resolves");
        assert_eq!(by_id.id(), by_alias.id());
        assert!(registry.resolve("nope").is_none());
    }

    #[test]
    fn shim_names_cover_exactly_the_runtime_integration_opt_ins() {
        let (_home, registry) = registry();
        // The opt-in set, not a hand-maintained list: Claude and OpenCode
        // declare `supports_runtime_integration`; Codex and Antigravity do
        // not, so no shim symlink is ever created for them.
        let shim_names = registry.shim_names();
        assert_eq!(shim_names, vec!["claude", "opencode"]);
        for name in &shim_names {
            let integration = registry.by_shim_name(name).expect("shim name resolves");
            assert!(integration.supports_runtime_integration());
            assert_eq!(integration.shim_name(), *name);
        }
        assert!(registry.by_shim_name("agy").is_none());
    }
}
