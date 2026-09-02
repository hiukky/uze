//! Lifecycle — update — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{
    Result,
    trust::{self, TrustAuthority},
};

use super::super::services::Plugins;
use super::super::*;

impl Plugins<'_> {
    pub fn update(&self, id: &str, authority: &dyn TrustAuthority) -> Result<UpdatePluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let installed = self.0.package_by_name(id)?;
        // An update is a version change, never a re-namespacing (ADR-038):
        // whatever local name this package currently answers to — its own
        // bare name, or an `alias` a past collision resolution gave it —
        // must come back exactly the same after the reinstall below removes
        // and recreates its registration.
        let active_name = installed.active_name.clone();
        let bare_name = installed.id.plugin_name().to_owned();

        // Re-resolve the *request*, not the resolution: that is what makes a
        // branch move forward while a pinned commit stays put.
        let materialized = self.acquire(&installed.provenance.requested)?;

        let previous = {
            let environment = self
                .0
                .engine()
                .compose(std::slice::from_ref(&installed.id))?;
            let resources: Vec<&uze_core::Resource> = environment.resources.iter().collect();
            trust::executable_capabilities(&resources)
        };
        self.0
            .authorize(&materialized, authority, &previous, true)?;

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
        // Re-installs under the package's own marketplace, never `local`:
        // an update is a version change, not a re-namespacing, and the
        // official-plugin protection and any project lock both key on the
        // marketplace-qualified id staying exactly what it was.
        let requested_active_name = (active_name != bare_name).then_some(active_name.as_str());
        let report = self.install_materialized_from_marketplace_as(
            materialized,
            installed.id.marketplace(),
            requested_active_name,
            &trust::AlwaysTrust,
            &[],
            true,
            &uze_core::naming::NoNameCollisionAuthority,
        )?;
        Ok(UpdatePluginReport::Updated {
            plugin: report.plugin,
            attachments: report.attachments,
            publications: report.publications,
        })
    }
}

impl Plugins<'_> {
    /// Applies every pending update this machine can settle on its own,
    /// and reports what it did.
    ///
    /// "On its own" is two deliberate restrictions, not an optimization:
    ///
    /// - **Only an update uze can already see.** `update_available` is a
    ///   local comparison against the embedded official snapshot — bytes
    ///   that shipped inside the binary already being run. Nothing here
    ///   reaches the network, so a Git- or path-sourced plugin is never
    ///   re-resolved behind the operator's back; those still update only
    ///   through an explicit `update_plugin`.
    /// - **Only under `NoTrustAuthority`.** A revision that introduces new
    ///   executable capability is refused and reported, exactly as a
    ///   non-interactive bootstrap refuses one (see
    ///   `docs/architecture/invariants.md`, "A default plugin crossing the
    ///   trust boundary is never installed silently"). The operator is then
    ///   still offered the update explicitly, with the dialog.
    ///
    /// Best-effort per plugin: one failure never stops the rest, and a
    /// blocked or refused update leaves the installed revision untouched —
    /// `update_plugin` already inspects before it detaches.
    ///
    /// This is not called from the CLI dispatch path: `ensure_default_plugins`
    /// runs before every command, read-only ones included, and a diagnostic
    /// must not rewrite plugin content. Interactive surfaces call this.
    pub fn auto_update(&self) -> Vec<AutoUpdateOutcome> {
        let pending: Vec<String> = self
            .0
            .installed_packages()
            .into_iter()
            .filter(|package| {
                matches!(
                    package.provenance.requested,
                    uze_core::PackageSource::Embedded { .. }
                )
            })
            .filter(|package| {
                self.0
                    .plugin_summary(package)
                    .ok()
                    .and_then(|summary| summary.update_available)
                    == Some(true)
            })
            .map(|package| package.id.as_str().to_owned())
            .collect();

        pending
            .into_iter()
            .map(|plugin| {
                let detail = match self.update(&plugin, &trust::NoTrustAuthority) {
                    Ok(UpdatePluginReport::Updated { .. }) => None,
                    Ok(UpdatePluginReport::Blocked { .. }) => {
                        Some("managed state was preserved; update it explicitly".to_owned())
                    }
                    Err(UzeError::TrustRequired { detail, .. }) => Some(format!(
                        "the new revision asks to execute something new ({detail}); \
                         confirm it explicitly"
                    )),
                    Err(error) => Some(error.to_string()),
                };
                AutoUpdateOutcome {
                    plugin,
                    applied: detail.is_none(),
                    detail,
                }
            })
            .collect()
    }
}
