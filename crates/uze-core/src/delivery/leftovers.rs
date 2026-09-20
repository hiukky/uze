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
