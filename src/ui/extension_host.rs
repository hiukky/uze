//! What the workspace client grants an extension.
//!
//! An extension holds no machine access of its own (see
//! `uze_extensions::Host`): it names what it needs, and this decides
//! whether to oblige. Today it always obliges, in this process, so nothing
//! observable changes — the point is that the grant now has a single, named
//! place, which is where a real capability model would go if extensions
//! were ever authored elsewhere.
//!
//! Most of it reads. Two methods write — the file explorer's save and
//! delete — and they are the reason this file is the one place to look
//! when the question is "what can an extension actually do to this
//! machine". Both are narrow on purpose: a write replaces a file that
//! already exists, and a delete removes a file and never a directory.

use std::path::Path;

/// The one Git question this host answers from memory rather than by
/// running Git. Spelled once, so the interception below and the reason for
/// it cannot drift apart.
const SHOW_TOPLEVEL_ARGS: [&str; 2] = ["rev-parse", "--show-toplevel"];

/// How much of a file this host will hand an extension.
///
/// Generous for anything a person reads and small enough that the read
/// itself is never what they notice — see [`WorkspaceHost::read_file`]
/// for what the bound is protecting against.
const READABLE_FILE_LIMIT: u64 = 2 * 1024 * 1024;

/// What a file that cannot be shown as text is called, wherever the
/// reason is the filesystem's rather than ours. Said once so the surface
/// and this grant cannot describe the same state differently.
const UNREADABLE: &str = "not readable as text";

/// The workspace client's grant. Zero-sized: the capabilities are the
/// host's own, not per-extension state.
pub(crate) struct WorkspaceHost;

impl uze_extensions::Host for WorkspaceHost {
    /// Through `uze-git`'s read path, so an overlay refreshing every few
    /// seconds cannot contend with an agent writing in a sibling checkout.
    ///
    /// Exit `1` is an answer rather than a failure: `git diff` uses it for
    /// "there are differences", which is the ordinary case here.
    fn git(&self, root: &Path, args: &[&str]) -> Result<String, String> {
        // "Which working tree is this" cannot change under a path that is
        // still there, and the change badge asks it on every refresh — a
        // quarter of the Git processes a session spawns were this one
        // question. `uze-git` remembers it; everything else is asked of Git
        // as written, because everything else can have changed since.
        if args == [SHOW_TOPLEVEL_ARGS[0], SHOW_TOPLEVEL_ARGS[1]] {
            return uze_git::repository::root(root).map(|found| format!("{}\n", found.display()));
        }
        uze_git::read(root, args)
            .map_err(|error| error.to_string())?
            .or_exit(1)
    }

    /// Bounded, because the gesture behind it is a single click on a row
    /// in a tree. Everything downstream of the read keeps a copy —
    /// syntect's spans per line, then the buffer's own — so the file's
    /// size is paid three times over, and a checked-in fixture, a log or
    /// a minified bundle that is one enormous line is a multi-gigabyte
    /// allocation and a highlighting pass measured in minutes from a
    /// click nobody would expect to cost anything.
    ///
    /// Read through a `take` rather than checked with `metadata` first:
    /// what the cap has to bound is how much lands in memory, and a
    /// length read separately from the bytes is a different question.
    fn read_file(&self, path: &Path) -> Result<String, String> {
        use std::io::Read;

        let file = std::fs::File::open(path).map_err(|_| UNREADABLE.to_owned())?;
        let mut text = String::new();
        let read = file
            // One byte past the cap: a file exactly at it still opens,
            // and anything larger is known to be larger without reading
            // the rest of it.
            .take(READABLE_FILE_LIMIT + 1)
            .read_to_string(&mut text)
            .map_err(|_| UNREADABLE.to_owned())?;
        if read as u64 > READABLE_FILE_LIMIT {
            return Err(format!(
                "too large to open here — over {} MiB",
                READABLE_FILE_LIMIT / (1024 * 1024)
            ));
        }
        Ok(text)
    }

