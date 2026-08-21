//! L1 contract for Git acquisition, against **local bare repositories only**.
//!
//! No network, no host, no GitHub. That is not a convenience: the mechanism
//! under test is Git, and a test that reached a hosting provider would be
//! testing that provider's availability instead. A networked smoke test
//! against a real remote belongs in a separate, non-gating tier.

#![cfg(unix)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{PackageSource, ResolvedSource, UzeError, UzeHome, UzeStore, acquisition::acquire};

fn temporary(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is available")
        .as_nanos();
    std::env::temp_dir().join(format!("uze-git-{label}-{}-{nonce}", std::process::id()))
}

fn git(arguments: &[&str], directory: &Path) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "uze")
        .env("GIT_AUTHOR_EMAIL", "uze@example.invalid")
        .env("GIT_COMMITTER_NAME", "uze")
        .env("GIT_COMMITTER_EMAIL", "uze@example.invalid")
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("git {arguments:?}: {error}"));
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write_package(root: &Path, name: &str, with_mcp: bool) {
    fs::create_dir_all(root.join("skills/example")).unwrap();
    fs::write(
        root.join("plugin.json"),
        format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
    )
    .unwrap();
    fs::write(
        root.join("skills/example/SKILL.md"),
        "---\nname: example\ndescription: fixture\n---\n\nbody\n",
    )
    .unwrap();
    if with_mcp {
        fs::write(
            root.join("mcp.json"),
            r#"{"mcpServers":{"server":{"command":"./bin/server","args":["--stdio"]}}}"#,
        )
        .unwrap();
    }
}

/// A working tree plus the bare repository UZE will clone from. Returns the
/// bare repo's `file://` URL.
struct Fixture {
    root: PathBuf,
    work: PathBuf,
    url: String,
}

impl Fixture {
    fn new(label: &str, with_mcp: bool) -> Self {
        Self::with_layout(label, |root| write_package(root, "git-fixture", with_mcp))
    }

    fn with_layout(label: &str, layout: impl FnOnce(&Path)) -> Self {
        let root = temporary(label);
        let work = root.join("work");
        let bare = root.join("origin.git");
        fs::create_dir_all(&work).unwrap();
        git(
            &["init", "--quiet", "--initial-branch", "trunk", "."],
            &work,
        );
        layout(&work);
        git(&["add", "-A"], &work);
        git(&["commit", "--quiet", "-m", "initial"], &work);
        fs::create_dir_all(&bare).unwrap();
        git(&["init", "--quiet", "--bare", "."], &bare);
        git(&["remote", "add", "origin", &bare.to_string_lossy()], &work);
        git(&["push", "--quiet", "origin", "trunk"], &work);
        // A real remote advertises its default branch. Setting it here keeps
        // the fixture honest rather than exercising a misconfigured remote.
        git(&["symbolic-ref", "HEAD", "refs/heads/trunk"], &bare);
        let url = format!("file://{}", bare.display());
        Self { root, work, url }
    }

    fn commit_on(&self, branch: &str, body: &str) -> String {
        git(&["checkout", "--quiet", "-B", branch], &self.work);
        fs::write(self.work.join("skills/example/SKILL.md"), body).unwrap();
        git(&["add", "-A"], &self.work);
        git(&["commit", "--quiet", "-m", "update"], &self.work);
        git(
            &["push", "--quiet", "--force", "origin", branch],
            &self.work,
        );
        git(&["rev-parse", "HEAD"], &self.work)
    }

    fn tag(&self, name: &str) -> String {
        git(&["tag", name], &self.work);
        git(&["push", "--quiet", "origin", name], &self.work);
        git(&["rev-parse", name], &self.work)
    }

