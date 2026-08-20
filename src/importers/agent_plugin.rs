use std::path::Path;

use crate::{bundle::ImportedBundle, error::Result};

use super::{ForeignImporter, import_from_manifest};

/// Agent Plugins 1.0 is the initial package input accepted by the UZE store.
pub struct AgentPluginImporter;

impl ForeignImporter for AgentPluginImporter {
    fn import(&self, root: &Path) -> Result<Option<ImportedBundle>> {
        let manifest = root.join("plugin.json");
        manifest
            .is_file()
            .then(|| import_from_manifest(root, manifest, "agent-plugin"))
            .transpose()
    }
}
