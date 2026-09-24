//! The derived marketplaces Claude Code and Codex install native plugins
//! from (ADR-013).
//!
//! Each harness gets two: `uze-local`, rooted at the Store itself, for the
//! packages that ship the harness's own envelope, and `uze-store`, rooted
//! under UZE's state, for the packages UZE synthesizes one for. Which
//! packages each catalogue lists, how a generated envelope is rebuilt and
//! removed, what a receipt records and when publication is current are one
//! lifecycle; a [`MarketplaceDialect`] supplies only what the vendor spells
//! differently — its envelope and catalogue files, its JSON, how it
//! preserves a Skill's invocation policy and its `plugin` CLI verbs.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    Result, UzeError,
    capability::{CapabilityKind, Resource},
    exposure::PackageExposurePlan,
    home::UzeHome,
    integration::{AttachmentInspection, AttachmentReceipt, ManagedArtifact, PublicationStatus},
    router::CompatibilityRoute,
    skill::SkillInvocationPolicy,
    store::{StoredPackage, is_valid_qualified_id},
};

use crate::shared::plan::blocked;

/// What one vendor spells differently about its derived marketplaces.
pub(crate) trait MarketplaceDialect {
    /// The harness's directory under UZE's attachment state.
    const VENDOR: &'static str;
    /// The name `uze setup` takes for this harness.
    const SETUP_NAME: &'static str;
    /// How messages name the catalogue (`Claude marketplace`).
    const CATALOGUE_NOUN: &'static str;
    /// The directory a package's own envelope manifest lives in.
    const ENVELOPE_DIR: &'static str;
    /// The catalogue file, relative to its marketplace root.
    const CATALOGUE_PATH: &'static str;
    /// Receipt `kind` for a package installed from its own envelope.
    const EXPLICIT_KIND: &'static str;
    /// Receipt `kind` for a package installed from a generated envelope.
    const GENERATED_KIND: &'static str;
    const EXPLICIT_EVIDENCE: &'static str;
    const GENERATED_EVIDENCE: &'static str;

    /// The whole catalogue document around its `plugins`.
    fn catalogue_document(
        name: &str,
        display_name: &str,
        plugins: Vec<serde_json::Value>,
    ) -> serde_json::Value;

    /// One package's catalogue entry, `source` relative to the marketplace
    /// root.
    fn catalogue_entry(
        package: &StoredPackage,
        source: String,
        origin: Origin,
    ) -> serde_json::Value;

    /// Which resources the package's own envelope provides (ADR-013 §2).
    fn explicit_coverage(package: &StoredPackage, resources: &[&Resource]) -> BTreeSet<String>;

    /// Whether a generated envelope carries this Skill policy faithfully
    /// (ADR-030 §6); one it cannot is left to capability-level delivery.
    fn envelope_preserves(policy: SkillInvocationPolicy) -> bool;

    /// Writes the generated manifest and every surface it declares into the
    /// fresh envelope directory `dir`.
    fn materialize_envelope(package: &StoredPackage, dir: &Path) -> Result<()>;

    /// The `package_root` a generated receipt records: the path the vendor
    /// itself reports the installed plugin at.
    fn generated_receipt_root(package: &StoredPackage, envelope_dir: &Path) -> PathBuf;

    fn marketplace_exists(executable: &Path, home: &Path, root: &Path) -> bool;
    fn add_marketplace(executable: &Path, home: &Path, root: &Path) -> Result<()>;
    /// Whether the harness already has `selector` installed, for a harness
    /// whose install is not relied on to be idempotent. Asked alongside
    /// [`Self::marketplace_exists`], since the two are independent reads of
    /// the same CLI.
    fn plugin_installed(_executable: &Path, _home: &Path, _selector: &str) -> bool {
        false
    }
    fn install_plugin(executable: &Path, home: &Path, selector: &str) -> Result<()>;
    fn inspect_plugin(
        executable: &Path,
        home: &Path,
        selector: &str,
        marketplace_root: &Path,
        detail: &BTreeMap<String, serde_json::Value>,
    ) -> AttachmentInspection;
    fn remove_plugin(executable: &Path, home: &Path, selector: &str) -> Result<()>;
}

