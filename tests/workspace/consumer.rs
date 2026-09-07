//! Integration tests for the Project Agent Environment (`agents.lock`).
//!
//! See openspec/changes/project-agent-environment/tasks.md §6 for the
//! scenario list this addresses; it does not attempt every item there
//! (offline-availability and executable-trust-denial scenarios need
//! fixtures/mechanisms this pass does not add — see that file for what
//! remains deliberately deferred).

use std::{fs, path::PathBuf};

use uze_application::{
    UzeApplication,
    application::{InstallReport, ProjectLockStatus, RemoveProjectPluginReport},
};
use uze_core::{UzeHome, project_lock, project_root, trust::AlwaysTrust};

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
    // A marketplace is a Git repository: its commits are what a lock pins
    // and what an update is measured against.
    uze_testkit::git::commit_everything_in(root);
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

    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
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
    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();
    let second_bytes = fs::read(&lock_path).unwrap();
    assert_eq!(
        first_bytes, second_bytes,
        "repeated add must not change the lock"
    );
}

/// A marketplace on this machine is a clone of a repository, not a loose
/// directory, so it pins exactly as well as a remote one.
#[test]
fn a_marketplace_on_this_machine_pins_like_any_other() {
    let fx = Fixture::new("local-no-pin");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();
    let lock = project_lock::load_lock(&fx.project_root).unwrap().unwrap();
    assert!(
        !lock.marketplaces["test-market"].revision.is_empty(),
        "a marketplace on this machine is a clone at a commit, and pins like any other"
    );
    assert!(lock.plugins["flow"].integrity.is_some());
    assert_eq!(lock.plugins["flow"].marketplace, "test-market");
}

#[test]
fn install_project_environment_reproduces_a_lock_on_a_fresh_machine() {
    let fx = Fixture::new("fresh-machine");
    fx.add_marketplace_to_global_registry();

    // "Machine A": adds the plugin, produces agents.lock.
    fx.app()
        .project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    // "Machine B": same UZE_HOME layout path but a fresh Store directory,
    // simulating `git clone` + `uze install` with no prior `uze add`. This
    // machine deliberately never calls `add_marketplace_to_global_registry`
    // — that is the whole point: `resolve_locked_plugin_source` reads the
    // marketplace source straight from the lock to *acquire* the plugin,
    // regardless of the global registry's state, but `market list` must
    // still end up knowing about it afterward (see the assertion below) —
    // a fresh machine that only ever ran `uze install` must be able to
    // `uze market inspect test-market` same as one that ran `market add`
    // by hand first.
    let fresh_home = fx.uze_home.parent().unwrap().join("home-fresh-machine");
    let fresh_app = UzeApplication::new(UzeHome::at(&fresh_home), Vec::new());
    assert!(
        fresh_app.plugins().list().unwrap().is_empty(),
        "fresh machine starts with nothing installed"
    );
    assert!(
        fresh_app.marketplace().list().unwrap().len() == 1,
        "fresh machine starts with only the embedded uze-official marketplace"
    );

    let report = fresh_app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();
    match report {
        InstallReport::Installed { plugins, .. } => {
            assert_eq!(plugins, vec!["flow".to_owned()]);
        }
        other => panic!("expected Installed, got {other:?}"),
    }
    assert!(
        fresh_app
            .plugins()
            .list()
            .unwrap()
            .iter()
            .any(|p| p.id == "flow@test-market"),
        "install_project_environment must actually install the missing plugin"
    );
    assert!(
        fresh_app
            .marketplace()
            .list()
            .unwrap()
            .iter()
            .any(|m| m.name == "test-market"),
        "install_project_environment must register the locked marketplace globally too — \
         otherwise the plugin is installed but orphaned from `market list`/`market inspect`, \
         and every UI that cross-references the marketplace catalog (Marketplace tab, Plugins \
         tab badges/descriptions) shows it as unrecognized"
    );

    // `project_lock_status` must recognize the plugin as installed by its
    // marketplace-qualified id ("flow@test-market"), the same id
    // `installed_plugin_ids`/`missing_locked_plugins` compare against —
    // never by the lock's bare plugin-name key ("flow"), which never
    // matches a real Store id and would make `uze status` report a
    // marketplace-sourced plugin as permanently missing even right after a
    // successful install.
    match fresh_app.project().lock_status(&fx.project_root) {
        ProjectLockStatus::Present { plugins } => {
            assert!(
                plugins.iter().any(|p| p.plugin == "flow" && p.installed),
                "expected `flow` to be reported installed, got {plugins:?}"
            );
        }
        other => panic!("expected Present, got {other:?}"),
    }
}

