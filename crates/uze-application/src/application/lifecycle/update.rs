//! Lifecycle — update — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::fs;

use uze_core::{
    MaterializedPackage, Result,
    trust::{self, TrustAuthority},
};

use super::super::services::Plugins;
use super::super::*;

impl Plugins<'_> {
    #[tracing::instrument(name = "plugins.update", skip_all, fields(id = %id), err)]
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

        // Asked here rather than from inside the install below, which is the
        // only other place that asks it: preparing a harness needs nothing
        // the removal takes away, and a vendor configuration that refuses to
        // be prepared is a failure with no consequence at all while the
        // package is still installed. Reached after the removal it was a
        // plugin gone from the machine with nothing left to heal it.
        self.0.prepare_detected_integrations(None)?;

        // What is left can still fail with the package already removed — the
        // ingest running out of disk, a revision whose environment will not
        // compose — so the installed bytes are kept aside until the install
        // below has answered for them.
        let superseded = self.0.home.state_dir().join(SUPERSEDED_DIRECTORY);
        let _ = fs::remove_dir_all(&superseded);
        self.0.store.copy_package_to(&installed.id, &superseded)?;

        // Nothing destructive has happened yet. From here the current package
        // is removed under the same ownership rules any removal obeys.
        // Updates are allowed to replace a protected official plugin — the
        // protection is against `remove`, not `update`.
        let removal = self.detach_and_remove(id, true)?;
        if let RemovePluginReport::Blocked { report, plan } = removal {
            let _ = fs::remove_dir_all(&superseded);
            return Ok(UpdatePluginReport::Blocked { report, plan });
        }
        // Trust was already settled above against the previous capabilities,
        // so installation must not ask a second time for the same answer.
        // Re-installs under the package's own marketplace, never `local`:
        // an update is a version change, not a re-namespacing, and the
        // official-plugin protection and any project lock both key on the
        // marketplace-qualified id staying exactly what it was.
        let requested_active_name = (active_name != bare_name).then_some(active_name.as_str());
        let installing = self.install_materialized_from_marketplace_as(
            materialized,
            installed.id.marketplace(),
            requested_active_name,
            &trust::AlwaysTrust,
            &[],
            true,
            &uze_core::naming::NoNameCollisionAuthority,
        );
        let report = match installing {
            Ok(report) => report,
            Err(failure) => {
                let restored = self.reinstate(&installed, &superseded, requested_active_name);
                let _ = fs::remove_dir_all(&superseded);
                return Err(UzeError::LifecycleBlocked(match restored {
                    Ok(()) => format!(
                        "`{id}` could not be updated: {failure}\nThe installed revision was put \
                         back; nothing on this machine changed."
                    ),
                    Err(restore_failure) => format!(
                        "`{id}` could not be updated: {failure}\nPutting the installed revision \
                         back also failed: {restore_failure}\nInstall it again to restore it."
                    ),
                }));
            }
        };
        let _ = fs::remove_dir_all(&superseded);
        Ok(UpdatePluginReport::Updated {
            plugin: report.plugin,
            attachments: report.attachments,
            publications: report.publications,
        })
    }

    /// Puts the revision an update removed back exactly as it was — its
    /// bytes, its registration under the same marketplace-qualified id and
    /// local name, and its attachments.
    ///
    /// Installing is what restoring is: the removal detached every harness
    /// artifact, so re-registering the bytes alone would leave the plugin
    /// listed and reaching nothing.
    fn reinstate(
        &self,
        installed: &uze_core::StoredPackage,
        superseded: &Path,
        requested_active_name: Option<&str>,
    ) -> Result<()> {
        let recovered =
            MaterializedPackage::borrowed(superseded.to_path_buf(), installed.provenance.clone());
        self.install_materialized_from_marketplace_as(
            recovered,
            installed.id.marketplace(),
            requested_active_name,
            &trust::AlwaysTrust,
            &[],
            true,
            &uze_core::naming::NoNameCollisionAuthority,
        )
        .map(|_| ())
    }
}

/// Where an update keeps the revision it is replacing, under UZE's own
/// state rather than beside the plugins: nothing that reads the Store may
/// mistake it for an installed package.
const SUPERSEDED_DIRECTORY: &str = "superseded";

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
    #[tracing::instrument(name = "plugins.auto_update", skip_all)]
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