/// Which of the two marketplaces a package is delivered through.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Origin {
    /// The package ships the harness's own envelope.
    Explicit,
    /// UZE synthesizes the envelope into a derived directory.
    Generated,
}

impl Origin {
    /// Distinct names, so a generated envelope can never be confused with,
    /// or silently override, an author-provided one.
    pub(crate) const fn marketplace_name(self) -> &'static str {
        match self {
            Self::Explicit => "uze-local",
            Self::Generated => "uze-store",
        }
    }

    const fn display_name(self) -> &'static str {
        match self {
            Self::Explicit => "UZE Local",
            Self::Generated => "UZE Local (generated)",
        }
    }
}

pub(crate) fn has_envelope<D: MarketplaceDialect>(package: &StoredPackage) -> bool {
    package
        .root
        .join(D::ENVELOPE_DIR)
        .join("plugin.json")
        .is_file()
}

/// Whether UZE can safely synthesize an envelope: none of the package's
/// own, and at least one structural surface to generate from — a
/// conventional `skills/` directory or a root `mcp.json`.
pub(crate) fn generatable<D: MarketplaceDialect>(package: &StoredPackage) -> bool {
    !has_envelope::<D>(package)
        && (package.root.join("skills").is_dir() || package.root.join("mcp.json").is_file())
}

/// Root of every generated envelope, and the generated marketplace's own
/// root: under UZE's attachment state, never under the Store.
pub(crate) fn generated_root<D: MarketplaceDialect>(uze_home: &UzeHome) -> PathBuf {
    crate::shared::path::attachment_root(uze_home, D::VENDOR).join("generated")
}

pub(crate) fn generated_package_dir<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    package_id: &str,
) -> PathBuf {
    generated_root::<D>(uze_home).join(package_id)
}

/// The root a marketplace is registered at. The explicit one is the Store:
/// Codex resolves a catalogue entry's `source.path` relative to it and
/// rejects both absolute paths and relative paths escaping it (confirmed
/// against Codex 0.148.0), so the catalogue sits beside the packages.
fn marketplace_root<D: MarketplaceDialect>(uze_home: &UzeHome, origin: Origin) -> PathBuf {
    match origin {
        Origin::Explicit => uze_home.store_dir(),
        Origin::Generated => generated_root::<D>(uze_home),
    }
}

fn catalogue_path<D: MarketplaceDialect>(uze_home: &UzeHome, origin: Origin) -> PathBuf {
    marketplace_root::<D>(uze_home, origin).join(D::CATALOGUE_PATH)
}

fn members<D: MarketplaceDialect>(
    packages: &[StoredPackage],
    origin: Origin,
) -> Vec<&StoredPackage> {
    packages
        .iter()
        .filter(|package| match origin {
            Origin::Explicit => has_envelope::<D>(package),
            Origin::Generated => generatable::<D>(package),
        })
        .collect()
}

/// A catalogue derived purely from the installed package set: delete the
/// file and this rebuilds it byte for byte from the Store.
pub(crate) fn catalogue_document<D: MarketplaceDialect>(
    packages: &[StoredPackage],
    origin: Origin,
) -> serde_json::Value {
    let plugins = members::<D>(packages, origin)
        .into_iter()
        .map(|package| {
            let source = match origin {
                Origin::Explicit => format!(
                    "./plugins/{}/{}",
                    package.id.marketplace(),
                    package.id.plugin_name()
                ),
                Origin::Generated => format!("./{}", package.id.as_str()),
            };
            D::catalogue_entry(package, source, origin)
        })
        .collect();
    D::catalogue_document(origin.marketplace_name(), origin.display_name(), plugins)
}