#[test]
fn install_project_environment_is_a_no_op_once_everything_is_installed() {
    let fx = Fixture::new("no-changes");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    // `add` declared the policy scaffold, so the first install still owes a
    // projection; what this test is about is the run after that one.
    app.project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();
    let report = app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();
    assert!(
        matches!(report, InstallReport::NoChanges),
        "expected NoChanges, got {report:?}"
    );
}

#[test]
fn install_project_environment_with_no_lock_installs_nothing_and_settles() {
    let fx = Fixture::new("no-lock");
    let app = fx.app();
    // The first run is not nothing: `install` creates the manifest a person
    // edits, and a manifest that declares a policy is owed a projection.
    let report = app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();
    match report {
        InstallReport::Installed { plugins, .. } => assert!(plugins.is_empty()),
        InstallReport::NoChanges => {}
    }
    // The second has nothing left to do, which is the property that
    // actually matters: installing twice is installing once.
    let report = app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();
    assert!(
        matches!(report, InstallReport::NoChanges),
        "expected NoChanges, got {report:?}"
    );
}

/// The path a person actually takes: write `agents.yaml` by hand (or clone
/// a repository carrying one), then `uze install`. There is no lock yet —
/// the lock is what installing *produces*, so a declaration nothing has
/// resolved must be resolved here rather than reported as up to date.
#[test]
fn install_resolves_a_declaration_the_lock_has_never_seen() {
    let fx = Fixture::new("install-from-manifest");
    fs::write(
        fx.project_root.join("agents.yaml"),
        format!(
            "marketplaces:\n  test-market:\n    path: {}\n    plugins:\n      - flow\n",
            fx.marketplace_root.display()
        ),
    )
    .unwrap();
    let app = fx.app();

    let report = app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();

    match report {
        InstallReport::Installed { plugins, .. } => assert_eq!(plugins, vec!["flow".to_owned()]),
        other => panic!("expected Installed, got {other:?}"),
    }
    assert!(
        app.plugins()
            .list()
            .unwrap()
            .iter()
            .any(|p| p.id == "flow@test-market"),
        "the declared plugin must reach the Store, not only the lock"
    );
    let lock = project_lock::load_lock(&fx.project_root)
        .unwrap()
        .expect("install must write the lock its resolution produced");
    assert!(lock.plugins.contains_key("flow"));
    assert!(lock.marketplaces.contains_key("test-market"));
    assert!(
        app.marketplace()
            .list()
            .unwrap()
            .iter()
            .any(|m| m.name == "test-market"),
        "a marketplace the manifest declares must be registered globally too"
    );

    // Second run: the lock now answers the declaration, so there is
    // nothing left to resolve and nothing left to install.
    let report = app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .unwrap();
    assert!(
        matches!(report, InstallReport::NoChanges),
        "expected NoChanges, got {report:?}"
    );
}

/// Moving a plugin to another marketplace in `agents.yaml` is heard —
/// `install` re-resolves it rather than treating the lock's older answer
/// as current — and stops on the machine-level name collision two
/// same-named plugins are, naming both. Silence is what this pins
/// against: the old entry must not be left standing as if the edit never
/// happened. Which of the two wins is a decision no command makes today
/// (`uze remove flow` first is the way through).
#[test]
fn install_re_resolves_a_plugin_the_manifest_moved_and_names_the_collision() {
    let fx = Fixture::new("install-moved-marketplace");
    let mirror_root = fx.uze_home.parent().unwrap().join("mirror");
    write_marketplace(&mirror_root, "mirror-market", "flow");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    fs::write(
        fx.project_root.join("agents.yaml"),
        format!(
            "marketplaces:\n  mirror-market:\n    path: {}\n    plugins:\n      - flow\n",
            mirror_root.display()
        ),
    )
    .unwrap();

    let error = app
        .project()
        .install(&fx.project_root, &AlwaysTrust)
        .expect_err("a declaration the Store cannot satisfy must be reported, not passed over");
    let reported = error.to_string();
    assert!(
        reported.contains("test-market") && reported.contains("mirror-market"),
        "the collision must name both marketplaces: {reported}"
    );
}

