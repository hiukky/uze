//! L1 contract: the Core owns packages, integrations own their derived views.
//!
//! These assertions exist to keep one property true as the harness set grows:
//! *adding a harness must not require knowledge of that harness in the Store,
//! the Engine, the Router or the package model.* They are written against the
//! `IntegrationPort` seam with fake integrations, so they hold for a harness
//! that does not exist yet.

use std::{cell::RefCell, collections::BTreeSet, fs, path::PathBuf};

use uze::{
    PackageExposurePlan, Resource, UzeApplication, UzeEngine, UzeHome, UzeStore,
    exposure::ExposurePlan,
    integration::{
        AttachmentReceipt, HarnessDetection, IntegrationPort, ManagedArtifact, PublicationStatus,
    },
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    store::StoredPackage,
};

/// The acquisition pipeline every install now goes through: a source is
/// acquired into a materialized package, and only then does the Store ingest
/// it. Spelled out here rather than hidden behind a Store convenience,
/// because the Store deliberately no longer accepts a path.
fn install(
    store: &UzeStore,
    path: impl Into<std::path::PathBuf>,
) -> uze::Result<uze::StoredPackage> {
    store.ingest(&uze::acquisition::acquire(&uze::PackageSource::local(
        path,
    ))?)
}

fn native_package_fixture() -> PathBuf {
    uze_testkit::fixtures::foreign("codex", "native-plugin")
}

fn plain_package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("skill-plugin")
}

fn temporary_home(label: &str) -> UzeHome {
    UzeHome::at(uze_testkit::temp::scratch(label))
}

/// Publishes a derived view of the installed package set, exactly the way a
/// harness integration would. Nothing about it is specific to any real
/// harness — which is the point.
struct PublishingIntegration {
    root: PathBuf,
    /// Set to make publication fail, so the caller can assert that a failed
    /// view never invalidates an installation.
    fail: bool,
    calls: RefCell<Vec<Vec<String>>>,
}

impl PublishingIntegration {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            fail: false,
            calls: RefCell::new(Vec::new()),
        }
    }

    fn failing(root: PathBuf) -> Self {
        Self {
            fail: true,
            ..Self::new(root)
        }
    }

    fn catalogue(&self) -> PathBuf {
        self.root.join("catalogue.json")
    }

    /// Only packages carrying this fake harness's envelope belong in the view.
    /// The filter is the integration's policy, never the Store's.
    fn publishable(packages: &[StoredPackage]) -> Vec<String> {
        packages
            .iter()
            .filter(|package| package.root.join(".codex-plugin/plugin.json").is_file())
            .map(|package| package.id.as_str().to_owned())
            .collect()
    }
}

impl IntegrationPort for PublishingIntegration {
    fn id(&self) -> &'static str {
        "fake-native"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["fake"]
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Unsupported,
            verification: VerificationStatus::Unverified,
            mechanism: uze::ExposureMechanism::Unsupported {
                rationale: "fake integration exposes nothing individually".to_owned(),
            },
            evidence: "test double".to_owned(),
        }
    }

    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: None,
        }
    }

    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<(), uze::UzeError> {
        let names = Self::publishable(packages);
        self.calls.borrow_mut().push(names.clone());
        if self.fail {
            return Err(uze::UzeError::ExposureUnavailable(
                "fake publication failure".to_owned(),
            ));
        }
        fs::create_dir_all(&self.root).map_err(|source| uze::UzeError::Write {
            path: self.root.clone(),
            source,
        })?;
        fs::write(self.catalogue(), names.join("\n")).map_err(|source| uze::UzeError::Write {
            path: self.catalogue(),
            source,
        })
    }

    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        let expected = Self::publishable(packages).join("\n");
        match fs::read_to_string(self.catalogue()) {
            Ok(actual) if actual == expected => PublicationStatus::Published,
            Ok(_) => PublicationStatus::Unpublished("catalogue is stale".to_owned()),
            Err(_) if expected.is_empty() => PublicationStatus::Published,
            Err(_) => PublicationStatus::Unpublished("catalogue is absent".to_owned()),
        }
    }
}

/// Publishes nothing at all — the shape most integrations will have.
struct QuietIntegration;

impl IntegrationPort for QuietIntegration {
    fn id(&self) -> &'static str {
        "fake-quiet"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Unsupported,
            verification: VerificationStatus::Unverified,
            mechanism: uze::ExposureMechanism::Unsupported {
                rationale: "quiet".to_owned(),
            },
            evidence: "test double".to_owned(),
        }
    }
}