    fn head(&self) -> String {
        git(&["rev-parse", "HEAD"], &self.work)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn resolved_commit(resolved: &ResolvedSource) -> String {
    match resolved {
        ResolvedSource::Git { commit, .. } => commit.clone(),
        other => panic!("expected a Git resolution, got {other:?}"),
    }
}

#[test]
fn a_local_repository_is_acquired_and_resolves_to_a_commit() {
    let fixture = Fixture::new("basic", false);
    let materialized = acquire(&PackageSource::git(&fixture.url)).expect("acquisition succeeds");

    assert!(materialized.root().join("plugin.json").is_file());
    assert_eq!(
        resolved_commit(&materialized.provenance().resolved),
        fixture.head()
    );
    // The repository's own metadata is not package content.
    assert!(!materialized.root().join(".git").exists());
}

/// No ref means the repository's own default branch — resolved from the
/// remote, never a hardcoded `main`/`master`. This fixture's default is
/// `trunk` precisely so a hardcoded guess would fail.
#[test]
fn an_unspecified_reference_resolves_the_repositorys_own_default_branch() {
    let fixture = Fixture::new("default-branch", false);
    let materialized = acquire(&PackageSource::git(&fixture.url)).unwrap();
    assert_eq!(
        resolved_commit(&materialized.provenance().resolved),
        fixture.head()
    );
}

#[test]
fn a_branch_resolves_to_an_immutable_commit() {
    let fixture = Fixture::new("branch", false);
    let commit = fixture.commit_on("feature", "---\nname: example\ndescription: f\n---\n\nv2\n");

    let materialized = acquire(&PackageSource::Git {
        url: fixture.url.clone(),
        reference: Some("feature".to_owned()),
        subdirectory: None,
    })
    .unwrap();

    assert_eq!(resolved_commit(&materialized.provenance().resolved), commit);
    // The request is preserved as asked; only the resolution is pinned.
    assert!(matches!(
        &materialized.provenance().requested,
        PackageSource::Git { reference: Some(reference), .. } if reference == "feature"
    ));
}

#[test]
fn a_tag_resolves_to_an_immutable_commit() {
    let fixture = Fixture::new("tag", false);
    let commit = fixture.tag("v1.0.0");

    let materialized = acquire(&PackageSource::Git {
        url: fixture.url.clone(),
        reference: Some("v1.0.0".to_owned()),
        subdirectory: None,
    })
    .unwrap();

    assert_eq!(resolved_commit(&materialized.provenance().resolved), commit);
}

/// An explicit commit must work, which is why the clone is not shallow: a
/// `--depth 1` fetch cannot check out an arbitrary pinned revision.
#[test]
fn an_explicit_commit_is_checked_out() {
    let fixture = Fixture::new("commit", false);
    let first = fixture.head();
    fixture.commit_on(
        "trunk",
        "---\nname: example\ndescription: f\n---\n\nmoved\n",
    );

    let materialized = acquire(&PackageSource::Git {
        url: fixture.url.clone(),
        reference: Some(first.clone()),
        subdirectory: None,
    })
    .unwrap();

    assert_eq!(resolved_commit(&materialized.provenance().resolved), first);
}

/// The reproducibility contract: reinstalling from the *resolved* source
/// returns the same bytes even after the branch has moved on.
#[test]
fn reinstalling_a_resolved_commit_stays_at_that_commit() {
    let fixture = Fixture::new("reinstall", false);
    let pinned = acquire(&PackageSource::git(&fixture.url)).unwrap();
    let pinned_commit = resolved_commit(&pinned.provenance().resolved);
    drop(pinned);

    let moved = fixture.commit_on(
        "trunk",
        "---\nname: example\ndescription: f\n---\n\nmoved\n",
    );
    assert_ne!(moved, pinned_commit);

    // Reinstall asks for the resolution, not the request.
    let again = acquire(&PackageSource::Git {
        url: fixture.url.clone(),
        reference: Some(pinned_commit.clone()),
        subdirectory: None,
    })
    .unwrap();
    assert_eq!(
        resolved_commit(&again.provenance().resolved),
        pinned_commit,
        "reinstall drifted off the resolved commit"
    );
}

/// The other half of that contract: updating asks the *request* again, so a
/// branch that moved produces a new commit.
#[test]
fn updating_re_resolves_the_request_and_moves_with_the_branch() {
    let fixture = Fixture::new("update", false);
    let request = PackageSource::Git {
        url: fixture.url.clone(),
        reference: Some("trunk".to_owned()),
        subdirectory: None,
    };
    let before = resolved_commit(&acquire(&request).unwrap().provenance().resolved);

    let moved = fixture.commit_on(
        "trunk",
        "---\nname: example\ndescription: f\n---\n\nmoved\n",
    );
    let after = resolved_commit(&acquire(&request).unwrap().provenance().resolved);

    assert_ne!(before, after);
    assert_eq!(after, moved);
}

#[test]
fn a_subdirectory_selects_the_package_root() {
    let fixture = Fixture::with_layout("subdir", |root| {
        fs::create_dir_all(root.join("packages/inner")).unwrap();
        write_package(&root.join("packages/inner"), "inner-package", false);
        fs::write(root.join("README.md"), "monorepo").unwrap();
    });

    let materialized = acquire(&PackageSource::Git {
        url: fixture.url.clone(),
        reference: None,
        subdirectory: Some(PathBuf::from("packages/inner")),
    })
    .unwrap();

    assert!(materialized.root().ends_with("packages/inner"));
    assert!(materialized.root().join("plugin.json").is_file());
}

#[test]
fn a_subdirectory_escaping_the_repository_is_rejected() {
    let fixture = Fixture::new("subdir-escape", false);
    for escape in ["../outside", "/etc", "packages/../../outside"] {
        let result = acquire(&PackageSource::Git {
            url: fixture.url.clone(),
            reference: None,
            subdirectory: Some(PathBuf::from(escape)),
        });
        assert!(
            matches!(result, Err(UzeError::PackageEscapesRoot { .. })),
            "accepted subdirectory `{escape}`"
        );
    }
}

#[test]
fn a_missing_subdirectory_is_rejected() {
    let fixture = Fixture::new("subdir-missing", false);
    assert!(matches!(
        acquire(&PackageSource::Git {
            url: fixture.url.clone(),
            reference: None,
            subdirectory: Some(PathBuf::from("packages/absent")),
        }),
        Err(UzeError::MissingPath(_))
    ));
}

/// A URL carrying a secret is refused outright rather than sanitized, so the
/// secret never enters UZE — not its memory, its registry, or its errors.
#[test]
fn a_credential_bearing_url_is_rejected_and_never_echoed() {
    let result = acquire(&PackageSource::git(
        "https://user:hunter2@example.invalid/repo.git",
    ));
    let error = result.expect_err("a credential-bearing URL was accepted");
    assert!(matches!(error, UzeError::CredentialBearingUrl));
    assert!(
        !error.to_string().contains("hunter2"),
        "the secret appeared in an error message"
    );
}

/// The scratch checkout is UZE's, and it must not survive the operation.
#[test]
fn the_materialized_checkout_is_removed_when_dropped() {
    let fixture = Fixture::new("cleanup", false);
    let checkout = {
        let materialized = acquire(&PackageSource::git(&fixture.url)).unwrap();
        materialized.root().to_path_buf()
    };
    assert!(
        !checkout.exists(),
        "a scratch checkout outlived its MaterializedPackage"
    );
}

/// End to end: a cloned repository becomes an ordinary installed package, and
/// the Store records both what was asked for and what it resolved to.
#[test]
fn an_acquired_repository_ingests_into_the_store_with_both_sources_recorded() {
    let fixture = Fixture::new("ingest", false);
    let home = UzeHome::at(fixture.root.join("uze-home"));
    let store = UzeStore::new(home.clone());

    let materialized = acquire(&PackageSource::git(&fixture.url)).unwrap();
    let installed = store.ingest(&materialized).expect("ingestion succeeds");

    assert_eq!(installed.id.as_str(), "git-fixture");
    assert!(installed.root.join("skills/example/SKILL.md").is_file());
    assert!(matches!(
        installed.provenance.requested,
        PackageSource::Git { .. }
    ));
    assert_eq!(
        resolved_commit(&installed.provenance.resolved),
        fixture.head()
    );

    // The Store keeps the only copy; the scratch checkout is gone.
    drop(materialized);
    assert!(installed.root.join("plugin.json").is_file());
}

/// Submodules are never recursed into, so a repository cannot pull in
/// content — or a URL — UZE was not asked about.
#[test]
fn submodules_are_not_recursed_into() {
    let inner = Fixture::new("submodule-inner", false);
    let outer = Fixture::with_layout("submodule-outer", |root| {
        write_package(root, "outer-package", false);
    });
    // Declare a submodule without vendoring its content.
    fs::write(
        outer.work.join(".gitmodules"),
        format!(
            "[submodule \"inner\"]\n\tpath = inner\n\turl = {}\n",
            inner.url
        ),
    )
    .unwrap();
    git(&["add", "-A"], &outer.work);
    git(
        &["commit", "--quiet", "-m", "declare submodule"],
        &outer.work,
    );
    git(&["push", "--quiet", "origin", "trunk"], &outer.work);

    let materialized = acquire(&PackageSource::git(&outer.url)).unwrap();
    assert!(
        !materialized.root().join("inner/plugin.json").exists(),
        "a submodule was fetched"
    );
}

// ---------------------------------------------------------------------------
// The trust boundary. `uze add ./plugin` always let a package register an MCP
// server, but the operator had the directory in front of them. A remote source
// removes that, so installing stops meaning "copy some files" and starts
// meaning "authorize execution of something nobody read".
// ---------------------------------------------------------------------------

use uze::{
    UzeApplication,
    trust::{NoTrustAuthority, TrustAuthority, TrustOutcome, TrustRequest},
};

/// Records what it was asked, so a test can assert the operator was shown
/// enough to decide.
struct RecordingAuthority {
    outcome: TrustOutcome,
    seen: std::cell::RefCell<Vec<TrustRequest>>,
}

impl RecordingAuthority {
    fn new(outcome: TrustOutcome) -> Self {
        Self {
            outcome,
            seen: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl TrustAuthority for RecordingAuthority {
    fn authorize(&self, request: &TrustRequest) -> TrustOutcome {
        self.seen.borrow_mut().push(request.clone());
        self.outcome
    }
}

fn application(root: &Path) -> (UzeHome, UzeApplication) {
    let home = UzeHome::at(root.join("uze-home"));
    (home.clone(), UzeApplication::new(home, Vec::new()))
}

/// A declarative package asks nothing of the operator: a Skill is text a
/// model reads, not code that runs.
#[test]
fn a_remote_package_with_only_a_skill_requires_no_trust() {
    let fixture = Fixture::new("trust-declarative", false);
    let (_, application) = application(&fixture.root);
    let authority = RecordingAuthority::new(TrustOutcome::Denied);

    application
        .add_plugin(PackageSource::git(&fixture.url), &authority)
        .expect("a declarative package installs without a trust question");

    assert!(
        authority.seen.borrow().is_empty(),
        "the operator was asked about a package that executes nothing"
    );
}

/// A declared MCP `command` is what crosses the boundary — and the request
/// must carry enough to judge it.
#[test]
fn a_remote_package_with_an_mcp_command_requires_trust() {
    let fixture = Fixture::new("trust-executable", true);
    let (_, application) = application(&fixture.root);
    let authority = RecordingAuthority::new(TrustOutcome::Granted);

    application
        .add_plugin(PackageSource::git(&fixture.url), &authority)
        .expect("a granted install succeeds");

    let seen = authority.seen.borrow();
    let request = seen.first().expect("the operator was never asked");
    assert_eq!(request.package_id, "git-fixture");
    assert!(request.requested_source.contains("file://"));
    assert_eq!(
        request.resolved_source,
        format!("{}@{}", fixture.url, fixture.head())
    );
    assert_eq!(request.executable.len(), 1);
    assert_eq!(request.executable[0].command, "./bin/server");
    assert_eq!(request.executable[0].arguments, vec!["--stdio".to_owned()]);
    assert!(!request.previously_trusted);
}

/// Denial must leave nothing behind — no bytes, no registry entry, no
/// attachment. Consent is asked before the Store is touched precisely so
/// "no" costs nothing to honour.
#[test]
fn denied_trust_leaves_the_store_completely_untouched() {
    let fixture = Fixture::new("trust-denied", true);
    let (home, application) = application(&fixture.root);
    let authority = RecordingAuthority::new(TrustOutcome::Denied);

    let error = application
        .add_plugin(PackageSource::git(&fixture.url), &authority)
        .expect_err("a denied install succeeded");
    assert!(matches!(error, UzeError::TrustDenied(_)));

    assert!(application.list_plugins().unwrap().is_empty());
    assert!(
        !home.packages_dir().join("git-fixture").exists(),
        "a denied package left bytes in the store"
    );
}

/// A process that cannot ask must fail with a signal a pipeline can act on,
/// never with a silent yes.
#[test]
fn a_non_interactive_process_reports_trust_required_rather_than_assuming_consent() {
    let fixture = Fixture::new("trust-headless", true);
    let (home, application) = application(&fixture.root);

    let error = application
        .add_plugin(PackageSource::git(&fixture.url), &NoTrustAuthority)
        .expect_err("a headless install succeeded");
    match &error {
        UzeError::TrustRequired { package, detail } => {
            assert_eq!(package, "git-fixture");
            assert!(detail.contains("./bin/server"), "detail was: {detail}");
        }
        other => panic!("expected TRUST_REQUIRED, got {other}"),
    }
    assert!(error.to_string().contains("TRUST_REQUIRED"));
    assert!(!home.packages_dir().join("git-fixture").exists());
}

/// A local path is unchanged from the posture UZE has always had: the
/// operator has the directory. This is a deliberate scope, and the test
/// exists so widening it is a decision rather than an accident.
#[test]
fn a_local_package_with_an_mcp_command_still_requires_no_trust() {
    let root = temporary("trust-local");
    let package = root.join("package");
    write_package(&package, "local-fixture", true);
    let (_, application) = application(&root);
    let authority = RecordingAuthority::new(TrustOutcome::Denied);

    application
        .add_plugin(PackageSource::local(&package), &authority)
        .expect("a local install is not gated on trust");
    assert!(authority.seen.borrow().is_empty());

    let _ = fs::remove_dir_all(root);
}

/// Consent is not inherited because the package id is unchanged: a revision
/// that introduces execution the installed one never had is a new question.
#[test]
fn an_update_introducing_executable_capability_asks_again() {
    let fixture = Fixture::new("trust-update", false);
    let (_, application) = application(&fixture.root);

    // First revision is declarative and installs unasked.
    let silent = RecordingAuthority::new(TrustOutcome::Denied);
    application
        .add_plugin(PackageSource::git(&fixture.url), &silent)
        .expect("the declarative revision installs");
    assert!(silent.seen.borrow().is_empty());

    // The next revision adds an MCP server.
    fs::write(
        fixture.work.join("mcp.json"),
        r#"{"mcpServers":{"server":{"command":"./bin/added","args":[]}}}"#,
    )
    .unwrap();
    git(&["add", "-A"], &fixture.work);
    git(&["commit", "--quiet", "-m", "add mcp"], &fixture.work);
    git(&["push", "--quiet", "origin", "trunk"], &fixture.work);

    let asked = RecordingAuthority::new(TrustOutcome::Granted);
    application
        .update_plugin("git-fixture", &asked)
        .expect("the update succeeds once trusted");

    let seen = asked.seen.borrow();
    let request = seen.first().expect("the update did not ask");
    assert!(
        request.previously_trusted,
        "the request did not mark this as an already-installed package"
    );
    assert_eq!(request.executable[0].command, "./bin/added");
}
