//! Replacing an installed plugin with what its original request resolves
//! to now, and applying the updates this machine can settle on its own.

use std::fs;

use uze_core::{
    MaterializedPackage, Result,
    trust::{self, TrustAuthority},
};

use super::super::services::Plugins;
use super::super::*;

impl Plugins<'_> {
    /// The package re-read from the checkout its marketplace is linked to,
    /// or `None` when it is not linked.
    ///
    /// Best-effort by design: a link pointing at a checkout that has since
    /// been moved or broken must not make the package un-updatable, so a
    /// failure here falls back to the source the package was installed
    /// from and the ordinary error surfaces from there.
    fn linked_source(
        &self,
        installed: &uze_core::StoredPackage,
    ) -> Option<uze_core::MaterializedPackage> {
        let marketplace = installed.id.marketplace();
        let record = uze_core::state::marketplace_get(&self.0.home, marketplace).ok()??;
        record.link?;
        let request = super::super::marketplace::MarketplaceRequest::of(&record.source).ok()?;
        request
            .materialize_plugin(
                installed.id.plugin_name(),
                super::super::marketplace::MirrorAt {
                    home: &self.0.home,
                    marketplace,
                    recent: None,
                    fetched: &self.0.mirrors_fetched,
                },
            )
            .ok()
    }

    #[tracing::instrument(name = "plugins.update", skip_all, fields(id = %id), err)]
    pub fn update(&self, id: &str, authority: &dyn TrustAuthority) -> Result<UpdatePluginReport> {
        self.0.begin_operation();
        // Reading which package this is, and fetching its new bytes, happen
        // *outside* the mutation lock. That lock is global and exclusive,
        // and acquisition reaches a remote: held across it, one background
        // update would refuse the operator's own command in another
        // terminal — and every mutating action inside the client itself,
        // reporting the client's own pid back at them. Nothing is written
        // until `deliver` below, which is where the lock belongs.
        let installed = self.0.package_by_name(id)?;

        // Re-resolve the *request*, not the resolution: that is what makes a
        // branch move forward while a pinned commit stays put.
        //
        // Unless the marketplace is linked to a checkout on this machine,
        // in which case the request is not where the bytes are any more.
        // Asked here rather than by the caller because a link is a machine
        // fact, and this is the machine-level way to bring a package up to
        // date — so `uze plugin update` and a project's own update follow
        // it alike.
        let materialized = match self.linked_source(&installed) {
            Some(request) => request,
            None => self.acquire(&installed.provenance.requested)?,
        };
        self.replace_with(id, materialized, authority)
    }

    /// Replaces `id`'s installed revision with bytes the caller already
    /// materialized, and puts the old one back if the new one cannot be
    /// delivered.
    ///
    /// Separate from `update` because *where the new bytes come from* is
    /// the caller's question and the replacement is not. A machine update
    /// re-resolves the package's own request; a project's update resolves
    /// the ref its manifest declares, which is a different revision
    /// whenever the request was itself pinned — a package reproduced from
    /// `agents.lock` has the locked commit *as* its request, so
    /// re-resolving that can only ever return what is already installed.
    pub(crate) fn replace_with(
        &self,
        id: &str,
        materialized: uze_core::MaterializedPackage,
        authority: &dyn TrustAuthority,
    ) -> Result<UpdatePluginReport> {
        let installed = self.0.package_by_name(id)?;
        // An update is a version change, never a re-namespacing (ADR-038):
        // whatever local name this package currently answers to — its own
        // bare name, or an `alias` a past collision resolution gave it —
        // must come back exactly the same after the reinstall below removes
        // and recreates its registration.
        let active_name = installed.active_name.clone();
        let bare_name = installed.id.plugin_name().to_owned();

        let previous = {
            let resources = uze_core::engine::package_resources(&installed)?;
            let resources: Vec<&uze_core::Resource> = resources.iter().collect();
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
        self.0.prepare_detected_integrations()?;

        // From here on the machine changes, so this is where the lock
        // belongs: everything above only read, and the bytes it fetched are
        // in scratch nobody else can see.
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // The package may have moved under us while the network was busy.
        // Re-read it rather than acting on what was true before the fetch.
        let installed = self.0.package_by_name(id)?;

        // What is left can still fail with the package already removed — the
        // ingest running out of disk, a revision whose environment will not
        // compose — so the installed bytes are kept aside until the install
        // below has answered for them.
        let superseded = self.0.home.superseded_dir();
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
        let installing = self.install_authorized(
            materialized,
            installed.id.marketplace(),
            requested_active_name,
            &uze_core::naming::NoNameCollisionAuthority,
        );
        let report = match installing {
            Ok(report) => report,
            Err(failure) => {
                let restored = self.reinstate(&installed, &superseded, requested_active_name);
                let message = match restored {
                    Ok(()) => {
                        let _ = fs::remove_dir_all(&superseded);
                        format!(
                            "`{id}` could not be updated: {failure}\nThe installed revision was \
                             put back; nothing on this machine changed."
                        )
                    }
                    // The copy kept aside is now the only one of this
                    // revision on the machine, so it stays: swept with the
                    // rest it would leave a message telling the operator to
                    // install something nothing on the machine still has.
                    // The directory is named too, because a reinstall that
                    // found bytes already there is what the failure most
                    // likely was, and it refuses again until they are gone.
                    Err(restore_failure) => format!(
                        "`{id}` could not be updated: {failure}\nPutting the installed revision \
                         back also failed: {restore_failure}\nIts bytes are kept at {superseded}. \
                         Remove {plugin_dir} if it is still there, then `uze plugin install \
                         {qualified}` to restore it.",
                        qualified = installed.id.as_str(),
                        superseded = superseded.display(),
                        plugin_dir = self.0.home.plugin_dir(&installed.id).display(),
                    ),
                };
                return Err(UzeError::LifecycleBlocked(message));
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
    ///
    /// Which makes it only as recoverable as an install: what it recovers
    /// from is most often an install that failed part-way and left the
    /// plugin's directory behind, which the Store clears — no registration
    /// claims that id — before writing the bytes back into it. Where even
    /// that fails, the restore fails too, and the caller keeps the
    /// superseded copy and says where it is rather than telling the
    /// operator to install a revision the machine no longer has.
    fn reinstate(
        &self,
        installed: &uze_core::StoredPackage,
        superseded: &Path,
        requested_active_name: Option<&str>,
    ) -> Result<()> {
        let recovered =
            MaterializedPackage::borrowed(superseded.to_path_buf(), installed.provenance.clone());
        self.install_authorized(
            recovered,
            installed.id.marketplace(),
            requested_active_name,
            &uze_core::naming::NoNameCollisionAuthority,
        )
        .map(|_| ())
    }
}

impl Plugins<'_> {
    /// Applies every pending update this machine can settle on its own,
    /// and reports what it did.
    ///
    /// "On its own" is one restriction, and it is a trust one, not a
    /// transport one: **only under `NoTrustAuthority`**. A revision that
    /// introduces executable capability the installed one did not have is
    /// refused and reported, exactly as a non-interactive bootstrap refuses
    /// one (see `docs/architecture/invariants.md`, "A default plugin
    /// crossing the trust boundary is never installed silently"). The
    /// operator is then still offered it explicitly, with the dialog.
    ///
    /// It used to be a transport restriction too — only updates this
    /// machine could already see, so a Git- or path-sourced plugin was
    /// never re-resolved. That reasoning still holds where it was written:
    /// `ensure_default_plugins` runs before *every* command, read-only ones
    /// included, and a diagnostic must not reach a remote or rewrite plugin
    /// content. It does not hold for the client opening, which is an
    /// explicit interactive act — the same argument `spawn_startup` already
    /// makes for the work it does there. So the rule is now about *who
    /// calls this*, and the CLI dispatch path still never does.
    ///
    /// Best-effort per plugin: one failure never stops the rest, and a
    /// blocked or refused update leaves the installed revision untouched —
    /// `Plugins::update` inspects before it detaches.
    ///
    /// Not called from the CLI dispatch path. Interactive surfaces call it.
    #[tracing::instrument(name = "plugins.auto_update", skip_all)]
    pub fn auto_update(&self) -> Vec<AutoUpdateOutcome> {
        // Only what is already known to be behind. Freshness is a local
        // read — the installed commit against the head its mirror last
        // recorded — so deciding *whether* to update costs no network at
        // all, and only a package that needs one is fetched.
        let pending: Vec<String> = self
            .0
            .installed_packages()
            .into_iter()
            .filter(|package| self.0.freshness_of(package).behind())
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
