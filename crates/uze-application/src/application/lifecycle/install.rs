//! Installing a plugin: its bytes, the trust question they raise, the Store
//! entry and the delivery to every detected harness.

use std::collections::BTreeSet;

use uze_core::{
    PackageSource, Result, UzeError,
    naming::{
        NameCollisionAuthority, NameCollisionRequest, NameCollisionResolution,
        NoNameCollisionAuthority,
    },
    trust::TrustAuthority,
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

    /// Installs a package straight from a source, under the `local`
    /// marketplace. Refuses, without asking, a bare plugin name already
    /// active under another marketplace (ADR-038).
    #[tracing::instrument(name = "plugins.add", skip_all, err)]
    pub fn add(
        &self,
        source: PackageSource,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // Acquisition brings the bytes to a local directory and owns their
        // cleanup; the Store only ever sees a materialized package.
        let materialized = self.acquire(&source)?;
        self.install_materialized(
            materialized,
            "local",
            None,
            authority,
            &NoNameCollisionAuthority,
        )
    }

    /// Installs bytes that already exist locally: asks `authority` about any
    /// execution they introduce, then ingests and delivers them.
    ///
    /// Deliberately takes no lock: every caller holds one already, and
    /// `MutationLock` is not reentrant.
    ///
    /// `active_name` requests a local name other than the package's own bare
    /// plugin name (ADR-038); an update uses it to keep an alias a past
    /// collision resolution gave the package.
    pub(crate) fn install_materialized(
        &self,
        materialized: uze_core::MaterializedPackage,
        marketplace: &str,
        active_name: Option<&str>,
        authority: &dyn TrustAuthority,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        // Trust is decided here — after the package is materialized and can
        // be inspected honestly, and strictly before anything is written to
        // the Store or shown to a harness. Neither the Store nor any
        // integration knows this question exists.
        self.0.authorize(&materialized, authority, &[], false)?;
        self.install_authorized(materialized, marketplace, active_name, name_authority)
    }

    /// `install_materialized` for bytes whose trust question was already
    /// answered — an update asks it against the revision it replaces.
    pub(super) fn install_authorized(
        &self,
        materialized: uze_core::MaterializedPackage,
        marketplace: &str,
        active_name: Option<&str>,
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
        // person looking. `uze doctor` (`Health::maintain`) is the
        // explicit, narrower place that reconciliation belongs; a blocked
        // install's `ProjectionConflict` is the correct, honest outcome
        // when the ambiguity can't be resolved silently.

        // `uze add` is deliberately enough for a harness the user already
        // has.  Preparing a detected integration only creates UZE's own
        // prerequisites (such as a user-scope discovery directory) and
        // records its setup state; it never installs, upgrades, or launches
        // the vendor executable.  Do it before ingesting so a preparation
        // failure cannot leave a newly installed package with no reported
        // delivery attempt.
        self.0.prepare_detected_integrations()?;

        let installed = self.ingest_resolving_name_collision(
            &materialized,
            marketplace,
            active_name,
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

        let resources = uze_core::engine::package_resources(&installed)?;
        let resources: Vec<_> = resources.iter().collect();
        // A package must remain installable on a machine that has only a
        // subset of UZE's peer harnesses. `add` prepares and attaches to
        // detected harnesses; an absent executable is neither a package
        // incompatibility nor a reason to invoke its vendor CLI. Native
        // delivery reads the view; attempting it against a view that failed
        // to publish would fail for a reason that has nothing to do with
        // this package.
        let targets: Vec<(&dyn IntegrationPort, NativeDelivery)> = self
            .0
            .integrations
            .iter()
            .filter(|integration| self.0.detect_cached(integration.as_ref()).present)
            .map(|integration| {
                let native = if unpublished.contains(integration.id()) {
                    NativeDelivery::Skipped
                } else {
                    NativeDelivery::Allowed
                };
                (integration.as_ref(), native)
            })
            .collect();
        if !targets.is_empty() {
            let names: Vec<&str> = targets
                .iter()
                .map(|(integration, _)| integration.id())
                .collect();
            tracing::info!(
                target: uze_core::acquisition::git::STEP,
                step = "deliver",
                harness = names.join(", ")
            );
        }
        // Every harness at once: each delivery is its own vendor CLI's
        // startup and work, and they share nothing but the receipt ledger,
        // which serializes its own writes. Collected in the registry's order,
        // so the report — and the first error — read the same as before.
        let parent = tracing::Span::current();
        let deliveries: Vec<_> = std::thread::scope(|scope| {
            let running: Vec<_> = targets
                .iter()
                .map(|(integration, native)| {
                    let parent = parent.clone();
                    let (installed, resources) = (&installed, &resources);
                    scope.spawn(move || {
                        parent.in_scope(|| {
                            self.0
                                .deliver_package_to(installed, resources, *integration, *native)
                        })
                    })
                })
                .collect();
            running
                .into_iter()
                .map(|delivery| delivery.join().expect("a delivery thread does not panic"))
                .collect()
        });
        let mut attachments = Vec::new();
        let mut package_plans = Vec::new();
        let mut blocked = Vec::new();
        for ((integration, _), delivery) in targets.iter().zip(deliveries) {
            let delivery = delivery?;
            if let Some(plan) = delivery.plan {
                package_plans.push((integration.id().to_owned(), plan));
            }
            attachments.extend(delivery.attachments);
            blocked.extend(delivery.blocked.into_iter().map(|one| BlockedCapability {
                integration: one.integration,
                capability: one.capability,
                reason: one.reason,
            }));
        }
        Ok(AddPluginReport {
            plugin: self.0.plugin_summary(&installed)?,
            package_plans,
            attachments,
            publications,
            blocked,
        })
    }

    /// Ingests `materialized` under `requested_active_name` (or its own bare
    /// plugin name, when `None` — every ordinary install), asking
    /// `name_authority` to resolve a collision with an already-active,
    /// differently-marketplaced package instead of failing outright
    /// (ADR-038). `Alias` retries the ingest under the chosen local name.
    /// `Replace` removes the existing active package first — only once that
    /// is proven `Safe`, exactly the rule `Plugins::remove` enforces, so a
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
        let (name, existing, requested) =
            match self
                .0
                .store
                .ingest(materialized, marketplace, requested_active_name)
            {
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
                self.0.store.ingest(materialized, marketplace, Some(&alias))
            }
            NameCollisionResolution::Replace => match self.detach_and_remove(&existing, false)? {
                RemovePluginReport::Removed { .. } | RemovePluginReport::AlreadyAbsent { .. } => {
                    self.0
                        .store
                        .ingest(materialized, marketplace, requested_active_name)
                }
                RemovePluginReport::Blocked { .. } => Err(UzeError::PluginNameCollision {
                    name,
                    existing,
                    requested,
                }),
            },
        }
    }
}
