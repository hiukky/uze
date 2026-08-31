//! Built-in extension registry — the TUI-side mirror of
//! `uze-integrations::registry::IntegrationRegistry`: the single
//! composition root that knows which extensions exist, in registration
//! order. Everything that lists extensions (the management TUI's
//! Extensions screen) consumes the registry or the catalog entry it
//! exposes, never a hand-maintained list of its own. Adding an extension
//! means one module under `crates/uze-extensions/src/` and one entry in
//! `builtin` — nothing else in the product needs to know it exists.

use crate::git_diff;

/// Catalog metadata for one built-in extension — what the Extensions
/// screen renders. Built-ins are compiled into the binary (no loading or
/// enablement yet), so this is also the shape a future user-installable
/// extension would populate from its manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinExtension {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Which uze surface the extension extends (`Workspace TUI`, …).
    pub surface: &'static str,
    /// One-line how-to-open hint shown in the detail drawer.
    pub usage: &'static str,
}

/// The built-in extension set, in registration order.
pub struct ExtensionRegistry {
    extensions: Vec<BuiltinExtension>,
}

impl ExtensionRegistry {
    /// Composition root: the one place that names the concrete extension
    /// modules. Unlike the integration registry there is deliberately no
    /// `isolated` variant — extensions are pure compiled-in code that
    /// never touches the machine, so there is no environment to root
    /// under.
    pub fn builtin() -> Self {
        Self {
            extensions: vec![git_diff::CATALOG],
        }
    }

    pub fn all(&self) -> &[BuiltinExtension] {
        &self.extensions
    }

    pub fn get(&self, id: &str) -> Option<&BuiltinExtension> {
        self.extensions.iter().find(|extension| extension.id == id)
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.extensions
            .iter()
            .map(|extension| extension.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::ExtensionRegistry;

    #[test]
    fn builtin_exposes_the_whole_extension_set_in_order() {
        let registry = ExtensionRegistry::builtin();
        let ids = registry.ids();
        assert!(!ids.is_empty(), "at least the git-changes extension ships");
        assert_eq!(ids.len(), registry.all().len());
        assert!(ids.iter().all(|id| !id.is_empty()));
        assert_eq!(registry.get(ids[0]).map(|e| e.id), Some(ids[0]));
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn catalog_entries_are_well_formed() {
        for extension in ExtensionRegistry::builtin().all() {
            assert!(!extension.name.is_empty());
            assert!(!extension.description.is_empty());
            assert!(!extension.surface.is_empty());
            assert!(!extension.usage.is_empty());
        }
    }
}
