//! What a previous version left behind that this one did not adopt.
//!
//! A record UZE could not carry across is kept rather than deleted, under
//! a name nothing reads as a record. That is the right thing to do with
//! it and a poor thing to leave unsaid: nothing reads those bytes again,
//! so without somewhere to report them they accumulate silently, and the
//! operator's first sign that anything happened is work they cannot find.
//!
//! Found by sweeping the filesystem rather than by asking each subsystem,
//! which is what lets this answer for the terminal runtime too — the one
//! record `uze-application` cannot reach, because `uze-terminal` depends
//! on nothing here by design.

use std::{fs, path::Path, path::PathBuf};

use crate::home::UzeHome;

/// The mark a set-aside record's name carries. Chosen so that what is set
/// aside stops being a `.json`: it must not read as a second record to
/// anything listing the directory.
pub const SET_ASIDE_MARK: &str = ".unreadable-";

/// How many set-aside records are reported before the rest are counted.
///
/// A record per event, kept forever, is its own leak — and a report that
/// lists forty of them is one nobody reads. The newest are the ones an
/// operator can still act on.
pub const REPORTED: usize = 5;

/// One record this build could not read, kept where it was.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Leftover {
    pub path: PathBuf,
    /// Seconds since the epoch, taken from the name the record was set
    /// aside under — not from the filesystem, which a copy or a restore
    /// rewrites.
    pub set_aside_at_unix: u64,
}

impl Leftover {
    /// What to do about it. The bytes are read by nothing, so the only
    /// action is the operator's.
    pub const REMEDY: &'static str = "nothing reads it; remove it when you no longer want it";
}

/// Every set-aside record under this home, newest first.
///
/// Walks `state/` rather than asking each subsystem for its own, because
/// the point is to find what nobody is asking about — including the
/// workspace, which is written by a crate that depends on nothing here.
pub fn set_aside(home: &UzeHome) -> Vec<Leftover> {
    let mut found = Vec::new();
    collect(&home.state_dir(), &mut found);
    found.sort_by_key(|leftover| std::cmp::Reverse(leftover.set_aside_at_unix));
    found
}