#[test]
fn the_store_writes_no_harness_owned_artifact_of_its_own_accord() {
    let home = temporary_home("store-writes");
    let store = UzeStore::new(home.clone());
    install(&store, native_package_fixture())
        .expect("a package carrying a native envelope installs");

    // The package tree and UZE's own state are the only things the Store
    // produces. A catalogue beside them would mean the Store still knows a
    // harness.
    let store_entries: BTreeSet<String> = fs::read_dir(home.store_dir())
        .expect("store dir exists")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        store_entries,
        BTreeSet::from(["packages".to_owned()]),
        "the Store published something a harness reads"
    );

    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn republish_is_a_noop_for_an_integration_that_publishes_nothing() {
    let home = temporary_home("quiet");
    let application = UzeApplication::new(home.clone(), vec![Box::new(QuietIntegration)]);
    application
        .add_plugin(
            uze::PackageSource::local(plain_package_fixture()),
            &uze::trust::AlwaysTrust,
        )
        .expect("install succeeds with an integration that publishes nothing");

    let report = application.doctor();
    let quiet = &report.harnesses[0];
    assert_eq!(quiet.publication, PublicationStatus::NotApplicable);

    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn a_derived_view_is_rebuilt_from_the_package_set_alone() {
    let home = temporary_home("rebuild");
    let views = home.root().join("views");
    let application = UzeApplication::new(
        home.clone(),
        vec![Box::new(PublishingIntegration::new(views.clone()))],
    );
    application
        .add_plugin(
            uze::PackageSource::local(native_package_fixture()),
            &uze::trust::AlwaysTrust,
        )
        .expect("install succeeds");

    let catalogue = views.join("catalogue.json");
    let published = fs::read_to_string(&catalogue).expect("view was published");
    assert!(published.contains("uze-plugin-first-conformance"));

    // Corrupt it, then prove the view holds nothing that exists only there:
    // `setup` reconstructs it byte for byte from the Store.
    fs::write(&catalogue, "garbage").unwrap();
    assert!(matches!(
        application.doctor().harnesses[0].publication,
        PublicationStatus::Unpublished(_)
    ));
    application.setup(None).expect("setup repairs the view");
    assert_eq!(fs::read_to_string(&catalogue).unwrap(), published);
    assert_eq!(
        application.doctor().harnesses[0].publication,
        PublicationStatus::Published
    );

    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn a_package_without_the_native_envelope_is_not_published() {
    let home = temporary_home("filter");
    let views = home.root().join("views");
    let application = UzeApplication::new(
        home.clone(),
        vec![Box::new(PublishingIntegration::new(views.clone()))],
    );
    application
        .add_plugin(
            uze::PackageSource::local(plain_package_fixture()),
            &uze::trust::AlwaysTrust,
        )
        .expect("install succeeds");

    // Which packages belong in a view is the integration's policy. The Store
    // installed the package regardless.
    assert_eq!(
        fs::read_to_string(views.join("catalogue.json")).unwrap(),
        ""
    );
    assert_eq!(application.list_plugins().unwrap().len(), 1);

    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn a_failed_publication_leaves_the_package_installed_and_says_so() {
    let home = temporary_home("failed-publication");
    let views = home.root().join("views");
    let application = UzeApplication::new(
        home.clone(),
        vec![
            Box::new(PublishingIntegration::failing(views.clone())),
            Box::new(QuietIntegration),
        ],
    );

    let report = application
        .add_plugin(
            uze::PackageSource::local(native_package_fixture()),
            &uze::trust::AlwaysTrust,
        )
        .expect("a failed derived view never fails the installation");

    // The package is a valid UZE installation.
    assert_eq!(application.list_plugins().unwrap().len(), 1);
    assert!(report.plugin.store_path.is_dir());

    // And the failure is reported, per integration, actionably.
    let failed: Vec<&str> = report
        .publications
        .iter()
        .filter(|outcome| outcome.error.is_some())
        .map(|outcome| outcome.integration.as_str())
        .collect();
    assert_eq!(failed, vec!["fake-native"]);

    // Doctor observes it independently of what add_plugin reported.
    let doctor = application.doctor();
    let publishing = doctor
        .harnesses
        .iter()
        .find(|harness| harness.integration == "fake-native")
        .unwrap();
    assert!(matches!(
        publishing.publication,
        PublicationStatus::Unpublished(_)
    ));
    let quiet = doctor
        .harnesses
        .iter()
        .find(|harness| harness.integration == "fake-quiet")
        .unwrap();
    assert_eq!(
        quiet.publication,
        PublicationStatus::NotApplicable,
        "one integration's failure must not affect another"
    );

    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn harness_selection_comes_from_the_registered_integrations() {
    let home = temporary_home("selection");
    let views = home.root().join("views");
    let application = UzeApplication::new(
        home.clone(),
        vec![Box::new(PublishingIntegration::new(views))],
    );

    // By id, and by an alias the integration itself declares.
    for name in ["fake-native", "fake"] {
        let results = application.setup(Some(name)).expect("registered harness");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].integration, "fake-native");
    }

    // A harness the composition root does not register is unknown, and the
    // error names what *is* registered rather than a hardcoded catalogue.
    let error = application.setup(Some("codex")).unwrap_err().to_string();
    assert!(error.contains("fake-native"), "error was: {error}");

    let _ = fs::remove_dir_all(home.root());
}

#[test]
fn native_package_delivery_still_suppresses_individual_attachment() {
    // The Core learns only *that* a harness consumes the package natively and
    // which resources that covers — never how.
    let home = temporary_home("suppression");
    let store = UzeStore::new(home.clone());
    let package = install(&store, native_package_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resources: Vec<&Resource> = environment.resources.iter().collect();

    let plan = PackageExposurePlan {
        package_id: package.id.clone(),
        route: CompatibilityRoute::Native,
        verification: VerificationStatus::Unverified,
        provided_resource_identities: resources
            .iter()
            .map(|resource| resource.identity())
            .collect(),
        evidence: "test double".to_owned(),
    };
    assert!(resources.iter().all(|resource| plan.provides(resource)));
    assert_eq!(plan.provided_resource_identities.len(), 2);

    let _ = fs::remove_dir_all(home.root());
}

/// Structural counterpart to the vendor-neutrality greps: the Store must not
/// learn source mechanisms as it learned harnesses.
///
/// It asserts on the source text because that is the only way to catch the
/// failure this guards against — a future contributor reaching for a quick
/// `if source is git` inside the Store. Provenance reaches `store.rs` as an
/// opaque value it persists and compares through `same_origin`; it never
/// matches a variant or reads a field.
#[test]
fn the_store_contains_no_source_mechanism_semantics() {
    let store = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/uze-core/src/store.rs"),
    )
    .expect("store source is readable");

    // Deliberately not `clone`/`fetch`: those are ordinary Rust vocabulary
    // here, and a check that cries wolf gets deleted rather than fixed.
    for forbidden in [
        "git ",
        "Git",
        "github",
        "GitHub",
        "gitlab",
        "https://",
        "git clone",
        "revision",
        "commit",
        "branch",
        "PackageSource::",
        "ResolvedSource",
    ] {
        assert!(
            !store.contains(forbidden),
            "store.rs mentions `{forbidden}`; source mechanisms belong to acquisition"
        );
    }

    // It may name the opaque carrier and the one comparison it is allowed to
    // make — that is the whole of its provenance vocabulary.
    assert!(store.contains("Provenance"));
    assert!(store.contains("same_origin"));
}

/// The Engine, Router and package model must not *depend* on acquisition.
///
/// Comments are stripped before checking, deliberately. The invariant is
/// about the dependency edge, not the vocabulary: `acquisition` depends on
/// `engine` — which is the correct direction — and the modules on that side
/// are allowed to explain why a rule exists. Banning the word would only
/// teach contributors to delete the explanation.
#[test]
fn no_core_module_depends_on_acquisition() {
    for module in [
        "crates/uze-core/src/engine.rs",
        "crates/uze-core/src/router.rs",
        "crates/uze-core/src/capability.rs",
        "crates/uze-core/src/project.rs",
        "crates/uze-core/src/store.rs",
    ] {
        let text = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(module))
            .expect("core source is readable");
        let code: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in ["crate::acquisition", "PackageSource", "ResolvedSource"] {
            assert!(
                !code.contains(forbidden),
                "{module} depends on `{forbidden}`; acquisition is not a core concern"
            );
        }
    }
}

#[test]
fn an_integration_owned_receipt_round_trips_through_the_ledger() {
    let receipt = AttachmentReceipt {
        package_id: "plugin-a".to_owned(),
        resource_identity: None,
        integration: "fake-native".to_owned(),
        strategy: "whatever-the-integration-calls-it".to_owned(),
        artifact: ManagedArtifact::IntegrationOwned {
            kind: "fake-catalogue-entry".to_owned(),
            selector: "plugin-a@fake".to_owned(),
            detail: [("anything".to_owned(), serde_json::json!({"nested": 1}))]
                .into_iter()
                .collect(),
        },
    };
    let encoded = serde_json::to_string(&receipt).unwrap();
    let decoded: AttachmentReceipt = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, receipt, "the Core must not lose opaque detail");
}
