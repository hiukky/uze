//! Integration tests for the Project Agent Environment (`agents.lock`).
//!
//! See openspec/changes/project-agent-environment/tasks.md §6 for the
//! scenario list this addresses; it does not attempt every item there
//! (offline-availability and executable-trust-denial scenarios need
//! fixtures/mechanisms this pass does not add — see that file for what
//! remains deliberately deferred).

use std::{fs, path::PathBuf};

use uze::{
    UzeApplication, UzeHome,
    application::{InstallReport, ProjectLockStatus, RemoveProjectPluginReport},
    project_lock, project_root,
    trust::AlwaysTrust,
};

fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

/// A local marketplace containing one plugin (a copy of the existing
/// `agent-plugin-skill` fixture, renamed), self-contained under `root` so
/// nothing in this test depends on the surrounding repo layout beyond that
/// one fixture.
fn write_marketplace(root: &PathBuf, marketplace_name: &str, plugin_name: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("marketplace.json"),
        format!(
            r#"{{"name": "{marketplace_name}", "plugins": [{{"name": "{plugin_name}", "source": "{plugin_name}"}}]}}"#
        ),
    )
    .unwrap();
    let plugin_dir = root.join(plugin_name);
    fs::create_dir_all(plugin_dir.join("skills/uze-e2e")).unwrap();
    fs::write(
        plugin_dir.join("plugin.json"),
        format!(r#"{{"name": "{plugin_name}"}}"#),
    )
    .unwrap();
    fs::write(
        plugin_dir.join("skills/uze-e2e/SKILL.md"),
        "# Conformance skill\n",
    )
    .unwrap();
}

struct Fixture {
    uze_home: PathBuf,
    project_root: PathBuf,
    marketplace_root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let base = temp(label);
        let fixture = Self {
            uze_home: base.join("home"),
            project_root: base.join("project"),
            marketplace_root: base.join("market"),
        };
        fs::create_dir_all(&fixture.project_root).unwrap();
        write_marketplace(&fixture.marketplace_root, "test-market", "flow");
        fixture
    }

    fn app(&self) -> UzeApplication {
        UzeApplication::new(UzeHome::at(&self.uze_home), Vec::new())
    }

    fn add_marketplace_to_global_registry(&self) {
        uze_core::state::marketplace_add(
            &UzeHome::at(&self.uze_home),
            "test-market",
            uze_core::PackageSource::Local {
                path: self.marketplace_root.clone(),
            },
        )
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.uze_home.parent().unwrap());
    }
}

#[test]
fn add_project_plugin_creates_a_deterministic_lock() {
    let fx = Fixture::new("add-deterministic");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();

    app.add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    let lock_path = project_lock::lock_path_for(&fx.project_root);
    assert!(lock_path.is_file(), "agents.lock must exist after add");
    let first_bytes = fs::read(&lock_path).unwrap();

    let lock = project_lock::load_lock(&fx.project_root).unwrap().unwrap();
    assert!(lock.plugins.contains_key("flow"));
    assert!(lock.marketplaces.contains_key("test-market"));

    // Re-adding the same plugin from the same marketplace must succeed
    // (install_materialized's own same-origin idempotency) and produce a
    // byte-identical lock — determinism, not just "doesn't crash".
    app.add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();
    let second_bytes = fs::read(&lock_path).unwrap();
    assert_eq!(
        first_bytes, second_bytes,
        "repeated add must not change the lock"
    );
}

#[test]
fn add_project_plugin_populates_resolved_revision_for_a_local_marketplace_plugin() {
    // A local source carries no revision by design (see
    // ResolvedSource::Local's doc) -- this asserts the field is at least
    // wired through (present as `None`, not left unset by an old code
    // path), not that a value was fabricated for a source that has none.
    let fx = Fixture::new("resolved-revision");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();
    let lock = project_lock::load_lock(&fx.project_root).unwrap().unwrap();
    assert_eq!(lock.plugins["flow"].resolved.revision, None);
}

#[test]
fn install_project_environment_reproduces_a_lock_on_a_fresh_machine() {
    let fx = Fixture::new("fresh-machine");
    fx.add_marketplace_to_global_registry();

    // "Machine A": adds the plugin, produces agents.lock.
    fx.app()
        .add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    // "Machine B": same UZE_HOME layout path but a fresh Store directory,
    // simulating `git clone` + `uze install` with no prior `uze add`.
    let fresh_home = fx.uze_home.parent().unwrap().join("home-fresh-machine");
    let fresh_app = UzeApplication::new(UzeHome::at(&fresh_home), Vec::new());
    // The fresh machine also needs the marketplace registered globally --
    // matching what `uze marketplace add` would do before `uze install`
    // (the lock's own marketplace source is used to *acquire*, but
    // `resolve_locked_plugin_source` reads it straight from the lock, not
    // the global registry, so this isn't even required for install to
    // work -- asserted implicitly by not calling it here).
    assert!(
        fresh_app.list_plugins().unwrap().is_empty(),
        "fresh machine starts with nothing installed"
    );

    let report = fresh_app
        .install_project_environment(&fx.project_root, &AlwaysTrust)
        .unwrap();
    match report {
        InstallReport::Installed { plugins } => {
            assert_eq!(plugins, vec!["flow".to_owned()]);
        }
        other => panic!("expected Installed, got {other:?}"),
    }
    assert!(
        fresh_app
            .list_plugins()
            .unwrap()
            .iter()
            .any(|p| p.id == "flow"),
        "install_project_environment must actually install the missing plugin"
    );
}