/// The package-level plan: the package's own envelope when it ships one,
/// else a generated one when anything is safely representable (ADR-013 §3).
/// Read-only either way.
pub(crate) fn package_plan<D: MarketplaceDialect>(
    package: &StoredPackage,
    resources: &[&Resource],
) -> Option<PackageExposurePlan> {
    let (provided, evidence) = if has_envelope::<D>(package) {
        (
            D::explicit_coverage(package, resources),
            D::EXPLICIT_EVIDENCE,
        )
    } else if generatable::<D>(package) {
        (
            generated_exact_coverage::<D>(package, resources),
            D::GENERATED_EVIDENCE,
        )
    } else {
        return None;
    };
    Some(PackageExposurePlan {
        package_id: package.id.clone(),
        route: CompatibilityRoute::Native,
        provided_resource_identities: provided,
        evidence: evidence.to_owned(),
    })
}

/// The resources a generated envelope provides, computed against what it
/// can preserve rather than by re-reading a manifest it wrote, so generation
/// and coverage agree by construction: a Skill under `skills/` whose policy
/// the envelope carries, and an MCP server named in the package's
/// `mcp.json`.
pub(crate) fn generated_exact_coverage<D: MarketplaceDialect>(
    package: &StoredPackage,
    resources: &[&Resource],
) -> BTreeSet<String> {
    let declared_mcp = canonical_mcp_server_names(package);
    resources
        .iter()
        .filter(|resource| match resource.capability.kind {
            CapabilityKind::AgentSkill => {
                in_skills_dir(package, resource)
                    && D::envelope_preserves(resource.skill_invocation())
            }
            CapabilityKind::Mcp => resource
                .resource_name
                .as_ref()
                .is_some_and(|name| declared_mcp.contains(name)),
            _ => false,
        })
        .map(|resource| resource.identity())
        .collect()
}

fn in_skills_dir(package: &StoredPackage, resource: &Resource) -> bool {
    resource
        .capability
        .path
        .strip_prefix(&package.root)
        .ok()
        .and_then(Path::parent)
        .is_some_and(|parent| parent.starts_with("skills"))
}

fn canonical_mcp_server_names(package: &StoredPackage) -> BTreeSet<String> {
    canonical_mcp_servers(package)
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default()
}

/// The `mcpServers` object of the package's canonical `mcp.json`.
pub(crate) fn canonical_mcp_servers(
    package: &StoredPackage,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("mcpServers")?.as_object().cloned())
}

/// The `mcpServers` value of the package's canonical `mcp.json`, whatever
/// its shape, for a manifest that carries it inline verbatim.
pub(crate) fn canonical_mcp_manifest_value(package: &StoredPackage) -> Option<serde_json::Value> {
    fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("mcpServers").cloned())
}

/// `description` and `version` from a manifest, each defaulted when absent
/// or unreadable — never invented beyond that.
pub(crate) fn manifest_fields(manifest: &Path, default_description: &str) -> (String, String) {
    let document = fs::read(manifest)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let field = |key: &str, default: &str| {
        document
            .as_ref()
            .and_then(|value| value.get(key)?.as_str())
            .unwrap_or(default)
            .to_owned()
    };
    (
        field("description", default_description),
        field("version", "0.1.0"),
    )
}

/// Rebuilds one package's generated envelope wholesale — never patched,
/// because the directory is UZE-owned and non-authoritative (ADR-013 §5).
pub(crate) fn materialize_generated_package<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    package: &StoredPackage,
) -> Result<PathBuf> {
    let dir = generated_package_dir::<D>(uze_home, package.id.as_str());
    crate::shared::skill::recreate_dir(&dir)?;
    D::materialize_envelope(package, &dir)?;
    Ok(dir)
}

