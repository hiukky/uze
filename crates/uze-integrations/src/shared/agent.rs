//! Markdown agent delivery, which three harnesses share: the canonical
//! definition is linked, unchanged, into the harness's own agents directory.

use std::path::Path;

use uze_core::{
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::ManagedArtifact,
    router::CompatibilityRoute,
};

/// The agent's own name: its logical capability name, else the resource's.
pub(crate) fn agent_name(resource: &Resource) -> String {
    resource
        .logical_capability_name()
        .unwrap_or_else(|| resource.name())
}

/// A receipt-owned link from `<agents_dir>/<entry_name>.md` to the Store's
/// canonical definition.
pub(crate) fn markdown_agent_plan(
    agents_dir: &Path,
    entry_name: &str,
    resource: &Resource,
    evidence: &str,
) -> ExposurePlan {
    ExposurePlan {
        route: CompatibilityRoute::Native,
        mechanism: ExposureMechanism::Managed(ManagedArtifact::SymlinkReference {
            path: agents_dir.join(format!("{entry_name}.md")),
            target: resource.capability.path.clone(),
        }),
        evidence: evidence.to_owned(),
    }
}
