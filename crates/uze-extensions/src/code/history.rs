//! What the checkout has already recorded: its recent commits, what one
//! of them says, and how much of it is still ahead of where the work is
//! going.
//!
//! Read for the sidebar's timeline section rather than for the overlay,
//! which is why it sits apart from [`super::status`] — the two answer
//! about different sides of the same checkout and share nothing but the
//! way they reach Git.

use std::{collections::BTreeSet, path::Path};

use super::{changes::parse_numstat, current_branch, repository_root, run_git};
use crate::{
    Host,
    view::{Role, Section, SectionRow, Span},
};

/// One commit of a checkout's [`Timeline`], newest first in the list it
/// belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    /// How long ago, in the compact form a narrow column can afford —
    /// `3h`, `2d`, `now` — rather than Git's own "3 hours ago" (see
    /// [`compact_age`]).
    pub age: String,
    /// No equivalent of it yet in what the checkout is measured against —
    /// the delivery target from any other branch, the upstream from the
    /// target itself (see [`comparison_base`]). What a delivery or a push
    /// would still move.
    pub ahead: bool,
}

/// The recent history of one checkout and the branch it is on: what the
/// sidebar's timeline section draws. Data only, like [`change_summary`] —
/// the host lays the rows out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timeline {
    pub branch: String,
    /// Newest first; the first one is `HEAD`.
    pub commits: Vec<Commit>,
}