/// Removes one package's generated envelope by the id its receipt carries.
pub(crate) fn remove_generated_package<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    package_id: &str,
) -> Result<()> {
    // The id comes from the receipt ledger, not a constructor: refuse one
    // that could not have been a real package id instead of joining it into
    // a path and removing whatever the traversal lands on.
    if !is_valid_qualified_id(package_id) {
        return Err(UzeError::ExposureUnavailable(format!(
            "refusing to remove generated envelope for malformed package id `{package_id}`"
        )));
    }
    let dir = generated_package_dir::<D>(uze_home, package_id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|source| UzeError::Write { path: dir, source })?;
    }
    Ok(())
}

/// Rewrites both catalogues and every generated envelope they reference. A
/// package that stopped being generatable simply drops out; its orphaned
/// envelope is removed at detach time.
pub(crate) fn republish<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    packages: &[StoredPackage],
) -> Result<()> {
    write_catalogue::<D>(uze_home, packages, Origin::Explicit)?;
    for package in members::<D>(packages, Origin::Generated) {
        materialize_generated_package::<D>(uze_home, package)?;
    }
    write_catalogue::<D>(uze_home, packages, Origin::Generated)
}

fn write_catalogue<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    packages: &[StoredPackage],
    origin: Origin,
) -> Result<()> {
    uze_core::persistence::write_atomic(
        &catalogue_path::<D>(uze_home, origin),
        &serde_json::to_vec_pretty(&catalogue_document::<D>(packages, origin))
            .expect("catalogue is serializable"),
    )
}

/// Whether both catalogues, and every envelope the generated one names,
/// match the installed package set.
pub(crate) fn publication<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    packages: &[StoredPackage],
) -> PublicationStatus {
    let noun = D::CATALOGUE_NOUN;
    let rerun = format!("re-run `uze setup {}`", D::SETUP_NAME);
    let expected = catalogue_document::<D>(packages, Origin::Explicit);
    let explicit = match fs::read(catalogue_path::<D>(uze_home, Origin::Explicit)) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(actual) if actual == expected => Ok(()),
            Ok(_) => Err(format!(
                "the {noun} does not match the installed package set; {rerun}"
            )),
            Err(error) => Err(format!("the {noun} is unreadable ({error}); {rerun}")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if members::<D>(packages, Origin::Explicit).is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "no {noun} has been written for the installed packages; {rerun}"
                ))
            }
        }
        Err(error) => Err(error.to_string()),
    };
    if let Err(reason) = explicit {
        return PublicationStatus::Unpublished(reason);
    }
    if !generated_catalogue_matches::<D>(uze_home, packages)
        || !generated_packages_present::<D>(uze_home, packages)
    {
        return PublicationStatus::Unpublished(format!(
            "the generated {noun} does not match the installed package set; {rerun}"
        ));
    }
    PublicationStatus::Published
}

fn generated_catalogue_matches<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    packages: &[StoredPackage],
) -> bool {
    let expected = catalogue_document::<D>(packages, Origin::Generated);
    match fs::read(catalogue_path::<D>(uze_home, Origin::Generated)) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .is_ok_and(|actual| actual == expected),
        Err(_) => members::<D>(packages, Origin::Generated).is_empty(),
    }
}

/// The catalogue matching is not enough on its own once republishing is
/// gated on it: a hand-removed envelope would otherwise stay missing until
/// something else rewrote the view.
fn generated_packages_present<D: MarketplaceDialect>(
    uze_home: &UzeHome,
    packages: &[StoredPackage],
) -> bool {
    members::<D>(packages, Origin::Generated)
        .into_iter()
        .all(|package| {
            generated_package_dir::<D>(uze_home, package.id.as_str())
                .join(D::ENVELOPE_DIR)
                .join("plugin.json")
                .is_file()
        })
}

