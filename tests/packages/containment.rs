//! L1 contract: **an installed package is self-contained within its root.**
//!
//! No symlink the Store persists may resolve outside the package. This is not
//! a rule about acquisition: it protects what happens *after* installation.
//! An integration later points a harness at a path inside the store, and the
//! harness follows whatever it finds there — so a package that reaches
//! outside its own root turns UZE into the thing that handed a harness a
//! pointer to arbitrary filesystem content.
//!
//! Every source is held to it identically, which is why these tests use a
//! plain local directory. If containment only applied to remote acquisition,
//! the same malicious package would simply be offered as a local path.

#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

use uze::{PackageSource, UzeError, UzeHome, UzeStore};

fn temporary(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

/// A minimal valid Agent Plugin, so every rejection below is about
/// containment and never about a malformed package.
fn package_at(root: &Path) {
    fs::create_dir_all(root.join("skills/example")).unwrap();
    fs::write(
        root.join("plugin.json"),
        r#"{"name":"containment-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        root.join("skills/example/SKILL.md"),
        "---\nname: example\ndescription: fixture\n---\n\nbody\n",
    )
    .unwrap();
}

fn install(root: &Path) -> (UzeHome, uze::Result<uze::StoredPackage>) {
    let home = UzeHome::at(root.join("uze-home"));
    let store = UzeStore::new(home.clone());
    let package = root.join("package");
    let result = uze::acquisition::acquire(&PackageSource::local(&package))
        .and_then(|materialized| store.ingest(&materialized));
    (home, result)
}

fn assert_escape_rejected(result: uze::Result<uze::StoredPackage>, label: &str) {
    match result {
        Err(UzeError::PackageEscapesRoot { .. }) => {}
        Err(other) => panic!("{label} was rejected for the wrong reason: {other}"),
        Ok(_) => panic!("{label} was installed"),
    }
}

#[test]
fn an_absolute_symlink_escape_is_rejected() {
    let root = temporary("absolute");
    let package = root.join("package");
    package_at(&package);
    symlink("/etc", package.join("escape")).unwrap();

    let (home, result) = install(&root);
    assert_escape_rejected(result, "an absolute symlink");
    // Rejected before any byte was written, so nothing is left half-installed.
    assert!(
        !home.packages_dir().join("containment-fixture").exists(),
        "a rejected package still left bytes in the store"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_relative_parent_escape_is_rejected() {
    let root = temporary("relative");
    let package = root.join("package");
    package_at(&package);
    symlink("../../../etc/passwd", package.join("skills/example/escape")).unwrap();

    let (_, result) = install(&root);
    assert_escape_rejected(result, "a `..` escape");

    let _ = fs::remove_dir_all(root);
}

/// A chain can only leave the root if some individual link leaves it, and
/// every link is checked. Nothing is ever followed, so a chain needs no
/// special handling and a cycle has nothing to loop on.
#[test]
fn a_chained_symlink_escaping_the_root_is_rejected() {
    let root = temporary("chained");
    let package = root.join("package");
    package_at(&package);
    // first -> second (inside, fine on its own), second -> outside.
    symlink("second", package.join("first")).unwrap();
    symlink("/etc", package.join("second")).unwrap();

    let (_, result) = install(&root);
    assert_escape_rejected(result, "a chained escape");

    let _ = fs::remove_dir_all(root);
}

/// A symlink pointing into a directory that is itself an escaping symlink
/// looks contained on its own; the escaping hop is what gets caught.
#[test]
fn an_escape_through_a_symlinked_directory_is_rejected() {
    let root = temporary("through-dir");
    let package = root.join("package");
    package_at(&package);
    symlink("/etc", package.join("outside")).unwrap();
    symlink("outside/passwd", package.join("indirect")).unwrap();

    let (_, result) = install(&root);
    assert_escape_rejected(result, "an escape through a symlinked directory");

    let _ = fs::remove_dir_all(root);
}

/// The invariant constrains where a link resolves, not whether links exist.
/// A package that references its own content keeps working.
#[test]
fn a_valid_internal_symlink_is_preserved() {
    let root = temporary("internal");
    let package = root.join("package");
    package_at(&package);
    fs::create_dir_all(package.join("bin")).unwrap();
    fs::write(package.join("bin/run"), "#!/bin/sh\n").unwrap();
    symlink("run", package.join("bin/current")).unwrap();
    // Also a link reaching across directories but staying inside the root.
    symlink("../bin/run", package.join("skills/example/tool")).unwrap();

    let (home, result) = install(&root);
    let installed = result.expect("an internally-linked package installs");
    assert!(installed.root.join("bin/current").is_symlink());
    assert!(installed.root.join("skills/example/tool").is_symlink());
    assert_eq!(
        fs::read_link(installed.root.join("bin/current")).unwrap(),
        PathBuf::from("run"),
        "the original link target was rewritten"
    );

    let _ = fs::remove_dir_all(home.root());
    let _ = fs::remove_dir_all(root);
}

/// Containment is a property of the package, not of how it arrived. A local
/// directory is held to exactly the rule a cloned repository will be.
#[test]
fn containment_is_enforced_for_a_plain_local_directory() {
    let root = temporary("local-source");
    let package = root.join("package");
    package_at(&package);
    symlink("/", package.join("root-escape")).unwrap();

    let (_, result) = install(&root);
    assert_escape_rejected(result, "a local package escaping its root");

    let _ = fs::remove_dir_all(root);
}

// ---------------------------------------------------------------------------
// Discovery must terminate on any tree a self-contained package may legally
// contain. Containment forbids leaving the root; it does not forbid a cycle
// *inside* it, so these are the cases the traversal rule has to survive.
// Each asserts termination: reaching the assertion at all is the result.
// ---------------------------------------------------------------------------

/// `a -> b`, `b -> a`. Legal under containment, fatal to a following walk.
#[test]
fn a_mutual_symlink_cycle_does_not_hang_discovery() {
    let root = temporary("cycle-mutual");
    let package = root.join("package");
    package_at(&package);
    symlink("b", package.join("skills/a")).unwrap();
    symlink("a", package.join("skills/b")).unwrap();

    let (home, result) = install(&root);
    let installed = result.expect("a cyclic but contained package installs");
    // The real skill is still found; the cycle is simply not entered.
    let found = uze::project::files_named(&installed.root.join("skills"), "SKILL.md").unwrap();
    assert_eq!(found.len(), 1);

    let _ = fs::remove_dir_all(home.root());
    let _ = fs::remove_dir_all(root);
}

/// A link to itself — the shortest possible cycle.
#[test]
fn a_self_referencing_symlink_does_not_hang_discovery() {
    let root = temporary("cycle-self");
    let package = root.join("package");
    package_at(&package);
    symlink("loop", package.join("skills/loop")).unwrap();

    let (home, result) = install(&root);
    let installed = result.expect("a self-linked but contained package installs");
    assert_eq!(
        uze::project::files_named(&installed.root.join("skills"), "SKILL.md")
            .unwrap()
            .len(),
        1
    );

    let _ = fs::remove_dir_all(home.root());
    let _ = fs::remove_dir_all(root);
}

/// A symlinked directory pointing at an ancestor inside the package: the
/// classic infinite descent.
#[test]
fn a_symlinked_directory_pointing_at_its_own_ancestor_does_not_hang_discovery() {
    let root = temporary("cycle-ancestor");
    let package = root.join("package");
    package_at(&package);
    symlink("..", package.join("skills/up")).unwrap();

    let (home, result) = install(&root);
    let installed = result.expect("an ancestor-linked but contained package installs");
    assert_eq!(
        uze::project::files_named(&installed.root.join("skills"), "SKILL.md")
            .unwrap()
            .len(),
        1
    );

    let _ = fs::remove_dir_all(home.root());
    let _ = fs::remove_dir_all(root);
}

/// The documented consequence of not following symlinked directories, kept
/// as a test so the trade-off is visible rather than discovered later: the
/// symlink survives in the Store as package content, but discovery does not
/// walk through it.
#[test]
fn content_reachable_only_through_a_symlinked_directory_is_not_discovered() {
    let root = temporary("through-link");
    let package = root.join("package");
    package_at(&package);
    fs::create_dir_all(package.join("extra/hidden")).unwrap();
    fs::write(
        package.join("extra/hidden/SKILL.md"),
        "---\nname: hidden\ndescription: fixture\n---\n\nbody\n",
    )
    .unwrap();
    symlink("../extra/hidden", package.join("skills/linked")).unwrap();

    let (home, result) = install(&root);
    let installed = result.expect("the package installs");
    assert!(
        installed.root.join("skills/linked").is_symlink(),
        "the symlink was not preserved as package content"
    );
    assert_eq!(
        uze::project::files_named(&installed.root.join("skills"), "SKILL.md")
            .unwrap()
            .len(),
        1,
        "discovery walked through a symlinked directory"
    );

    let _ = fs::remove_dir_all(home.root());
    let _ = fs::remove_dir_all(root);
}