/// The newest `limit` commits reachable from `HEAD` in the checkout `cwd`
/// is in, each marked whether it is still ahead of what the checkout is
/// measured against (`target` being the repository's delivery target, if
/// it has one — see [`comparison_base`]). `None` outside a repository,
/// without a usable `git`, and in a repository with no commits yet —
/// every case where there is no history to show, so the caller can omit
/// the section entirely rather than draw an empty one.
pub fn timeline(
    host: &dyn Host,
    cwd: &Path,
    limit: usize,
    target: Option<&str>,
) -> Option<Timeline> {
    let root = repository_root(host, cwd).ok()?;
    let count = format!("--max-count={limit}");
    let log = run_git(host, &root, &["log", &count, LOG_FORMAT]).ok()?;
    let mut commits = parse_log(&log);
    if commits.is_empty() {
        return None;
    }
    let branch = current_branch(host, &root);
    if let Some(base) = comparison_base(host, &root, &branch, target) {
        // By patch, not by ancestry: a delivery rebases the branch onto
        // the target, so what landed there carries another hash, and by
        // ancestry every delivered commit would still read as ahead.
        // `--cherry-pick` drops the ones the base holds an equivalent of;
        // a commit reworked on the way in stays ahead, which is the truth.
        let ahead = run_git(
            host,
            &root,
            &[
                "rev-list",
                "--cherry-pick",
                "--right-only",
                &count,
                &format!("{base}...HEAD"),
            ],
        )
        .map(|hashes| {
            hashes
                .lines()
                .map(str::trim)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
        for commit in &mut commits {
            commit.ahead = ahead.contains(&commit.hash);
        }
    }
    Some(Timeline { branch, commits })
}

/// The sidebar section a checkout's [`Timeline`] draws as.
///
/// Everything the host used to decide for this extension: the fold
/// marker's meaning, which hue a commit's dot wears, which text gives way
/// when the column is narrow. The host still owns every rectangle — see
/// [`crate::view::Section`].
///
/// `collapsed` and `scroll` come back from the host because the gestures
/// that change them are the host's (a click on the header, a wheel over
/// the rows); the extension is told what they are, and says what the
/// section looks like as a result.
pub fn timeline_section(timeline: &Timeline, collapsed: bool, scroll: usize) -> Section {
    Section {
        title: "timeline".to_owned(),
        caption: Span::new(timeline.branch.clone(), Role::Dim),
        collapsed,
        resizable: true,
        scroll,
        rows: timeline
            .commits
            .iter()
            .enumerate()
            .map(|(index, commit)| {
                let head = index == 0;
                SectionRow {
                    // The hue is the commit's standing, the ring is
                    // `HEAD`: the badge hue for what is still ahead of
                    // the base — what a delivery or a push would move —
                    // and the target's own warning hue for what has
                    // landed in it.
                    marker: Span::new(
                        if head { "\u{25c9}" } else { "\u{25cf}" },
                        if commit.ahead {
                            Role::Info
                        } else {
                            Role::Warning
                        },
                    ),
                    name: Span::new(
                        commit.subject.clone(),
                        if head { Role::Inactive } else { Role::Dim },
                    ),
                    trailing: Span::new(commit.age.clone(), Role::Faint),
                }
            })
            .collect(),
    }
}

/// What "ahead" is measured against: the delivery target from any other
/// branch, and the upstream from the target itself or where no target is
/// known — the two the sidebar's captions already measure a branch by. A
/// target with no local ref falls back to the upstream too. `None` when
/// neither exists, and nothing is ahead of nothing.
pub(super) fn comparison_base(
    host: &dyn Host,
    root: &Path,
    branch: &str,
    target: Option<&str>,
) -> Option<String> {
    target
        .filter(|target| *target != branch)
        .into_iter()
        .chain(std::iter::once("@{upstream}"))
        .find(|candidate| ref_exists(host, root, candidate))
        .map(str::to_owned)
}

/// `--quiet --verify` exits `1` for a name that resolves to nothing, which
/// the host reports as an empty answer rather than a failure.
pub(super) fn ref_exists(host: &dyn Host, root: &Path, name: &str) -> bool {
    run_git(
        host,
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .is_ok_and(|stdout| !stdout.trim().is_empty())
}

/// Unit-separated so a subject holding a tab, a pipe or any other
/// punctuation a person might type still splits into exactly three fields.
/// The full hash, since it is matched against `rev-list` rather than
/// shown.
pub(super) const LOG_FORMAT: &str = "--format=%H\x1f%s\x1f%cr";
pub(super) const LOG_FIELD_SEPARATOR: char = '\u{1f}';

/// Everything the sidebar's commit popup says about one commit: what
/// `git show` knows without its patch, and the shape of that patch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitDetail {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    /// Git's own relative date, unabridged — the popup has the room the
    /// timeline row has not.
    pub age: String,
    /// When it was committed, `YYYY-MM-DD HH:MM` in the committer's zone.
    pub date: String,
    /// The branches and tags at this commit, as bare names: the popup
    /// pins them as labels, and `HEAD ->` or `tag:` is not a name.
    pub refs: Vec<String>,
    pub subject: String,
    /// The message past its subject, trailing blank lines dropped; empty
    /// for a one-line message.
    pub body: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

/// The full account of `hash` in the checkout `cwd` is in, or `None` when
/// there is no such commit to give one of.
pub fn commit_detail(host: &dyn Host, cwd: &Path, hash: &str) -> Option<CommitDetail> {
    let root = repository_root(host, cwd).ok()?;
    let shown = run_git(
        host,
        &root,
        &["show", "--no-patch", SHOW_DATE, SHOW_FORMAT, hash],
    )
    .ok()?;
    let mut detail = parse_show(&shown)?;
    let numstat = run_git(host, &root, &["show", "--numstat", "--format=", hash]).ok()?;
    let (insertions, deletions) = parse_numstat(&numstat);
    detail.files_changed = numstat
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u32;
    detail.insertions = insertions;
    detail.deletions = deletions;
    Some(detail)
}

pub(super) const SHOW_DATE: &str = "--date=format:%Y-%m-%d %H:%M";
/// The body last: it is the one field that spans lines, so everything
/// before it splits on the separator and it keeps whatever remains.
pub(super) const SHOW_FORMAT: &str = "--format=%H\x1f%h\x1f%an\x1f%cr\x1f%cd\x1f%D\x1f%s\x1f%b";

pub(super) fn parse_show(output: &str) -> Option<CommitDetail> {
    let mut fields = output.splitn(8, LOG_FIELD_SEPARATOR);
    let hash = fields.next()?.trim();
    if hash.is_empty() {
        return None;
    }
    let short_hash = fields.next()?.trim().to_owned();
    let author = fields.next()?.trim().to_owned();
    let age = fields.next()?.trim().to_owned();
    let date = fields.next()?.trim().to_owned();
    let refs = parse_refs(fields.next()?);
    let subject = fields.next()?.trim().to_owned();
    let body = fields.next().unwrap_or_default().trim().to_owned();
    Some(CommitDetail {
        hash: hash.to_owned(),
        short_hash,
        author,
        age,
        date,
        refs,
        subject,
        body,
        files_changed: 0,
        insertions: 0,
        deletions: 0,
    })
}

/// `%D` — `HEAD -> main, origin/main, tag: v1` — as the names alone. A
/// detached `HEAD` names nothing and is dropped.
pub(super) fn parse_refs(decorations: &str) -> Vec<String> {
    decorations
        .split(',')
        .map(str::trim)
        .map(|reference| {
            reference
                .strip_prefix("HEAD -> ")
                .or_else(|| reference.strip_prefix("tag: "))
                .unwrap_or(reference)
        })
        .filter(|name| !name.is_empty() && *name != "HEAD")
        .map(str::to_owned)
        .collect()
}

pub(super) fn parse_log(output: &str) -> Vec<Commit> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(LOG_FIELD_SEPARATOR);
            let hash = fields.next()?.trim();
            let subject = fields.next()?.trim();
            let age = fields.next()?.trim();
            (!hash.is_empty()).then(|| Commit {
                hash: hash.to_owned(),
                subject: subject.to_owned(),
                age: compact_age(age),
                ahead: false,
            })
        })
        .collect()
}