/// Installs a package through its marketplace — registering the marketplace
/// first when the harness does not know it — and returns the receipt.
pub(crate) fn attach_package<D: MarketplaceDialect>(
    executable: &Path,
    command_home: &Path,
    uze_home: &UzeHome,
    integration_id: &str,
    package: &StoredPackage,
) -> Result<AttachmentReceipt> {
    let (origin, package_root) = if has_envelope::<D>(package) {
        (Origin::Explicit, package.root.clone())
    } else {
        let envelope = materialize_generated_package::<D>(uze_home, package)?;
        (
            Origin::Generated,
            D::generated_receipt_root(package, &envelope),
        )
    };
    let root = marketplace_root::<D>(uze_home, origin);
    let selector = format!(
        "{}@{}",
        package.active_name.as_str(),
        origin.marketplace_name()
    );
    // Two independent questions to one CLI, each a process start of its
    // own: asked at once, they cost the slower of the two.
    let parent = tracing::Span::current();
    let (known, installed) = std::thread::scope(|scope| {
        let known = scope
            .spawn(|| parent.in_scope(|| D::marketplace_exists(executable, command_home, &root)));
        let installed = D::plugin_installed(executable, command_home, &selector);
        (known.join().unwrap_or(false), installed)
    });
    if !known {
        D::add_marketplace(executable, command_home, &root)?;
    }
    if !installed {
        D::install_plugin(executable, command_home, &selector)?;
    }
    let mut detail: BTreeMap<String, serde_json::Value> = [
        ("marketplace_root".to_owned(), serde_json::json!(root)),
        ("package_root".to_owned(), serde_json::json!(package_root)),
    ]
    .into_iter()
    .collect();
    let kind = match origin {
        Origin::Explicit => D::EXPLICIT_KIND,
        Origin::Generated => {
            detail.insert("origin".to_owned(), serde_json::json!("generated"));
            D::GENERATED_KIND
        }
    };
    Ok(AttachmentReceipt {
        package_id: package.id.as_str().to_owned(),
        resource_identity: None,
        integration: integration_id.to_owned(),
        artifact: ManagedArtifact::IntegrationOwned {
            kind: kind.to_owned(),
            selector,
            detail,
        },
    })
}

/// The marketplace a receipt `kind` belongs to, if it is one of this
/// dialect's.
pub(crate) fn receipt_origin<D: MarketplaceDialect>(kind: &str) -> Option<Origin> {
    if kind == D::EXPLICIT_KIND {
        Some(Origin::Explicit)
    } else if kind == D::GENERATED_KIND {
        Some(Origin::Generated)
    } else {
        None
    }
}

pub(crate) fn inspect_package<D: MarketplaceDialect>(
    executable: &Path,
    command_home: &Path,
    selector: &str,
    detail: &BTreeMap<String, serde_json::Value>,
) -> AttachmentInspection {
    let Some(marketplace_root) = detail_path(detail, "marketplace_root") else {
        return blocked("plugin receipt has no marketplace root");
    };
    D::inspect_plugin(
        executable,
        command_home,
        selector,
        &marketplace_root,
        detail,
    )
}

/// Uninstalls the plugin, then the generated envelope behind it — a Derived
/// Artifact nothing references any more (ADR-013 §5).
pub(crate) fn detach_package<D: MarketplaceDialect>(
    executable: &Path,
    command_home: &Path,
    uze_home: &UzeHome,
    receipt: &AttachmentReceipt,
    selector: &str,
    origin: Origin,
) -> Result<()> {
    D::remove_plugin(executable, command_home, selector)?;
    if origin == Origin::Generated {
        remove_generated_package::<D>(uze_home, &receipt.package_id)?;
    }
    Ok(())
}

/// Reads one path out of an opaque receipt payload.
pub(crate) fn detail_path(
    detail: &BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<PathBuf> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

/// The `plugin marketplace list --json` entries, in either shape a vendor
/// has answered with: a bare array or `{"marketplaces": [...]}`.
pub(crate) fn marketplace_entries(listing: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    listing.as_array().or_else(|| {
        listing
            .get("marketplaces")
            .and_then(serde_json::Value::as_array)
    })
}
