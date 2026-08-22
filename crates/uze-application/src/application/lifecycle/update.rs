//! Lifecycle — update — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{
    Result,
    trust::{self, TrustAuthority},
};

use super::super::*;

impl UzeApplication {
    pub fn update_plugin(
        &self,
        id: &str,
        authority: &dyn TrustAuthority,
    ) -> Result<UpdatePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        let installed = self.package_by_name(id)?;

        // Re-resolve the *request*, not the resolution: that is what makes a
        // branch move forward while a pinned commit stays put.
        let materialized = self.acquire(&installed.provenance.requested)?;

        let previous = {
            let environment = self.engine().compose(std::slice::from_ref(&installed.id))?;
            let resources: Vec<&uze_core::Resource> = environment.resources.iter().collect();
            trust::executable_capabilities(&resources)
        };
        self.authorize(&materialized, authority, &previous, true)?;

        // Nothing destructive has happened yet. From here the current package
        // is removed under the same ownership rules any removal obeys.
        // Updates are allowed to replace a protected official plugin — the
        // protection is against `remove`, not `update`.
        let removal = self.detach_and_remove(id, true)?;
        if let RemovePluginReport::Blocked { report, plan } = removal {
            return Ok(UpdatePluginReport::Blocked { report, plan });
        }
        // Trust was already settled above against the previous capabilities,
        // so installation must not ask a second time for the same answer.
        let report = self.install_materialized(materialized, &trust::AlwaysTrust, &[], true)?;
        Ok(UpdatePluginReport::Updated {
            plugin: report.plugin,
            attachments: report.attachments,
            publications: report.publications,
        })
    }
}