/// Git's relative date (`%cr`, "3 hours ago") shortened to what a column
/// beside a subject can afford: the leading count and one letter for its
/// unit, `now` for anything under a minute. Git's compound form ("1 year,
/// 2 months ago") keeps only its leading part, since the tail is precision
/// the column has no room for. Anything else is handed back as it came,
/// so a wording this does not know is still shown rather than dropped.
pub(super) fn compact_age(relative: &str) -> String {
    let mut words = relative.split_whitespace();
    let (Some(count), Some(unit)) = (words.next(), words.next()) else {
        return relative.to_owned();
    };
    let unit = unit.trim_end_matches(',');
    let suffix = match unit.trim_end_matches('s') {
        "second" => return "now".to_owned(),
        "minute" => "m",
        "hour" => "h",
        "day" => "d",
        "week" => "w",
        "month" => "mo",
        "year" => "y",
        _ => return relative.to_owned(),
    };
    format!("{count}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_date_compacts_to_its_count_and_unit() {
        assert_eq!(compact_age("5 seconds ago"), "now");
        assert_eq!(compact_age("1 minute ago"), "1m");
        assert_eq!(compact_age("3 hours ago"), "3h");
        assert_eq!(compact_age("2 days ago"), "2d");
        assert_eq!(compact_age("6 weeks ago"), "6w");
        assert_eq!(compact_age("4 months ago"), "4mo");
        assert_eq!(compact_age("1 year, 2 months ago"), "1y");
        assert_eq!(compact_age("in the future"), "in the future");
    }
    #[test]
    fn a_log_line_splits_on_the_unit_separator_whatever_the_subject_holds() {
        let output = "abc1234\u{1f}fix: a | b\tc\u{1f}3 hours ago\ndef5678\u{1f}feat: first\u{1f}2 days ago\n";
        let commits = parse_log(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].subject, "fix: a | b\tc");
        assert_eq!(commits[0].age, "3h");
        assert_eq!(commits[1].subject, "feat: first");
    }
    #[test]
    fn decorations_become_bare_names() {
        assert_eq!(
            parse_refs("HEAD -> main, origin/main, tag: v1"),
            ["main", "origin/main", "v1"]
        );
        assert_eq!(parse_refs("HEAD"), Vec::<String>::new());
        assert_eq!(parse_refs(""), Vec::<String>::new());
    }
    #[test]
    fn a_shown_commit_keeps_its_body_whole() {
        let shown = "abc\u{1f}abc1234\u{1f}Ada\u{1f}3 hours ago\u{1f}2026-09-03 19:39\u{1f}HEAD -> main\u{1f}feat: first\u{1f}One paragraph.\n\nAnother, with a | pipe.\n\n";
        let detail = parse_show(shown).expect("well formed");
        assert_eq!(detail.short_hash, "abc1234");
        assert_eq!(detail.author, "Ada");
        assert_eq!(detail.refs, ["main"]);
        assert_eq!(detail.subject, "feat: first");
        assert_eq!(detail.body, "One paragraph.\n\nAnother, with a | pipe.");
        assert!(parse_show("").is_none());
    }
}