/// A marketplace in a Git repository, with one plugin whose skill carries
/// `body` — the text a test changes to prove which revision was read.
fn write_git_marketplace(repository: &uze_testkit::git::Repository, body: &str) -> String {
    let root = repository.root();
    fs::write(
        root.join("marketplace.json"),
        r#"{"name": "git-market", "plugins": [{"name": "flow", "source": "flow"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("flow/skills/uze-e2e")).unwrap();
    fs::write(root.join("flow/plugin.json"), r#"{"name": "flow"}"#).unwrap();
    fs::write(root.join("flow/skills/uze-e2e/SKILL.md"), body).unwrap();
    repository.git(&["add", "-A"]);
    repository.git(&["commit", "-q", "-m", "marketplace"]);
    repository.git(&["rev-parse", "HEAD"]).trim().to_owned()
}

fn git_marketplace_source(repository: &uze_testkit::git::Repository) -> uze_core::PackageSource {
    uze_core::PackageSource::Git {
        // `file://` because a bare path is a local clone, and this must
        // exercise the same code path a real remote takes.
        url: format!("file://{}", repository.root().display()),
        reference: None,
        subdirectory: None,
    }
}

/// Every skill body under a Store, so a test can say which revision's
/// bytes actually landed.
fn installed_skill_bodies(home: &std::path::Path) -> Vec<String> {
    fn walk(directory: &std::path::Path, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.file_name().is_some_and(|name| name == "SKILL.md") {
                found.push(fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut found = Vec::new();
    walk(&UzeHome::at(home).plugins_dir(), &mut found);
    found
}

/// What a lock is *for*. A local path pins nothing by design — a directory
/// is mutable — but a Git marketplace resolves to an immutable commit, and
/// the entry must carry it along with a digest of the bytes that landed.
#[test]
fn a_plugin_from_a_git_marketplace_is_pinned_to_a_commit_and_a_digest() {
    let market = uze_testkit::git::Repository::new("git-market-pin");
    let commit = write_git_marketplace(&market, "# first\n");

    let base = uze_testkit::temp::scratch("git-market-pin-base");
    let (home, project) = (base.join("home"), base.join("project"));
    fs::create_dir_all(&project).unwrap();
    uze_core::state::marketplace_add(
        &UzeHome::at(&home),
        "git-market",
        git_marketplace_source(&market),
    )
    .unwrap();

    UzeApplication::new(UzeHome::at(&home), Vec::new())
        .project()
        .add("flow", "git-market", &project, &AlwaysTrust)
        .unwrap();

    let lock = project_lock::load_lock(&project).unwrap().unwrap();
    assert_eq!(
        lock.marketplaces["git-market"].revision, commit,
        "the marketplace entry must record the commit it was read at"
    );
    let integrity = lock.plugins["flow"]
        .integrity
        .as_deref()
        .expect("immutable bytes must record a digest");
    assert!(integrity.starts_with("sha256:"), "{integrity}");
}

/// The property the file exists to hold: a second machine installs what
/// the lock recorded, not what the branch points at by then.
#[test]
fn reproduction_reads_the_locked_commit_after_the_marketplace_moved() {
    let market = uze_testkit::git::Repository::new("git-market-moved");
    let locked_commit = write_git_marketplace(&market, "# first\n");

    let base = uze_testkit::temp::scratch("git-market-moved-base");
    let (home, project) = (base.join("home"), base.join("project"));
    fs::create_dir_all(&project).unwrap();
    uze_core::state::marketplace_add(
        &UzeHome::at(&home),
        "git-market",
        git_marketplace_source(&market),
    )
    .unwrap();
    UzeApplication::new(UzeHome::at(&home), Vec::new())
        .project()
        .add("flow", "git-market", &project, &AlwaysTrust)
        .unwrap();

    // The marketplace moves on, exactly as a shared repository does.
    let moved_commit = write_git_marketplace(&market, "# second\n");
    assert_ne!(locked_commit, moved_commit);

    let fresh_home = base.join("home-fresh");
    UzeApplication::new(UzeHome::at(&fresh_home), Vec::new())
        .project()
        .install(&project, &AlwaysTrust)
        .unwrap();

    assert_eq!(
        installed_skill_bodies(&fresh_home),
        vec!["# first\n".to_owned()],
        "reproduction must read the commit the lock pinned, not the branch tip"
    );
}

/// The integrity field is checked, not decorative: bytes that do not match
/// what the lock pinned stop before anything is ingested.
#[test]
fn reproduction_refuses_bytes_that_are_not_the_bytes_the_lock_pinned() {
    let market = uze_testkit::git::Repository::new("git-market-tampered");
    write_git_marketplace(&market, "# first\n");

    let base = uze_testkit::temp::scratch("git-market-tampered-base");
    let (home, project) = (base.join("home"), base.join("project"));
    fs::create_dir_all(&project).unwrap();
    uze_core::state::marketplace_add(
        &UzeHome::at(&home),
        "git-market",
        git_marketplace_source(&market),
    )
    .unwrap();
    UzeApplication::new(UzeHome::at(&home), Vec::new())
        .project()
        .add("flow", "git-market", &project, &AlwaysTrust)
        .unwrap();

    // A lock claiming a digest the marketplace's bytes do not have is the
    // same shape as a substituted remote or a rewritten history.
    let mut lock = project_lock::load_lock(&project).unwrap().unwrap();
    lock.plugins.get_mut("flow").unwrap().integrity =
        Some("sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned());
    project_lock::save_lock(&project, &lock).unwrap();

    let fresh_home = base.join("home-fresh");
    let error = UzeApplication::new(UzeHome::at(&fresh_home), Vec::new())
        .project()
        .install(&project, &AlwaysTrust)
        .expect_err("a digest that does not match must stop the install");
    assert!(
        matches!(error, uze_core::UzeError::IntegrityMismatch { .. }),
        "{error:?}"
    );
    assert!(
        installed_skill_bodies(&fresh_home).is_empty(),
        "nothing may be ingested once the pin failed"
    );
}

/// The marketplace built into UZE is not a project's to declare: its
/// plugins are installed for every project by the machine's own bootstrap.
/// `declare_plugin` already refuses to write one into `agents.yaml`, and
/// the lock refuses it for the same reason — an entry recording something
/// nobody declared is a line nobody can act on.
#[test]
fn adding_a_built_in_plugin_installs_it_without_writing_the_project_files() {
    let fx = Fixture::new("built-in-not-declared");
    let app = fx.app();

    app.project()
        .add("uze", "uze-official", &fx.project_root, &AlwaysTrust)
        .unwrap();

    assert!(
        app.plugins()
            .list()
            .unwrap()
            .iter()
            .any(|plugin| plugin.id == "uze@uze-official"),
        "the plugin is installed on the machine"
    );
    assert!(
        !project_lock::lock_path_for(&fx.project_root).exists(),
        "nothing the project declares changed, so it has nothing to lock"
    );
    assert!(
        !fx.project_root.join("agents.yaml").exists(),
        "and nothing to declare"
    );
}

#[test]
fn remove_project_plugin_removes_from_lock_but_not_from_the_store() {
    let fx = Fixture::new("remove-lock-only");
    fx.add_marketplace_to_global_registry();
    let app = fx.app();
    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    let report = app.project().remove("flow", &fx.project_root).unwrap();
    assert!(matches!(report, RemoveProjectPluginReport::Removed { .. }));

    let lock = project_lock::load_lock(&fx.project_root).unwrap().unwrap();
    assert!(
        !lock.plugins.contains_key("flow"),
        "removed plugin must be gone from the lock"
    );
    assert!(
        app.plugins()
            .list()
            .unwrap()
            .iter()
            .any(|p| p.id == "flow@test-market"),
        "remove_project_plugin must NOT touch the Store -- only the lock"
    );
}

#[test]
fn remove_project_plugin_reports_no_lock_and_not_in_lock_distinctly() {
    let fx = Fixture::new("remove-reports");
    let app = fx.app();

    let no_lock = app.project().remove("flow", &fx.project_root).unwrap();
    assert!(matches!(no_lock, RemoveProjectPluginReport::NoLock));

    fx.add_marketplace_to_global_registry();
    app.project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();
    let not_in_lock = app
        .project()
        .remove("does-not-exist", &fx.project_root)
        .unwrap();
    assert!(matches!(
        not_in_lock,
        RemoveProjectPluginReport::NotInLock { .. }
    ));
}

#[test]
fn same_named_plugins_from_two_marketplaces_coexist_and_require_qualified_lookup() {
    // ADR-036's Store layout (bytes/registrations coexist per marketplace)
    // is unchanged; ADR-038 adds that only one of them may be *active*
    // under the bare name at a time. Plain `plugin_install` refuses the
    // second one; resolving with an explicit alias lets both coexist.
    let base = temp("same-name-marketplaces");
    let home = UzeHome::at(base.join("home"));
    let first = base.join("first-market");
    let second = base.join("second-market");
    write_marketplace(&first, "first", "flow");
    write_marketplace(&second, "second", "flow");
    uze_core::state::marketplace_add(
        &home,
        "first",
        uze_core::PackageSource::Local { path: first },
    )
    .unwrap();
    uze_core::state::marketplace_add(
        &home,
        "second",
        uze_core::PackageSource::Local { path: second },
    )
    .unwrap();
    let app = UzeApplication::new(home.clone(), Vec::new());

    app.marketplace()
        .install_plugin("flow@first", &AlwaysTrust)
        .unwrap();
    let collision = app
        .marketplace()
        .install_plugin("flow@second", &AlwaysTrust)
        .unwrap_err();
    assert!(matches!(
        collision,
        uze_core::UzeError::PluginNameCollision { existing, requested, .. }
            if existing == "flow@first" && requested == "flow@second"
    ));
    // Refused: only `flow@first` is installed.
    assert_eq!(
        app.plugins()
            .list()
            .unwrap()
            .into_iter()
            .map(|plugin| plugin.id)
            .collect::<Vec<_>>(),
        vec!["flow@first"]
    );

    app.marketplace()
        .install_plugin_resolving(
            "flow@second",
            &AlwaysTrust,
            &uze_core::naming::FixedResolution(uze_core::naming::NameCollisionResolution::Alias(
                "flow-second".to_owned(),
            )),
        )
        .unwrap();

    let ids: Vec<_> = app
        .plugins()
        .list()
        .unwrap()
        .into_iter()
        .map(|plugin| plugin.id)
        .collect();
    assert_eq!(ids, vec!["flow@first", "flow@second"]);
    assert!(
        home.plugin_dir(
            &uze_core::PackageId::from_marketplace_plugin(
                "first",
                "flow",
                std::path::Path::new("plugin.json"),
            )
            .unwrap()
        )
        .is_dir()
    );
    // Once resolved, at most one package ever answers to a bare name at all
    // (ADR-038) — `flow` now unambiguously means "whichever is active under
    // it", never the old "installed from multiple marketplaces" refusal.
    // The aliased one is addressable the same way, by its own active name.
    assert!(matches!(
        app.plugins().remove("flow-second"),
        Ok(uze_application::application::RemovePluginReport::Removed { .. })
    ));
    assert_eq!(app.plugins().list().unwrap()[0].id, "flow@first");
    assert!(matches!(
        app.plugins().remove("flow"),
        Ok(uze_application::application::RemovePluginReport::Removed { .. })
    ));
    assert!(app.plugins().list().unwrap().is_empty());
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
    assert!(app.project().environment(&fx.project_root).is_err());
    assert!(app.project().plan(&fx.project_root).is_err());
    assert!(
        app.project()
            .install(&fx.project_root, &AlwaysTrust)
            .is_err()
    );

    // `uze status`'s view degrades to a reported Malformed state instead
    // of propagating the error -- status must never refuse to run because
    // of the exact problem it exists to diagnose.
    match app.project().lock_status(&fx.project_root) {
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
    assert!(app.project().environment(&fx.project_root).is_err());
}

#[test]
fn global_add_plugin_never_touches_the_project_lock() {
    let fx = Fixture::new("global-add-neutral");
    let app = fx.app();
    let source = uze_core::PackageSource::Local {
        path: fx.marketplace_root.join("flow"),
    };
    app.plugins().add(source, &AlwaysTrust).unwrap();

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
        .project()
        .add("flow", "test-market", &fx.project_root, &AlwaysTrust)
        .unwrap();

    let nested = fx.project_root.join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    let resolved = project_root::resolve_project_root(&nested).unwrap();
    assert_eq!(resolved, fx.project_root.canonicalize().unwrap());
}

/// Drift along the chain a project's environment passes through, and what
/// converging it does. Every assertion reads the two documents, the Store
/// and the projected file — never UZE's own summary of them.
mod drift {
    use super::*;

    fn declaring(fx: &Fixture, plugins: &[&str]) {
        let mut text = format!(
            "marketplaces:\n  test-market:\n    path: {}\n",
            fx.marketplace_root.display()
        );
        if !plugins.is_empty() {
            text.push_str("    plugins:\n");
            for plugin in plugins {
                text.push_str(&format!("      - {plugin}\n"));
            }
        }
        fs::write(fx.project_root.join("agents.yaml"), text).unwrap();
    }

    /// The edit a person just made is the one thing a plan founded on the
    /// lock could never see.
    #[test]
    fn a_manifest_edit_is_visible_to_the_plan_before_anything_is_installed() {
        let fx = Fixture::new("drift-unresolved");
        declaring(&fx, &["flow"]);
        let app = fx.app();

        let plan = app.project().plan(&fx.project_root).unwrap();

        assert_eq!(plan.unresolved, vec!["flow".to_owned()]);
        assert!(plan.has_changes);
        assert!(
            project_lock::load_lock(&fx.project_root).unwrap().is_none(),
            "planning resolves nothing and writes nothing"
        );
    }

    #[test]
    fn a_plugin_the_manifest_no_longer_declares_reads_as_surplus() {
        let fx = Fixture::new("drift-surplus");
        declaring(&fx, &["flow"]);
        let app = fx.app();
        app.project()
            .install(&fx.project_root, &AlwaysTrust)
            .unwrap();

        declaring(&fx, &[]);
        let plan = app.project().plan(&fx.project_root).unwrap();

        assert_eq!(plan.surplus, vec!["flow".to_owned()]);
        assert!(
            project_lock::load_lock(&fx.project_root)
                .unwrap()
                .is_some_and(|lock| lock.plugins.contains_key("flow")),
            "the plan reports; it does not remove"
        );
    }

    /// Converging the lock is the least destructive thing an install does,
    /// not the most: the lock is derived, and what it loses is what the
    /// person just deleted from the file they author.
    ///
    /// What is *not* touched is the machine. The Store keeps the package
    /// and every harness keeps reading it, because other projects share
    /// both — taking it off this machine is `uze plugin remove`, which is
    /// a different scope by ADR-019.
    #[test]
    fn install_converges_the_lock_and_leaves_the_machine_alone() {
        let fx = Fixture::new("drift-converge");
        declaring(&fx, &["flow"]);
        let app = fx.app();
        app.project()
            .install(&fx.project_root, &AlwaysTrust)
            .unwrap();
        let installed_before = app.plugins().list().unwrap().len();

        declaring(&fx, &[]);
        let report = app
            .project()
            .install(&fx.project_root, &AlwaysTrust)
            .unwrap();

        match report {
            InstallReport::Installed { removed, .. } => {
                assert_eq!(removed, vec!["flow".to_owned()])
            }
            other => panic!("expected the removal to be reported, got {other:?}"),
        }
        assert!(
            project_lock::load_lock(&fx.project_root)
                .unwrap()
                .is_none_or(|lock| !lock.plugins.contains_key("flow")),
            "the lock no longer answers for a plugin nobody declares"
        );
        assert_eq!(
            app.plugins().list().unwrap().len(),
            installed_before,
            "and the machine's Store is untouched — other projects share it"
        );
        assert!(
            app.project()
                .plan(&fx.project_root)
                .unwrap()
                .surplus
                .is_empty(),
            "with the drift cleared"
        );
    }

    /// The policy hole: `worktrees:` is read live wherever UZE acts on it,
    /// but the agents that must honour it read the projected text.
    #[test]
    fn a_policy_change_reads_as_a_stale_projection_until_install_clears_it() {
        let fx = Fixture::new("drift-projection");
        let manifest = fx.project_root.join("agents.yaml");
        fs::write(&manifest, "worktrees:\n  completion: handoff\n").unwrap();
        let app = fx.app();
        app.context().reconcile(&fx.project_root).unwrap();
        assert!(
            app.project()
                .plan(&fx.project_root)
                .unwrap()
                .stale_projection
                .is_none(),
            "a freshly reconciled project is not stale"
        );

        fs::write(&manifest, "worktrees:\n  completion: merge\n").unwrap();

        let stale = app
            .project()
            .plan(&fx.project_root)
            .unwrap()
            .stale_projection;
        assert!(
            stale.is_some_and(|stale| stale.declared == "merge"),
            "agents are still reading the previous instruction"
        );
        let projected = fs::read_to_string(fx.project_root.join("AGENTS.md")).unwrap();
        assert!(
            !projected.contains("fast-forwards the target"),
            "the file on disk still carries the old clause"
        );

        app.project()
            .install(&fx.project_root, &AlwaysTrust)
            .unwrap();

        assert!(
            app.project()
                .plan(&fx.project_root)
                .unwrap()
                .stale_projection
                .is_none(),
            "one install later, the projection has caught up"
        );
        let projected = fs::read_to_string(fx.project_root.join("AGENTS.md")).unwrap();
        assert!(
            projected.contains("fast-forwards the target"),
            "and the file carries the new policy's own words: {projected}"
        );
    }
}