fn collect(directory: &Path, found: &mut Vec<Leftover>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some((_, stamp)) = name.rsplit_once(SET_ASIDE_MARK) {
            found.push(Leftover {
                set_aside_at_unix: stamp.parse().unwrap_or_default(),
                path,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_set_aside_record_is_found_wherever_it_was_kept() {
        let root = uze_testkit::temp::scratch("leftovers");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        fs::create_dir_all(home.state_dir().join("terminal")).unwrap();
        fs::write(home.state_dir().join("packages.json.unreadable-20"), b"x").unwrap();
        fs::write(
            home.state_dir()
                .join("terminal/workspace.json.unreadable-40"),
            b"x",
        )
        .unwrap();
        fs::write(home.state_dir().join("packages.json"), b"{}").unwrap();

        let found = set_aside(&home);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(
            found[0].set_aside_at_unix, 40,
            "newest first: those are the ones an operator can still act on"
        );
        assert!(
            found
                .iter()
                .any(|leftover| leftover.path.to_string_lossy().contains("workspace")),
            "including the runtime's own, which no crate here can be asked for"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_home_with_nothing_left_over_answers_with_nothing() {
        let root = uze_testkit::temp::scratch("leftovers-none");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        assert!(set_aside(&home).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}

/// A reference UZE wrote into a harness's discovery root, pointing into
/// UZE's own home at something that is no longer there, which no receipt
/// claims.
///
/// It can only be UZE's: nothing but UZE writes inside `$UZE_HOME`, so a
/// reference into it was made by a UZE that no longer accounts for it —
/// left by a capability that was renamed or removed while its receipt was
/// lost. It delivers nothing (its target is gone) and it holds a name
/// another package may need, which is the whole reason to report it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DanglingReference {
    pub path: PathBuf,
    /// Where it still points, which is what says it was UZE's.
    pub target: PathBuf,
}

impl DanglingReference {
    pub const REMEDY: &'static str = "nothing reads it; UZE can remove it";
}

/// Every dangling reference under `roots` that `claimed` does not account
/// for, newest first by name so the answer is stable.
///
/// Scoped to the roots the caller passes, which today is the *shared* Agent
/// Skills root: it is the one namespace where one package's leftover blocks
/// a different package's attach. A root a single integration owns can only
/// collide with itself, and asking every integration to enumerate its
/// directories would make each answer a question only one shape of delivery
/// raises.
///
/// Three conditions, all necessary. The entry is a symbolic link; its
/// target is *absent* — not merely unreadable, since a volume not mounted
/// or a directory this user may not traverse is a reference still doing its
/// job; and that target is inside `home`.
pub fn dangling_references(
    home: &UzeHome,
    roots: &[PathBuf],
    claimed: &std::collections::BTreeSet<PathBuf>,
) -> Vec<DanglingReference> {
    let home_root = home.root();
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();
            if claimed.contains(&path) {
                continue;
            }
            let Ok(target) = fs::read_link(&path) else {
                continue;
            };
            if !target.starts_with(home_root) {
                continue;
            }
            match fs::metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    found.push(DanglingReference { path, target });
                }
                _ => {}
            }
        }
    }
    found.sort_by(|left, right| left.path.cmp(&right.path));
    found
}

/// Removes one dangling reference, re-checking every condition first: the
/// answer it was found by may be stale by the time a person acts on it.
pub fn remove_dangling(home: &UzeHome, reference: &DanglingReference) -> crate::Result<bool> {
    let claimed = std::collections::BTreeSet::new();
    let root = match reference.path.parent() {
        Some(parent) => vec![parent.to_path_buf()],
        None => return Ok(false),
    };
    if !dangling_references(home, &root, &claimed)
        .iter()
        .any(|found| found == reference)
    {
        return Ok(false);
    }
    fs::remove_file(&reference.path).map_err(|source| crate::UzeError::Write {
        path: reference.path.clone(),
        source,
    })?;
    Ok(true)
}

#[cfg(all(test, unix))]
mod dangling_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn world(label: &str) -> (PathBuf, UzeHome, PathBuf) {
        let root = uze_testkit::temp::scratch(label);
        let home = UzeHome::at(root.join("uze-home"));
        let discovery = root.join("agents/skills");
        fs::create_dir_all(&discovery).unwrap();
        fs::create_dir_all(home.root()).unwrap();
        (root, home, discovery)
    }

    #[test]
    fn a_reference_into_uze_home_whose_target_is_gone_is_found() {
        let (root, home, discovery) = world("dangling-found");
        let gone = home
            .root()
            .join("runtime/attachments/a-harness/skills/git/pr");
        crate::persistence::create_symlink(&gone, &discovery.join("git:pr")).unwrap();

        let found = dangling_references(&home, std::slice::from_ref(&discovery), &BTreeSet::new());

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].path, discovery.join("git:pr"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_reference_a_receipt_claims_is_never_reported() {
        let (root, home, discovery) = world("dangling-claimed");
        let gone = home
            .root()
            .join("runtime/attachments/a-harness/skills/git/pr");
        let link = discovery.join("git:pr");
        crate::persistence::create_symlink(&gone, &link).unwrap();

        let claimed: BTreeSet<PathBuf> = [link].into_iter().collect();

        assert!(dangling_references(&home, &[discovery], &claimed).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_reference_that_resolves_is_never_reported() {
        let (root, home, discovery) = world("dangling-resolves");
        let alive = home.root().join("store/plugins/ai/git");
        fs::create_dir_all(&alive).unwrap();
        crate::persistence::create_symlink(&alive, &discovery.join("git:pr")).unwrap();

        assert!(dangling_references(&home, &[discovery], &BTreeSet::new()).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_reference_pointing_outside_uze_home_is_never_reported() {
        let (root, home, discovery) = world("dangling-foreign");
        let theirs = root.join("somewhere-else/their-skill");
        crate::persistence::create_symlink(&theirs, &discovery.join("their:skill")).unwrap();

        assert!(dangling_references(&home, &[discovery], &BTreeSet::new()).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn removing_re_checks_before_it_deletes() {
        let (root, home, discovery) = world("dangling-remove");
        let gone = home
            .root()
            .join("runtime/attachments/a-harness/skills/git/pr");
        let link = discovery.join("git:pr");
        crate::persistence::create_symlink(&gone, &link).unwrap();
        let found = dangling_references(&home, std::slice::from_ref(&discovery), &BTreeSet::new());

        // The world changes under the answer: the target comes back.
        fs::create_dir_all(&gone).unwrap();
        assert!(
            !remove_dangling(&home, &found[0]).unwrap(),
            "a reference that resolves again is not removed"
        );
        assert!(link.is_symlink());

        fs::remove_dir_all(&gone).unwrap();
        assert!(remove_dangling(&home, &found[0]).unwrap());
        assert!(!link.is_symlink());
        fs::remove_dir_all(&root).unwrap();
    }
}