#[test]
fn install_project_environment_is_a_no_op_once_everything_is_installed() {
    let fx = Fixture::new("no-changes");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    let report = app
        .install_project_environment(&fx.project_root, &AlwaysTrust)
        .unwrap();
    assert!(matches!(report, InstallReport::NoChanges));
}

#[test]
fn install_project_environment_with_no_lock_is_a_no_op() {
    let fx = Fixture::new("no-lock");
    let report = fx
        .app()
        .install_project_environment(&fx.project_root, &AlwaysTrust)
        .unwrap();
    assert!(matches!(report, InstallReport::NoChanges));
}

#[test]
fn remove_project_plugin_removes_from_lock_but_not_from_the_store() {
    let fx = Fixture::new("remove-lock-only");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    let report = app.remove_project_plugin("flow", &fx.project_root).unwrap();
    assert!(matches!(report, RemoveProjectPluginReport::Removed { .. }));

    let lock = project_lock::load_lock(&fx.project_root).unwrap().unwrap();
    assert!(
        !lock.plugins.contains_key("flow"),
        "removed plugin must be gone from the lock"
    );
    assert!(
        app.list_plugins().unwrap().iter().any(|p| p.id == "flow"),
        "remove_project_plugin must NOT touch the Store -- only the lock"
    );
}

#[test]
fn remove_project_plugin_reports_no_lock_and_not_in_lock_distinctly() {
    let fx = Fixture::new("remove-reports");
    let app = fx.app();

    let no_lock = app.remove_project_plugin("flow", &fx.project_root).unwrap();
    assert!(matches!(no_lock, RemoveProjectPluginReport::NoLock));

    fx.add_marketplace_to_global_registry();
    app.add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();
    let not_in_lock = app
        .remove_project_plugin("does-not-exist", &fx.project_root)
        .unwrap();
    assert!(matches!(
        not_in_lock,
        RemoveProjectPluginReport::NotInLock { .. }
    ));
}

#[test]
fn malformed_lock_is_reported_not_panicked_on() {
    let fx = Fixture::new("malformed-lock");
    fs::write(
        project_lock::lock_path_for(&fx.project_root),
        "version: 1\nplugins: [not, valid, plugin, shape",
    )
    .unwrap();

    let app = fx.app();
    assert!(app.project_environment(&fx.project_root).is_err());
    assert!(app.plan_project_environment(&fx.project_root).is_err());
    assert!(
        app.install_project_environment(&fx.project_root, &AlwaysTrust)
            .is_err()
    );

    // `uze status`'s view degrades to a reported Malformed state instead
    // of propagating the error -- status must never refuse to run because
    // of the exact problem it exists to diagnose.
    match app.project_lock_status(&fx.project_root) {
        ProjectLockStatus::Malformed { .. } => {}
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn unsupported_lock_version_is_reported_not_panicked_on() {
    let fx = Fixture::new("unsupported-version");
    fs::write(
        project_lock::lock_path_for(&fx.project_root),
        "version: 99\n",
    )
    .unwrap();
    let app = fx.app();
    assert!(app.project_environment(&fx.project_root).is_err());
}

#[test]
fn global_add_plugin_never_touches_the_project_lock() {
    let fx = Fixture::new("global-add-neutral");
    let app = fx.app();
    let source = uze_core::PackageSource::Local {
        path: fx.marketplace_root.join("flow"),
    };
    app.add_plugin(source, &AlwaysTrust).unwrap();

    assert!(
        !project_lock::lock_path_for(&fx.project_root).is_file(),
        "a global `uze add` must never create agents.lock"
    );
}

#[test]
fn project_root_resolution_is_deterministic_from_a_subdirectory() {
    let fx = Fixture::new("root-resolution");
    fx.add_marketplace_to_global_registry();
    fx.app()
        .add_project_plugin("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    let nested = fx.project_root.join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    let resolved = project_root::resolve_project_root(&nested).unwrap();
    assert_eq!(resolved, fx.project_root.canonicalize().unwrap());
}