    /// Directories first, then files, each half by name — the order the
    /// contract promises, resolved here rather than in the extension
    /// because `read_dir` answers in whatever order the filesystem
    /// happens to hold, and a tree that reorders itself between two
    /// listings of the same directory is a tree nobody can click in.
    fn list_dir(&self, path: &Path) -> Result<Vec<uze_extensions::DirEntry>, String> {
        let mut entries: Vec<uze_extensions::DirEntry> = std::fs::read_dir(path)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| uze_extensions::DirEntry {
                // `file_type` rather than a follow: a symlink to a
                // directory is still browsable, and one that dangles is
                // reported as the file it is not rather than as an error
                // that takes the whole listing with it.
                directory: entry.file_type().is_ok_and(|kind| kind.is_dir()),
                name: entry.file_name().to_string_lossy().into_owned(),
            })
            .collect();
        entries.sort();
        Ok(entries)
    }

    /// Refuses a path that is not already a file, so "save" can only ever
    /// mean "save this file" — never "create whatever this string names".
    fn write_file(&self, path: &Path, contents: &str) -> Result<(), String> {
        if !path.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        std::fs::write(path, contents).map_err(|error| error.to_string())
    }

    /// Files only. A recursive removal is a different act from the one
    /// the gesture that reaches here describes, and the difference
    /// between them is measured in how much is gone afterwards.
    fn delete_file(&self, path: &Path) -> Result<(), String> {
        if !path.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        std::fs::remove_file(path).map_err(|error| error.to_string())
    }

    /// Counted off a buffered read rather than the whole file: the change
    /// badge asks this of every untracked file every refresh, and an
    /// untracked directory a project never gitignored is megabytes read
    /// into memory on a timer for a line count.
    fn count_lines(&self, path: &Path) -> u32 {
        use std::io::{BufReader, Read};
        let Ok(file) = std::fs::File::open(path) else {
            return 0;
        };
        let mut reader = BufReader::new(file);
        let mut buffer = [0u8; 16 * 1024];
        let mut lines: u32 = 0;
        let mut last = None;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    lines = lines.saturating_add(
                        buffer[..read].iter().filter(|byte| **byte == b'\n').count() as u32,
                    );
                    last = buffer[..read].last().copied();
                }
                Err(_) => return 0,
            }
        }
        // `str::lines` yields nothing for an empty file and does not add a
        // line for a trailing newline, so only unterminated content counts
        // one more than its newlines.
        match last {
            None => 0,
            Some(b'\n') => lines,
            Some(_) => lines.saturating_add(1),
        }
    }

    fn display_path(&self, path: &Path) -> String {
        crate::ui::display_project_path(path)
    }

    /// The active theme's, so highlighted content is drawn for the same
    /// background the chrome around it is.
    fn syntax_theme(&self) -> String {
        uze_theme::active().syntax_theme().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use uze_extensions::Host;

    use super::WorkspaceHost;

    /// The two methods that change the machine, and the two things they
    /// refuse. Both refusals are the whole difference between "an
    /// extension edits the files a project has" and "an extension writes
    /// anywhere it can name".
    #[test]
    fn the_write_grant_is_narrower_than_the_filesystem() {
        let directory = scratch("uze-write-grant");
        let file = directory.join("a.txt");
        std::fs::write(&file, "before\n").unwrap();
        let nested = directory.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        assert!(WorkspaceHost.write_file(&file, "after\n").is_ok());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "after\n");

        assert!(
            WorkspaceHost
                .write_file(&directory.join("invented.txt"), "x")
                .is_err(),
            "saving never creates a file that was not there"
        );
        assert!(
            WorkspaceHost.write_file(&nested, "x").is_err(),
            "a directory is not a file to be overwritten"
        );
        assert!(
            WorkspaceHost.delete_file(&nested).is_err(),
            "a directory is never removed by the gesture that removes a file"
        );
        assert!(nested.is_dir(), "and it is still there afterwards");

        assert!(WorkspaceHost.delete_file(&file).is_ok());
        assert!(!file.exists());
        std::fs::remove_dir_all(&directory).ok();
    }

    /// A click on a row in a tree never reads more than the grant allows,
    /// and says so rather than answering the same "not readable as text"
    /// a binary gets — the two are different things to do about it.
    #[test]
    fn opening_a_file_is_bounded_by_what_the_grant_allows() {
        let directory = scratch("uze-read-cap");
        let at_the_cap = directory.join("at-the-cap");
        std::fs::write(&at_the_cap, "x".repeat(super::READABLE_FILE_LIMIT as usize)).unwrap();
        let over_the_cap = directory.join("over-the-cap");
        std::fs::write(
            &over_the_cap,
            "x".repeat(super::READABLE_FILE_LIMIT as usize + 1),
        )
        .unwrap();

        assert_eq!(
            WorkspaceHost.read_file(&at_the_cap).map(|text| text.len()),
            Ok(super::READABLE_FILE_LIMIT as usize),
            "a file exactly at the cap still opens, whole"
        );
        let refused = WorkspaceHost
            .read_file(&over_the_cap)
            .expect_err("one byte over is refused");
        assert!(
            refused.contains("too large"),
            "the refusal names the size rather than the file's kind: {refused}"
        );
        assert_eq!(
            WorkspaceHost.read_file(&directory.join("absent")),
            Err(super::UNREADABLE.to_owned()),
            "a file that is not there is the state a view draws, not a path leak"
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    /// Directories before files, each half by name — a tree that reorders
    /// itself between two listings is a tree nobody can click in.
    #[test]
    fn a_listing_is_ordered_the_same_way_every_time() {
        let directory = scratch("uze-listing-order");
        std::fs::create_dir_all(directory.join("zeta")).unwrap();
        std::fs::create_dir_all(directory.join("alpha")).unwrap();
        std::fs::write(directory.join("b.txt"), "").unwrap();
        std::fs::write(directory.join("a.txt"), "").unwrap();

        let names: Vec<String> = WorkspaceHost
            .list_dir(&directory)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["alpha", "zeta", "a.txt", "b.txt"]);
        assert!(
            WorkspaceHost.list_dir(&directory.join("absent")).is_err(),
            "a directory that is not there is an error, not an empty tree"
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    /// A scratch directory of this test run's own.
    fn scratch(prefix: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// The streaming count must answer exactly what the contract's default
    /// answers — `str::lines` — or the badge's totals move the day a host
    /// stops materialising the file. The cases that differ are the edges:
    /// an empty file, content with no trailing newline, and a file larger
    /// than the read buffer.
    #[test]
    fn counting_lines_without_the_file_in_memory_matches_str_lines() {
        let directory = std::env::temp_dir().join(format!(
            "uze-count-lines-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).unwrap();

        let big = "x".repeat(100) + "\n";
        for (name, contents) in [
            ("empty", String::new()),
            ("just-a-newline", "\n".to_owned()),
            ("no-trailing-newline", "a\nb".to_owned()),
            ("trailing-newline", "a\nb\n".to_owned()),
            ("one-unterminated-line", "solo".to_owned()),
            ("past-the-buffer", big.repeat(1000)),
        ] {
            let path = directory.join(name);
            std::fs::write(&path, &contents).unwrap();
            assert_eq!(
                WorkspaceHost.count_lines(&path),
                contents.lines().count() as u32,
                "{name} counted differently from str::lines"
            );
        }

        assert_eq!(
            WorkspaceHost.count_lines(&directory.join("absent")),
            0,
            "a file that cannot be read counts as nothing, never as an error"
        );
        std::fs::remove_dir_all(&directory).ok();
    }
}
