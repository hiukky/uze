//! What the workspace client grants an extension.
//!
//! An extension holds no machine access of its own (see
//! `uze_extensions::Host`): it names what it needs, and this decides
//! whether to oblige. Today it always obliges, in this process, so nothing
//! observable changes — the point is that the grant now has a single, named
//! place, which is where a real capability model would go if extensions
//! were ever authored elsewhere.
//!
//! Read-only throughout. Nothing reachable from here writes anything.

use std::path::Path;

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
        uze_git::read(root, args)
            .map_err(|error| error.to_string())?
            .or_exit(1)
    }

    fn read_file(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
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