#[cfg(test)]
mod repository_tests {
    use super::*;

    /// The same grant the workspace client makes, so these exercise the
    /// real path rather than a stub. A fake would be the right tool for
    /// testing *the view*; these test what the view reads.
    struct TestHost;

    impl Host for TestHost {
        fn git(&self, root: &Path, args: &[&str]) -> Result<String, String> {
            uze_git::read(root, args)
                .map_err(|error| error.to_string())?
                .or_exit(1)
        }

        fn read_file(&self, path: &Path) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }

        fn display_path(&self, path: &Path) -> String {
            path.display().to_string()
        }

        fn syntax_theme(&self) -> String {
            crate::shared::highlight::FALLBACK_SYNTAX_THEME.to_owned()
        }

        /// Never reached: what these read, they read through `git`.
        fn list_dir(&self, _path: &Path) -> Result<Vec<crate::DirEntry>, String> {
            unreachable!("the changes half lists no directory of its own")
        }

        fn write_file(&self, _path: &Path, _contents: &str) -> Result<(), String> {
            unreachable!("reading what changed writes nothing")
        }

        fn delete_file(&self, _path: &Path) -> Result<(), String> {
            unreachable!("reading what changed deletes nothing")
        }
    }

    /// The popup's account is the commit's own: who, when, what it said,
    /// and how much it touched.
    #[test]
    fn the_detail_of_a_real_commit_counts_what_it_touched() {
        let repository = uze_testkit::git::Repository::new("git-commit-detail-test");
        let root = repository.root().to_path_buf();
        std::fs::write(root.join("a.rs"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("b.rs"), "three\n").unwrap();
        repository.git(&["add", "."]);
        repository.git(&[
            "commit",
            "--quiet",
            "-m",
            "feat: two files\n\nWhy they were added.",
        ]);
        let head = repository.head();

        let detail = commit_detail(&TestHost, &root, &head).expect("the commit exists");
        assert_eq!(detail.hash, head);
        assert_eq!(detail.subject, "feat: two files");
        assert_eq!(detail.body, "Why they were added.");
        assert!(!detail.author.is_empty());
        assert!(detail.age.ends_with("ago"), "{}", detail.age);
        assert_eq!(
            detail.date.len(),
            "2026-09-03 19:39".len(),
            "{}",
            detail.date
        );
        assert_eq!(detail.refs, [repository.branch()]);
        assert_eq!(
            (detail.files_changed, detail.insertions, detail.deletions),
            (2, 3, 0)
        );
        assert!(commit_detail(&TestHost, &root, "0000000").is_none());
    }
    /// The timeline is the checkout's own history, newest first, and a
    /// repository with nothing committed yet has no timeline at all —
    /// the section is omitted, not drawn empty.
    #[test]
    fn the_timeline_lists_a_real_repositorys_commits_newest_first() {
        let repository = uze_testkit::git::Repository::empty("git-timeline-test");
        let root = repository.root().to_path_buf();
        assert_eq!(timeline(&TestHost, &root, 5, None), None);

        repository.commit_file("one.rs", "fn one() {}\n");
        repository.commit_file("two.rs", "fn two() {}\n");
        repository.commit_file("three.rs", "fn three() {}\n");

        let timeline = timeline(&TestHost, &root, 2, None).expect("history exists");
        assert!(!timeline.branch.is_empty());
        let subjects: Vec<&str> = timeline
            .commits
            .iter()
            .map(|commit| commit.subject.as_str())
            .collect();
        assert_eq!(subjects.len(), 2, "bounded by the limit: {subjects:?}");
        assert!(subjects[0].contains("three"), "{subjects:?}");
        assert!(subjects[1].contains("two"), "{subjects:?}");
        assert!(
            timeline
                .commits
                .iter()
                .all(|commit| commit.hash.len() == 40)
        );
        assert!(timeline.commits.iter().all(|commit| commit.age == "now"));
        assert!(
            timeline.commits.iter().all(|commit| !commit.ahead),
            "nothing is ahead of nothing: no target, no upstream"
        );
    }
    /// From a branch of its own, what is ahead is what the target lacks;
    /// on the target itself, what the upstream lacks — and a target the
    /// checkout is on is not measured against itself.
    #[test]
    fn commits_past_the_base_are_ahead_and_the_rest_have_landed() {
        let repository = uze_testkit::git::Repository::new("git-timeline-ahead-test");
        let root = repository.root().to_path_buf();
        let target = repository.branch();
        repository.commit_file("landed.rs", "");
        repository.git(&["checkout", "--quiet", "-b", "feature"]);
        repository.commit_file("first.rs", "");
        repository.commit_file("second.rs", "");

        let flags = |timeline: Timeline| -> Vec<(String, bool)> {
            timeline
                .commits
                .into_iter()
                .map(|commit| (commit.subject, commit.ahead))
                .collect()
        };
        let on_feature = timeline(&TestHost, &root, 10, Some(&target)).expect("history");
        assert_eq!(
            flags(on_feature),
            [
                ("second.rs".to_owned(), true),
                ("first.rs".to_owned(), true),
                ("landed.rs".to_owned(), false),
                ("README.md".to_owned(), false),
            ]
        );

        // Delivered by rebase, the target holds `first.rs` under another
        // hash — and the branch, never rebased since, still holds its own.
        // By patch it has landed; only `second.rs` is still ahead.
        let first = repository.git(&["rev-parse", "HEAD~1"]);
        repository.git(&["checkout", "--quiet", &target]);
        repository.git(&["cherry-pick", "--quiet", &first]);
        repository.git(&["checkout", "--quiet", "feature"]);
        let after_delivery = timeline(&TestHost, &root, 10, Some(&target)).expect("history");
        assert_eq!(
            flags(after_delivery)
                .into_iter()
                .filter(|(_, ahead)| *ahead)
                .map(|(subject, _)| subject)
                .collect::<Vec<_>>(),
            ["second.rs"]
        );

        // The same branch measured by its upstream instead, target unknown.
        repository.git(&["branch", "--quiet", "--set-upstream-to", &target]);
        let by_upstream = timeline(&TestHost, &root, 10, None).expect("history");
        assert_eq!(
            flags(by_upstream)
                .iter()
                .filter(|(_, ahead)| *ahead)
                .count(),
            1
        );

        repository.git(&["checkout", "--quiet", &target]);
        let on_target = timeline(&TestHost, &root, 10, Some(&target)).expect("history");
        assert!(
            flags(on_target).iter().all(|(_, ahead)| !ahead),
            "the target has no upstream, so nothing is ahead"
        );
    }
}
