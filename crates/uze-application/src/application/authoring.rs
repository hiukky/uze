//! The authoring orchestration: scaffold → register → link, scaffold →
//! manifest entry, and the offline check, each one deterministic answer.
//!
//! Every verb here is machine-scoped by construction (ADR-019): the
//! marketplace record lives in the machine's own registry, the bytes stay
//! in the directory the author named, and no project file is touched. A
//! scaffolded marketplace is born registered **and linked** to the
//! directory it was scaffolded in, so installs read the author's working
//! tree — including not-yet-committed files — from the first moment.

use std::path::PathBuf;

use uze_core::{Result, UzeError, authoring};

use super::services::Project;

impl Project<'_> {
    /// Scaffolds a marketplace at `at`, registers it under `name`, and
    /// links it to the directory it was born in — one deterministic step.
    ///
    /// Registration goes through the same `market add` path an operator's
    /// registration takes, so a scaffolded marketplace is validated
    /// exactly like one anybody added by hand; the link reuses the
    /// registry's own machinery, which refuses a checkout that is some
    /// other repository.
    #[tracing::instrument(name = "authoring.marketplace_create", skip_all, fields(name = %name, at = %at.display()), err)]
    pub fn create_marketplace(
        &self,
        name: &str,
        description: Option<&str>,
        at: &PathBuf,
    ) -> Result<MarketplaceCreated> {
        let root = authoring::scaffold_marketplace(name, description, at)?;
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // The same `market add` path an operator's registration takes —
        // the manifest is validated the same way, and the marketplace is
        // named by what the manifest itself says.
        self.0.marketplace().add(&root.to_string_lossy())?;
        uze_core::state::marketplace_link(&self.0.home, name, &root)?;
        self.0.marketplace_catalogues.invalidate(name);
        Ok(MarketplaceCreated {
            name: name.to_owned(),
            root,
            committed: true,
        })
    }

    /// Scaffolds one plugin inside a marketplace this machine knows by
    /// `name`, and adds the entry that makes it installable.
    #[tracing::instrument(name = "authoring.plugin_create", skip_all, fields(name = %name, market = %market), err)]
    pub fn create_plugin(
        &self,
        market: &str,
        name: &str,
        description: Option<&str>,
        hook: bool,
        mcp: bool,
        instructions: bool,
    ) -> Result<PluginCreated> {
        let checkout = self.marketplace_checkout(market)?;
        let root =
            authoring::scaffold_plugin(&checkout, name, description, hook, mcp, instructions)?;
        Ok(PluginCreated {
            name: name.to_owned(),
            market: market.to_owned(),
            root,
        })
    }

    /// Where a registered marketplace's bytes are on this machine: a
    /// linked checkout answers first (an author edits it), else the
    /// registered source when it is a local path. A Git-registered
    /// marketplace has no directory to author in — its mirror is UZE's
    /// cache, not the author's own text.
    fn marketplace_checkout(&self, market: &str) -> Result<PathBuf> {
        let record = uze_core::state::marketplace_get(&self.0.home, market)?
            .ok_or_else(|| UzeError::UnknownMarketplace(market.to_owned()))?;
        let linked = record.link.clone().ok_or_else(|| {
            UzeError::MarketplaceScaffold(format!(
                "`{market}` is not linked to a checkout on this machine — authoring needs the \
                 marketplace the author edits, so scaffold a marketplace or `uze market link \
                 {market} <checkout>` first"
            ))
        })?;
        Ok(linked)
    }

    /// The offline check: what the authored artifact would deliver, and
    /// every finding the install would have surfaced.
    #[tracing::instrument(name = "authoring.check", skip_all, fields(path = %path.display()), err)]
    pub fn check(
        &self,
        path: &PathBuf,
        as_marketplace: bool,
    ) -> Result<authoring::ValidationReport> {
        if as_marketplace {
            authoring::check_marketplace(path)
        } else {
            authoring::check_plugin(path)
        }
    }
}

/// A scaffolded marketplace's answer: where the bytes are, and that the
/// machine registry now carries it linked.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MarketplaceCreated {
    pub name: String,
    pub root: PathBuf,
    /// The initial commit the scaffold made — what makes this directory a
    /// marketplace at all.
    pub committed: bool,
}

/// A scaffolded plugin: where it is, and the marketplace it is installable
/// from right now.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PluginCreated {
    pub name: String,
    pub market: String,
    pub root: PathBuf,
}
