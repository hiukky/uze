//! One partial repository per marketplace, fetched rather than re-cloned.
//!
//! A marketplace is a Git repository, and UZE asks it three questions: what
//! does it offer, what is in this plugin's directory, and is there anything
//! newer than what a project pinned. Answering them by cloning the whole
//! repository — once to list, once more per plugin installed — paid a
//! network round trip for bytes already on disk, and then threw away the
//! one thing that could answer the third question: `.git`.
//!
//! A mirror is that repository, kept. Cloned without blobs, so what travels
//! is commits and trees — enough to resolve a ref, read one file, and count
//! the distance between two commits — with file content fetched only for
//! what is actually checked out. Refreshed by `fetch`, so the second
//! question costs nothing the first did not already pay for.
//!
//! It lives in the cache tier: deleting it costs one clone and never
//! correctness. Nothing here is authoritative, and a package's bytes are
//! never read from a mirror — they are ingested into the Store, which is
//! where every harness reads them and which must stand with this gone.
//!
//! A server that does not offer the filter ignores it and sends everything,
//! so this is a saving where it is available and correct where it is not.

use std::path::Path;

use super::git::{reject_option_shaped, run};
use crate::error::{Result, UzeError};

/// Blobs are what a clone spends its time on, and what answering a question
/// about *history* never needs.
const NO_BLOBS: &str = "--filter=blob:none";

