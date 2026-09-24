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

use super::forge::{self, Transport};
use super::git::{Reach, reject_option_shaped, run, run_as, through};
use crate::error::{Result, UzeError};

/// Blobs are what a clone spends its time on, and what answering a question
/// about *history* never needs.
const NO_BLOBS: &str = "--filter=blob:none";

/// Where a mirror remembers which repository it is and how it was last
/// reached. Inside the mirror, so it goes with it: the mirror is cache, and
/// so is this.
const TRANSPORT_FILE: &str = "transport.json";

/// A mirror's own account of itself.
#[derive(serde::Serialize, serde::Deserialize)]
struct Remembered {
    identity: String,
    transport: Transport,
}

/// Makes `directory` a mirror of the repository `identity` names, cloning
/// it when there is none and fetching into it when there is. `fetch` is
/// where the bytes are read from — the identity itself, or a checkout of it
/// on this disk.
///
/// The fetch is what makes a second plugin from one marketplace free: the
/// objects the first install brought are already here, and only what has
/// been pushed since travels.
///
/// Reached through [`through`](super::git::through), starting from the
/// transport that worked last time. `origin` is set to the transport that
/// answered, because a blobless mirror fetches a missing blob from `origin`
/// later, and it must reach the same place the same way.
pub fn ensure(fetch: &str, identity: &str, directory: &Path) -> Result<()> {
    super::git::reject_inline_credentials(fetch)?;
    reject_option_shaped(fetch, "repository url")?;

    let remembered = remembered(directory);
    if directory.join("HEAD").exists() {
        // A directory that is a mirror of *something else* is not this
        // marketplace's, whatever it is called on disk. Fetching into it
        // would answer questions about one repository with another's
        // history — which is exactly what a name registered against a new
        // source must not get. A mirror of the same repository under an
        // older spelling is this one, and is kept.
        let known = match &remembered {
            Some(remembered) => remembered.identity.clone(),
            None => run(&["remote", "get-url", "origin"], Some(directory))
                .map(|answer| answer.trim().to_owned())
                .unwrap_or_default(),
        };
        if forge::same_repository(&known, identity) || forge::same_repository(&known, fetch) {
            let transport = remembered.as_ref().map(|remembered| &remembered.transport);
            // `origin` is where a blobless mirror fetches a missing blob
            // later, so it is pointed at each transport as it is tried — and
            // put back when none answers, or the next pinned install would
            // fetch from the last one tried with the access of another.
            let origin = run(&["remote", "get-url", "origin"], Some(directory))
                .map(|answer| answer.trim().to_owned())
                .unwrap_or_default();
            let refs = run(
                &["for-each-ref", "--format=%(objectname) %(refname)"],
                Some(directory),
            )
            .unwrap_or_default();
            let answered = through(fetch, transport, |transport| {
                run(
                    &["remote", "set-url", "origin", &transport.url],
                    Some(directory),
                )?;
                // `--prune` so a ref deleted upstream stops being resolvable
                // here, which is the honest answer to "does this ref still
                // exist".
                run_as(
                    Reach::of(transport),
                    &["fetch", "--prune", NO_BLOBS, "origin", "+refs/*:refs/*"],
                    Some(directory),
                )?;
                let answered = super::git::holds_a_commit(directory, transport);
                if answered.is_err() {
                    // An answer with no refs at all — a login page served
                    // with 200 — pruned every ref this mirror had.
                    restore_refs(directory, &refs);
                }
                answered
            });
            return match answered {
                Ok(((), answered)) => remember(directory, identity, answered),
                Err(error) => {
                    if !origin.is_empty() {
                        let _ = run(&["remote", "set-url", "origin", &origin], Some(directory));
                    }
                    Err(error)
                }
            };
        }
        std::fs::remove_dir_all(directory).map_err(|source| UzeError::Write {
            path: directory.to_path_buf(),
            source,
        })?;
    }

    if let Some(parent) = directory.parent() {
        std::fs::create_dir_all(parent).map_err(|source| UzeError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    // Bare: nothing here is ever edited, and a working tree would be the
    // second materialized copy this exists to remove.
    let ((), answered) = through(fetch, None, |transport| {
        let _ = std::fs::remove_dir_all(directory);
        run_as(
            Reach::of(transport),
            &[
                "clone",
                "--bare",
                NO_BLOBS,
                "--no-recurse-submodules",
                "--",
                &transport.url,
                &directory.to_string_lossy(),
            ],
            None,
        )?;
        super::git::holds_a_commit(directory, transport)
    })?;
    remember(directory, identity, answered)
}

/// Puts back the refs `listing` (`<object> <ref>` per line) names.
fn restore_refs(directory: &Path, listing: &str) {
    for line in listing.lines() {
        if let Some((object, reference)) = line.split_once(' ') {
            let _ = run(&["update-ref", reference, object], Some(directory));
        }
    }
}

/// How each mirror was last reached, as this process last read or wrote
/// it: a listing reads several manifests out of one mirror, and each read
/// needs the answer.
static REACHES: std::sync::Mutex<Vec<(std::path::PathBuf, Reach)>> =
    std::sync::Mutex::new(Vec::new());

fn remembered(directory: &Path) -> Option<Remembered> {
    serde_json::from_slice(&std::fs::read(directory.join(TRANSPORT_FILE)).ok()?).ok()
}

fn remember(directory: &Path, identity: &str, transport: Transport) -> Result<()> {
    let payload = serde_json::to_vec_pretty(&Remembered {
        identity: identity.to_owned(),
        transport,
    })
    .expect("a transport is serializable");
    if let Ok(mut reaches) = REACHES.lock() {
        reaches.retain(|(known, _)| known != directory);
    }
    std::fs::write(directory.join(TRANSPORT_FILE), payload).map_err(|source| UzeError::Write {
        path: directory.join(TRANSPORT_FILE),
        source,
    })
}

/// How a mirror is reached again for what it does not hold yet — a blob a
/// checkout asks for — which is how it was last reached.
fn reach_of(directory: &Path) -> Reach {
    if let Some(reach) = REACHES.lock().ok().and_then(|reaches| {
        reaches
            .iter()
            .find(|(known, _)| known == directory)
            .map(|(_, reach)| *reach)
    }) {
        return reach;
    }
    let reach =
        remembered(directory).map_or(Reach::LOCAL, |remembered| Reach::of(&remembered.transport));
    if let Ok(mut reaches) = REACHES.lock() {
        reaches.push((directory.to_path_buf(), reach));
    }
    reach
}

/// Makes `directory` able to answer about `reference`, fetching only when
/// it cannot already.
///
/// A reference that is a full commit id the mirror already holds is
/// immutable: nothing a fetch could bring would change what it names, so
/// the fetch is skipped. That is what a project reproducing its
/// `agents.lock` asks for, and it makes doing so cost no network at all on
/// a machine that has seen that commit before.
///
/// Everything else — a branch, a tag, no reference at all — names whatever
/// it names *now*, and only the remote knows that.
pub fn ensure_for(
    fetch: &str,
    identity: &str,
    directory: &Path,
    reference: Option<&str>,
) -> Result<()> {
    if let Some(reference) = reference
        && is_full_commit_id(reference)
        && directory.join("HEAD").exists()
        && resolve(directory, Some(reference)).is_ok()
    {
        return Ok(());
    }
    ensure(fetch, identity, directory)
}

/// Whether `reference` is a full 40-character commit id, which is the only
/// shape that cannot come to mean something else.
fn is_full_commit_id(reference: &str) -> bool {
    reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit())
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
    run_as(reach_of(directory), &["show", &target], Some(directory)).map(String::into_bytes)
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
    let counted: usize = run(&["rev-list", "--count", &range], Some(directory))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    // Nothing reachable from `head` that `pinned` lacks, yet they are not
    // the same commit: `head` is *behind* `pinned`. Counting the other
    // direction would be a distance, but not this one's — the ref moved
    // backwards, and `Some(0)` here would read as "identical" at every
    // caller.
    (counted > 0).then_some(counted)
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
    // The destination goes with a failure. Left behind, it is a directory
    // holding nothing that a caller cannot tell from one holding the
    // answer.
    if let Err(error) = run_as(reach_of(directory), &arguments, Some(directory)) {
        let _ = std::fs::remove_dir_all(destination);
        return Err(error);
    }
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
        // The label goes into the tree: two fixtures with identical
        // content, author and message in the same second produce the same
        // commit, which would let a test comparing two repositories pass
        // for the wrong reason.
        fs::write(origin.join("who.txt"), label).unwrap();
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
        ensure(
            &root.join("origin").to_string_lossy(),
            &root.join("origin").to_string_lossy(),
            &mirror,
        )
        .unwrap();

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
        ensure(
            &root.join("origin").to_string_lossy(),
            &root.join("origin").to_string_lossy(),
            &mirror,
        )
        .unwrap();

        assert_eq!(distance(&mirror, &second, &second), Some(0));
        assert_eq!(distance(&mirror, &first, &second), Some(1));
        assert_eq!(
            distance(&mirror, "0000000000000000000000000000000000000000", &second),
            None,
            "a commit this mirror does not have yields no number at all"
        );
        assert_eq!(
            distance(&mirror, &second, &first),
            None,
            "a ref that moved backwards is not a distance of zero: `Some(0)` \
             is what an identical pair answers, and these are not identical"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_subdirectory_is_materialized_and_the_rest_is_not() {
        let (root, _first, second) = origin("mirror-materialize");
        let mirror = root.join("mirror");
        ensure(
            &root.join("origin").to_string_lossy(),
            &root.join("origin").to_string_lossy(),
            &mirror,
        )
        .unwrap();

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
    fn a_commit_the_mirror_already_holds_needs_no_network() {
        let (root, first, second) = origin("mirror-offline");
        let origin_dir = root.join("origin");
        let mirror = root.join("mirror");
        ensure(
            &origin_dir.to_string_lossy(),
            &origin_dir.to_string_lossy(),
            &mirror,
        )
        .unwrap();

        // The remote goes away entirely.
        fs::remove_dir_all(&origin_dir).unwrap();

        // A pinned commit still answers, because it cannot come to mean
        // anything else than what the mirror already holds.
        ensure_for(
            "does-not-resolve",
            "does-not-resolve",
            &mirror,
            Some(&first),
        )
        .unwrap();
        assert_eq!(resolve(&mirror, Some(&first)).unwrap(), first);
        ensure_for(
            "does-not-resolve",
            "does-not-resolve",
            &mirror,
            Some(&second),
        )
        .unwrap();

        // A branch does not: only the remote knows where it points now.
        assert!(
            ensure_for(
                "does-not-resolve",
                "does-not-resolve",
                &mirror,
                Some("main")
            )
            .is_err()
        );
        assert!(ensure_for("does-not-resolve", "does-not-resolve", &mirror, None).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_commit_describes_itself_and_an_absent_one_says_nothing() {
        let (root, first, second) = origin("mirror-describe");
        let mirror = root.join("mirror");
        ensure(
            &root.join("origin").to_string_lossy(),
            &root.join("origin").to_string_lossy(),
            &mirror,
        )
        .unwrap();

        let described = describe(&mirror, &second).expect("the head is described");
        assert!(second.starts_with(&described.short));
        assert_eq!(described.subject, "second");
        assert!(
            described.age.contains("ago") || described.age.contains("now"),
            "Git's own relative date, whatever it decided: {}",
            described.age
        );
        assert_eq!(describe(&mirror, &first).unwrap().subject, "first");

        assert!(
            describe(&mirror, "0000000000000000000000000000000000000000").is_none(),
            "a commit this mirror does not have is an answer, not a failure"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_plugins_own_last_commit_is_not_the_marketplaces_head() {
        let (root, _first, second) = origin("mirror-describe-path");
        let origin_dir = root.join("origin");
        let mirror = root.join("mirror");

        // The marketplace moves, without touching the plugin.
        fs::write(origin_dir.join("README.md"), "unrelated").unwrap();
        run(&["add", "-A"], Some(&origin_dir)).unwrap();
        run(&["commit", "-m", "docs: unrelated"], Some(&origin_dir)).unwrap();
        ensure(
            &origin_dir.to_string_lossy(),
            &origin_dir.to_string_lossy(),
            &mirror,
        )
        .unwrap();
        let head = resolve(&mirror, Some("main")).unwrap();

        assert_eq!(describe(&mirror, &head).unwrap().subject, "docs: unrelated");
        let plugin = describe_path(&mirror, &head, Some("plugins/flow")).unwrap();
        assert_eq!(
            plugin.subject, "second",
            "a plugin is described by the last commit that touched it, not by the \
             marketplace's head"
        );
        assert!(second.starts_with(&plugin.short));

        assert!(
            describe_path(&mirror, &head, Some("plugins/nothing-here")).is_none(),
            "a path no commit has touched has nothing to describe"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_mirror_of_another_repository_is_replaced_not_fetched_into() {
        let (root, _first, second) = origin("mirror-foreign");
        let mirror = root.join("mirror");
        ensure(
            &root.join("origin").to_string_lossy(),
            &root.join("origin").to_string_lossy(),
            &mirror,
        )
        .unwrap();
        assert_eq!(resolve(&mirror, Some("main")).unwrap(), second);

        // A different repository, registered under the same directory.
        let (other_root, _, other_head) = origin("mirror-foreign-other");
        ensure(
            &other_root.join("origin").to_string_lossy(),
            &other_root.join("origin").to_string_lossy(),
            &mirror,
        )
        .unwrap();

        assert_eq!(
            resolve(&mirror, Some("main")).unwrap(),
            other_head,
            "the mirror answers about the repository it was last pointed at"
        );
        assert_eq!(
            distance(&mirror, &second, &other_head),
            None,
            "the previous repository's history is gone, not silently merged"
        );
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&other_root).unwrap();
    }

    #[test]
    fn a_second_ensure_fetches_instead_of_cloning() {
        let (root, _first, second) = origin("mirror-fetch");
        let origin_dir = root.join("origin");
        let mirror = root.join("mirror");
        ensure(
            &origin_dir.to_string_lossy(),
            &origin_dir.to_string_lossy(),
            &mirror,
        )
        .unwrap();
        assert_eq!(resolve(&mirror, Some("main")).unwrap(), second);

        // The marketplace moves.
        fs::write(origin_dir.join("README.md"), "third").unwrap();
        run(&["add", "-A"], Some(&origin_dir)).unwrap();
        run(&["commit", "-m", "third"], Some(&origin_dir)).unwrap();
        let third = run(&["rev-parse", "HEAD"], Some(&origin_dir))
            .unwrap()
            .trim()
            .to_owned();

        ensure(
            &origin_dir.to_string_lossy(),
            &origin_dir.to_string_lossy(),
            &mirror,
        )
        .unwrap();
        assert_eq!(resolve(&mirror, Some("main")).unwrap(), third);
        assert_eq!(distance(&mirror, &second, &third), Some(1));
        fs::remove_dir_all(&root).unwrap();
    }
}

/// What a checkout's `subdirectory` holds, as package content: the files
/// Git tracks, plus the ones written and not yet committed, minus the ones
/// its ignore rules exclude.
///
/// That set is the author's own intent, already written down. A file they
/// have just created is what a linked marketplace exists to deliver; a
/// file they told Git to ignore — an editor's swapfile, a build artifact —
/// is not part of the package in any revision, and would otherwise reach a
/// harness as one.
///
/// Paths are relative to `checkout` and sorted, so two calls on an
/// unchanged tree answer identically.
pub fn tracked_and_new(checkout: &Path, subdirectory: Option<&str>) -> Result<Vec<String>> {
    let mut arguments = vec![
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
        "--",
    ];
    let spec = subdirectory.unwrap_or(".");
    reject_option_shaped(spec, "subdirectory")?;
    arguments.push(spec);
    let listing = run(&arguments, Some(checkout))?;
    let mut paths: Vec<String> = listing
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Copies `subdirectory` of a linked `checkout` into `destination` — the
/// files [`tracked_and_new`] names, and nothing else.
///
/// The one path that reads a working tree somebody is editing. Everything
/// else here reads history, which cannot change under it.
pub fn materialize_linked(
    checkout: &Path,
    subdirectory: Option<&str>,
    destination: &Path,
) -> Result<()> {
    for relative in tracked_and_new(checkout, subdirectory)? {
        let source = checkout.join(&relative);
        // Written out at the same depth a materialized commit would be, so
        // a caller resolves the plugin's root the same way either way.
        let target = destination.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| UzeError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        // A path Git lists but that is no longer there — deleted between
        // the listing and the copy — is not an error: it is content the
        // author has just removed.
        match std::fs::copy(&source, &target) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source_error) => {
                return Err(UzeError::Read {
                    path: source,
                    source: source_error,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod linked_tests {
    use super::*;
    use std::fs;

    fn checkout(label: &str) -> std::path::PathBuf {
        let root = uze_testkit::temp::scratch(label);
        let at = root.join("checkout");
        fs::create_dir_all(at.join("plugins/flow")).unwrap();
        let git = |args: &[&str]| {
            run(args, Some(&at)).unwrap();
        };
        git(&["init", "--initial-branch=main"]);
        git(&["config", "user.email", "t@example.invalid"]);
        git(&["config", "user.name", "Test"]);
        fs::write(at.join(".gitignore"), "*.swp\ntarget/\n").unwrap();
        fs::write(at.join("plugins/flow/plugin.json"), r#"{"name":"flow"}"#).unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "first"]);
        at
    }

    #[test]
    fn a_file_written_and_not_yet_committed_is_package_content() {
        let at = checkout("linked-uncommitted");
        fs::write(at.join("plugins/flow/new.md"), "just written").unwrap();

        let content = tracked_and_new(&at, Some("plugins/flow")).unwrap();

        assert!(
            content.contains(&"plugins/flow/new.md".to_owned()),
            "what the link exists to deliver: {content:?}"
        );
        fs::remove_dir_all(at.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_file_the_checkout_ignores_is_not_package_content() {
        let at = checkout("linked-ignored");
        fs::write(at.join("plugins/flow/.SKILL.md.swp"), "editor noise").unwrap();
        fs::create_dir_all(at.join("plugins/flow/target")).unwrap();
        fs::write(at.join("plugins/flow/target/built"), "artifact").unwrap();

        let content = tracked_and_new(&at, Some("plugins/flow")).unwrap();

        assert!(
            !content.iter().any(|path| path.contains(".swp")),
            "an editor's temporary file never reaches a harness: {content:?}"
        );
        assert!(
            !content.iter().any(|path| path.contains("target/")),
            "nor does a build artifact: {content:?}"
        );
        fs::remove_dir_all(at.parent().unwrap()).unwrap();
    }

    #[test]
    fn materializing_writes_the_content_and_only_the_content() {
        let at = checkout("linked-materialize");
        fs::write(at.join("plugins/flow/new.md"), "just written").unwrap();
        fs::write(at.join("plugins/flow/.SKILL.md.swp"), "editor noise").unwrap();
        let out = at.parent().unwrap().join("out");

        materialize_linked(&at, Some("plugins/flow"), &out).unwrap();

        assert!(out.join("plugins/flow/plugin.json").is_file());
        assert!(out.join("plugins/flow/new.md").is_file());
        assert!(!out.join("plugins/flow/.SKILL.md.swp").exists());
        fs::remove_dir_all(at.parent().unwrap()).unwrap();
    }
}

/// What one commit says about itself: its short form, how long ago it
/// landed, and its subject.
///
/// The age comes from Git's own `%cr` ("3 hours ago") rather than from a
/// timestamp this would have to render: Git already decides where the
/// boundary between hours and days falls, and a second opinion on that is
/// a second thing to get wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitSummary {
    pub short: String,
    pub age: String,
    pub subject: String,
}

/// The last commit at or before `commit` that touched `subdirectory`,
/// described.
///
/// A marketplace's own head says when the *marketplace* moved, which for a
/// repository carrying several plugins is rarely when any one of them
/// changed: a marketplace whose head is an hour old can hold a plugin
/// nobody has touched in two weeks. The question a person asks of a plugin
/// is about the plugin.
pub fn describe_path(
    directory: &Path,
    commit: &str,
    subdirectory: Option<&str>,
) -> Option<CommitSummary> {
    let Some(subdirectory) = subdirectory.filter(|path| *path != ".") else {
        return describe(directory, commit);
    };
    reject_option_shaped(commit, "commit").ok()?;
    reject_option_shaped(subdirectory, "subdirectory").ok()?;
    let answer = run(
        &[
            "log",
            "-1",
            "--format=%h%x1f%cr%x1f%s",
            commit,
            "--",
            subdirectory,
        ],
        Some(directory),
    )
    .ok()?;
    parse_summary(&answer)
}

/// `commit` described, or `None` when this mirror does not have it — a
/// commit the remote no longer carries, or one that was never fetched.
/// Absence is an answer here, not a failure: the caller shows what it
/// knows and says nothing about what it does not.
pub fn describe(directory: &Path, commit: &str) -> Option<CommitSummary> {
    reject_option_shaped(commit, "commit").ok()?;
    let answer = run(
        &["show", "--no-patch", "--format=%h%x1f%cr%x1f%s", commit],
        Some(directory),
    )
    .ok()?;
    parse_summary(&answer)
}

fn parse_summary(answer: &str) -> Option<CommitSummary> {
    let answer = answer.trim();
    if answer.is_empty() {
        return None;
    }
    let mut fields = answer.split('\u{1f}');
    Some(CommitSummary {
        short: fields.next()?.to_owned(),
        age: fields.next()?.to_owned(),
        subject: fields.next().unwrap_or_default().to_owned(),
    })
}
