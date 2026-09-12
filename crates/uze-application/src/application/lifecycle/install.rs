//! Lifecycle — install — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use std::collections::BTreeSet;

use uze_core::{
    PackageSource, Result, UzeError,
    naming::{
        NameCollisionAuthority, NameCollisionRequest, NameCollisionResolution,
        NoNameCollisionAuthority,
    },
    trust::{self, TrustAuthority},
};

use crate::bootstrap;

use super::super::services::Plugins;
use super::super::*;
use super::attach::NativeDelivery;

impl Plugins<'_> {
    pub(crate) fn acquire(&self, source: &PackageSource) -> Result<uze_core::MaterializedPackage> {
        match source {
            PackageSource::Embedded { id } => bootstrap::materialize(id),
            _ => uze_core::acquisition::acquire(source),
        }
    }

    #[tracing::instrument(name = "plugins.add", skip_all, err)]
    pub fn add(
        &self,
        source: PackageSource,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        self.add_resolving(source, authority, &NoNameCollisionAuthority)
    }

    /// `add_plugin`, with an explicit answer for what to do if the
    /// package's bare plugin name is already active under a different
    /// marketplace (ADR-038) — the CLI/TUI's interactive `--alias`/
    /// `--replace` entry point. Plain `add_plugin` refuses without asking.
    #[tracing::instrument(name = "plugins.add_resolving", skip_all, err)]
    pub fn add_resolving(
        &self,
        source: PackageSource,
        authority: &dyn TrustAuthority,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // An embedded snapshot is always the official marketplace, never
        // `local` — `install_from_marketplace` takes this same path and
        // relies on it for the official-plugin protection/removal rules to
        // recognize the result.
        let marketplace = match &source {
            PackageSource::Embedded { .. } => "uze-official",
            _ => "local",
        };
        // Acquisition brings the bytes to a local directory and owns their
        // cleanup; the Store only ever sees a materialized package.
        let materialized = self.acquire(&source)?;
        self.install_materialized_from_marketplace(
            materialized,
            marketplace,
            authority,
            &[],
            false,
            name_authority,
        )
    }

    pub(crate) fn install_materialized_from_marketplace(
        &self,
        materialized: uze_core::MaterializedPackage,
        marketplace: &str,
        authority: &dyn TrustAuthority,
        already_trusted: &[trust::ExecutableCapability],
        replacing_installed: bool,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        self.install_materialized_from_marketplace_as(
            materialized,
            marketplace,
            None,
            authority,
            already_trusted,
            replacing_installed,
            name_authority,
        )
    }

    /// `install_materialized_from_marketplace`, requesting an explicit local
    /// active name instead of the package's own bare plugin name (ADR-038)
    /// — used only by `update_plugin`, to restore an `alias` a past
    /// collision resolution gave this exact package across its
    /// remove-then-reinstall cycle. `None` behaves identically to the
    /// wrapper above.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn install_materialized_from_marketplace_as(
        &self,
        materialized: uze_core::MaterializedPackage,
        marketplace: &str,
        requested_active_name: Option<&str>,
        authority: &dyn TrustAuthority,
        already_trusted: &[trust::ExecutableCapability],
        replacing_installed: bool,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        // Any installation changes vendor-visible state; cached inspection
        // verdicts must not outlive it (ADR 018).
        self.0.inspection_cache.invalidate();
        // Deliberately does NOT run `reconcile_orphaned_receipts` here.
        // Attach's own conflict detection needs the first look at whatever
        // occupies a shared projection slot: a receipt that is Matched but
        // keyed by an id nothing installs under any more is exactly as
        // consistent with "the previous generation's incompatible wrapper,
        // a real conflict a person must see" as it is with "a plain
        // rename/removal, safe to clean" — the two are indistinguishable
        // from here, and only the second is safe to resolve without a
        // person looking. `uze doctor` (`maintain_environment`) is the
        // explicit, narrower place that reconciliation belongs; a blocked
        // install's `ProjectionConflict` is the correct, honest outcome
        // when the ambiguity can't be resolved silently.
        // Trust is decided here — after the package is materialized and can
        // be inspected honestly, and strictly before anything is written to
        // the Store or shown to a harness. Neither the Store nor any
        // integration knows this question exists.
        self.0.authorize(
            &materialized,
            authority,
            already_trusted,
            replacing_installed,
        )?;

        // `uze add` is deliberately enough for a harness the user already
        // has.  Preparing a detected integration only creates UZE's own
        // prerequisites (such as a user-scope discovery directory) and
        // records its setup state; it never installs, upgrades, or launches
        // the vendor executable.  Do it before ingesting so a preparation
        // failure cannot leave a newly installed package with no reported
        // delivery attempt.
        self.0.prepare_detected_integrations(None)?;

        let installed = self.ingest_resolving_name_collision(
            &materialized,
            marketplace,
            requested_active_name,
            name_authority,
        )?;

        // Derived views refresh before attachment: a native package delivery
        // reads the view it was just given. A failure here is recorded, never
        // propagated — the package is installed, and one integration's view
        // being stale does not make the installation invalid.
        let publications = self.0.republish_all();
        let unpublished: BTreeSet<&str> = publications
            .iter()
            .filter(|outcome| outcome.error.is_some())
            .map(|outcome| outcome.integration.as_str())
            .collect();

        let environment = self
            .0
            .engine()
            .compose(std::slice::from_ref(&installed.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let mut attachments = Vec::new();
        let mut package_plans = Vec::new();
        for integration in &self.0.integrations {
            // A package must remain installable on a machine that has only a
            // subset of UZE's peer harnesses. `add` prepares and attaches to
            // detected harnesses; an absent executable is neither a package
            // incompatibility nor a reason to invoke its vendor CLI.
            if !self.0.detect_cached(integration.as_ref()).present {
                continue;
            }
            // Native delivery reads the view; attempting it against a view
            // that failed to publish would fail for a reason that has
            // nothing to do with this package.
            let native_delivery = if unpublished.contains(integration.id()) {
                NativeDelivery::Skipped
            } else {
                NativeDelivery::Allowed
            };
            let delivery = self.0.deliver_package_to(
                &installed,
                &resources,
                integration.as_ref(),
                native_delivery,
            )?;
            if let Some(plan) = delivery.plan {
                package_plans.push((integration.id().to_owned(), plan));
            }
            attachments.extend(delivery.attachments);
        }
        Ok(AddPluginReport {
            plugin: self.0.plugin_summary(&installed)?,
            package_plans,
            attachments,
            publications,
        })
    }

    /// Ingests `materialized` under `requested_active_name` (or its own bare
    /// plugin name, when `None` — every ordinary install), asking
    /// `name_authority` to resolve a collision with an already-active,
    /// differently-marketplaced package instead of failing outright
    /// (ADR-038). `Alias` retries the ingest under the chosen local name.
    /// `Replace` removes the existing active package first — only once that
    /// is proven `Safe`, exactly the rule `remove_plugin` enforces, so a
    /// `Blocked` removal aborts the whole replace with the existing package
    /// left exactly as it was — then retries the ingest under the name it
    /// just freed. Any other ingest error (an unrelated `PackageConflict`, a
    /// bad manifest) is never routed through the authority at all.
    fn ingest_resolving_name_collision(
        &self,
        materialized: &uze_core::MaterializedPackage,
        marketplace: &str,
        requested_active_name: Option<&str>,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<uze_core::StoredPackage> {
        let (name, existing, requested) = match self.0.store.ingest_with_active_name(
            materialized,
            marketplace,
            requested_active_name,
        ) {
            Ok(installed) => return Ok(installed),
            Err(UzeError::PluginNameCollision {
                name,
                existing,
                requested,
            }) => (name, existing, requested),
            Err(other) => return Err(other),
        };
        let request = NameCollisionRequest {
            name: name.clone(),
            existing: existing.clone(),
            requested: requested.clone(),
        };
        match name_authority.resolve(&request) {
            NameCollisionResolution::Abort => Err(UzeError::PluginNameCollision {
                name,
                existing,
                requested,
            }),
            NameCollisionResolution::Alias(alias) => {
                self.0
                    .store
                    .ingest_with_active_name(materialized, marketplace, Some(&alias))
            }
            NameCollisionResolution::Replace => {
                match self.detach_and_remove(&existing, false)? {
                    RemovePluginReport::Removed { .. }
                    | RemovePluginReport::AlreadyAbsent { .. } => self
                        .0
                        .store
                        .ingest_with_active_name(materialized, marketplace, requested_active_name),
                    RemovePluginReport::Blocked { .. } => Err(UzeError::PluginNameCollision {
                        name,
                        existing,
                        requested,
                    }),
                }
            }
        }
    }
}