/// Makes `directory` a mirror of `url`, cloning it when there is none and
/// fetching into it when there is.
///
/// The fetch is what makes a second plugin from one marketplace free: the
/// objects the first install brought are already here, and only what has
/// been pushed since travels.
pub fn ensure(url: &str, directory: &Path) -> Result<()> {
    super::git::reject_inline_credentials(url)?;
    reject_option_shaped(url, "repository url")?;

    if directory.join("HEAD").exists() {
        // `--prune` so a ref deleted upstream stops being resolvable here,
        // which is the honest answer to "does this ref still exist".
        run(
            &["fetch", "--prune", NO_BLOBS, "origin", "+refs/*:refs/*"],
            Some(directory),
        )?;
        return Ok(());
    }

    if let Some(parent) = directory.parent() {
        std::fs::create_dir_all(parent).map_err(|source| UzeError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    // Bare: nothing here is ever edited, and a working tree would be the
    // second materialized copy this exists to remove.
    run(
        &[
            "clone",
            "--bare",
            NO_BLOBS,
            "--no-recurse-submodules",
            "--",
            url,
            &directory.to_string_lossy(),
        ],
        None,
    )?;
    Ok(())
}

/// The commit `reference` names — a branch, a tag, a commit — or the
/// repository's own default branch when it names nothing.
///
/// Resolved to a commit before anything else asks a question, so every
/// later answer is about one immutable thing.
pub fn resolve(directory: &Path, reference: Option<&str>) -> Result<String> {
    let wanted = match reference {
        Some(reference) => {
            reject_option_shaped(reference, "reference")?;
            format!("{reference}^{{commit}}")
        }
        None => "HEAD^{commit}".to_owned(),
    };
    let commit = run(&["rev-parse", "--verify", &wanted], Some(directory))?
        .trim()
        .to_owned();
    reject_option_shaped(&commit, "resolved commit")?;
    Ok(commit)
}

/// One file's bytes at `commit`, read from the repository rather than from
/// a checkout — which is the whole reason a mirror has no working tree.
pub fn read_file(directory: &Path, commit: &str, path: &str) -> Result<Vec<u8>> {
    reject_option_shaped(commit, "commit")?;
    reject_option_shaped(path, "path")?;
    let target = format!("{commit}:{path}");
    run(&["show", &target], Some(directory)).map(String::into_bytes)
}

/// How many commits `head` is ahead of `pinned`.
///
/// `None` when the two do not share history in a way this can answer —
/// a rewritten history, a commit fetched and then pruned, a ref that moved
/// backwards. A number that might be wrong is worse than no number: the
/// caller reports "differs" instead of inventing a distance.
pub fn distance(directory: &Path, pinned: &str, head: &str) -> Option<usize> {
    reject_option_shaped(pinned, "commit").ok()?;
    reject_option_shaped(head, "commit").ok()?;
    if pinned == head {
        return Some(0);
    }
    let range = format!("{pinned}..{head}");
    run(&["rev-list", "--count", &range], Some(directory))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Writes `subdirectory` at `commit` into `destination`, which must not
/// exist.
///
/// This is the only call that needs file content, so it is the only one
/// that makes a blobless mirror reach for blobs — and only for the one
/// directory a plugin occupies, never the repository.
pub fn materialize_subdirectory(
    directory: &Path,
    commit: &str,
    subdirectory: Option<&str>,
    destination: &Path,
) -> Result<()> {
    reject_option_shaped(commit, "commit")?;
    if let Some(subdirectory) = subdirectory {
        reject_option_shaped(subdirectory, "subdirectory")?;
    }
    std::fs::create_dir_all(destination).map_err(|source| UzeError::Write {
        path: destination.to_path_buf(),
        source,
    })?;
    // `--work-tree` writes the tree out without the mirror ever gaining one
    // of its own, and the pathspec confines it to the plugin's directory.
    let work_tree = format!("--work-tree={}", destination.display());
    let mut arguments = vec![work_tree.as_str(), "checkout", commit, "--"];
    let spec = subdirectory.unwrap_or(".");
    arguments.push(spec);
    run(&arguments, Some(directory))?;
    // The index the checkout wrote belongs to the mirror, not to the
    // answer: left behind, the next materialization would read a state
    // from the last one.
    let _ = std::fs::remove_file(directory.join("index"));
    // Containment is not re-checked here: the Store validates every byte it
    // ingests (`store.rs`, "no symlink the Store persists may resolve
    // outside the package root"), and a second copy of that rule is a
    // second place for it to be wrong.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A repository with two commits, the second adding a plugin directory.
    fn origin(label: &str) -> (std::path::PathBuf, String, String) {
        let root = uze_testkit::temp::scratch(label);
        let origin = root.join("origin");
        fs::create_dir_all(origin.join("plugins/flow")).unwrap();
        let git = |args: &[&str]| {
            run(args, Some(&origin)).unwrap_or_else(|error| panic!("{args:?}: {error}"));
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.email", "t@example.invalid"]);
        git(&["config", "user.name", "Test"]);
        fs::write(
            origin.join("marketplace.json"),
            r#"{"name":"m","plugins":[]}"#,
        )
        .unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "first"]);
        let first = run(&["rev-parse", "HEAD"], Some(&origin))
            .unwrap()
            .trim()
            .to_owned();

        fs::write(
            origin.join("plugins/flow/plugin.json"),
            r#"{"name":"flow"}"#,
        )
        .unwrap();
        fs::write(
            origin.join("marketplace.json"),
            r#"{"name":"m","plugins":["flow"]}"#,
        )
        .unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "second"]);
        let second = run(&["rev-parse", "HEAD"], Some(&origin))
            .unwrap()
            .trim()
            .to_owned();
        (root, first, second)
    }

    #[test]
    fn a_mirror_answers_without_a_working_tree() {
        let (root, first, second) = origin("mirror-answers");
        let mirror = root.join("mirror");
        ensure(&root.join("origin").to_string_lossy(), &mirror).unwrap();

        assert_eq!(resolve(&mirror, None).unwrap(), second);
        assert_eq!(resolve(&mirror, Some("main")).unwrap(), second);
        assert_eq!(resolve(&mirror, Some(&first)).unwrap(), first);

        let manifest = read_file(&mirror, &second, "marketplace.json").unwrap();
        assert!(String::from_utf8_lossy(&manifest).contains("flow"));
        // The first commit's manifest is still readable: history is what a
        // mirror keeps and a copied tree throws away.
        let older = read_file(&mirror, &first, "marketplace.json").unwrap();
        assert!(!String::from_utf8_lossy(&older).contains("flow"));

        assert!(
            !mirror.join("marketplace.json").exists(),
            "a mirror materializes nothing"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn distance_counts_commits_and_refuses_to_guess() {
        let (root, first, second) = origin("mirror-distance");
        let mirror = root.join("mirror");
        ensure(&root.join("origin").to_string_lossy(), &mirror).unwrap();

        assert_eq!(distance(&mirror, &second, &second), Some(0));
        assert_eq!(distance(&mirror, &first, &second), Some(1));
        assert_eq!(
            distance(&mirror, "0000000000000000000000000000000000000000", &second),
            None,
            "a commit this mirror does not have yields no number at all"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_subdirectory_is_materialized_and_the_rest_is_not() {
        let (root, _first, second) = origin("mirror-materialize");
        let mirror = root.join("mirror");
        ensure(&root.join("origin").to_string_lossy(), &mirror).unwrap();

        let out = root.join("out");
        materialize_subdirectory(&mirror, &second, Some("plugins/flow"), &out).unwrap();

        assert!(out.join("plugins/flow/plugin.json").is_file());
        assert!(
            !out.join("marketplace.json").exists(),
            "only the plugin's own directory travels"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_second_ensure_fetches_instead_of_cloning() {
        let (root, _first, second) = origin("mirror-fetch");
        let origin_dir = root.join("origin");
        let mirror = root.join("mirror");
        ensure(&origin_dir.to_string_lossy(), &mirror).unwrap();
        assert_eq!(resolve(&mirror, Some("main")).unwrap(), second);

        // The marketplace moves.
        fs::write(origin_dir.join("README.md"), "third").unwrap();
        run(&["add", "-A"], Some(&origin_dir)).unwrap();
        run(&["commit", "-m", "third"], Some(&origin_dir)).unwrap();
        let third = run(&["rev-parse", "HEAD"], Some(&origin_dir))
            .unwrap()
            .trim()
            .to_owned();

        ensure(&origin_dir.to_string_lossy(), &mirror).unwrap();
        assert_eq!(resolve(&mirror, Some("main")).unwrap(), third);
        assert_eq!(distance(&mirror, &second, &third), Some(1));
        fs::remove_dir_all(&root).unwrap();
    }
}
