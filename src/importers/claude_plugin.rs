use std::path::Path;

use crate::{bundle::ImportedBundle, error::Result};

use super::{ForeignImporter, import_from_manifest};

/// Foreign Claude Plugin source importer. It is deliberately unrelated to
/// ClaudeIntegration, which is a peer runtime adapter.
pub struct ClaudePluginImporter;

impl ForeignImporter for ClaudePluginImporter {
    fn import(&self, root: &Path) -> Result<Option<ImportedBundle>> {
        let manifest = root.join(".claude-plugin/plugin.json");
        manifest
            .is_file()
            .then(|| import_from_manifest(root, manifest, "claude-plugin"))
            .transpose()
    }
}
