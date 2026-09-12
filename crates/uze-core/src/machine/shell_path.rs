//! Explicit, reversible shell `PATH` integration for `UzeHome::shims_dir()`.
//!
//! Never invoked implicitly — only from `uze setup <harness>`, an action
//! the operator already explicitly ran, and only once that call actually
//! created a shim needing `PATH`. The edit lives inside a whole-line marked
//! block, structurally in the same spirit as `text_region`'s ownership
//! guarantee (exactly one begin marker, exactly one end marker, content
//! between verified before any rewrite) but not built on `text_region`
//! itself: shell scripts have no block-comment syntax, so `text_region`'s
//! `<!-- uze:begin ... -->` HTML-comment markers would corrupt a real
//! `.bashrc`/`.zshrc` if written verbatim. This module uses `#`-prefixed
//! whole-line markers instead — a shell comment in every shell this
//! targets.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{
    error::{Result, UzeError},
    persistence::write_atomic,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellRcTarget {
    pub kind: ShellKind,
    pub rc_file: PathBuf,
}

/// Which file bash actually reads, which is not the same question on both
/// platforms.
///
/// Bash reads `~/.bashrc` for an interactive non-login shell and
/// `~/.bash_profile` for a login one. On Linux a terminal window opens the
/// former; on macOS every terminal window is a login shell, so a `PATH` line
/// written to `.bashrc` there is a line the user never runs — the shims
/// would simply not be found, with a file on disk saying they should be.
///
/// zsh needs no such split: it reads `.zshrc` for any interactive shell,
/// login or not, which is why the zsh arm below names one file for both.
#[cfg(target_os = "macos")]
const BASH_RC_FILE: &str = ".bash_profile";
#[cfg(not(target_os = "macos"))]
const BASH_RC_FILE: &str = ".bashrc";

/// Detects the user's shell from `$SHELL` and the conventional rc file for
/// it under `home_dir`. `None` for anything not recognized (POSIX `sh`,
/// `dash`, `csh`, `$SHELL` unset, …) — deliberately conservative, never
/// guesses at an unfamiliar shell's syntax or startup file.
pub fn detect_shell_rc(home_dir: &Path) -> Option<ShellRcTarget> {
    let shell = env::var("SHELL").ok()?;
    let name = Path::new(&shell).file_name()?.to_str()?;
    match name {
        "bash" => Some(ShellRcTarget {
            kind: ShellKind::Bash,
            rc_file: home_dir.join(BASH_RC_FILE),
        }),
        "zsh" => Some(ShellRcTarget {
            kind: ShellKind::Zsh,
            rc_file: home_dir.join(".zshrc"),
        }),
        "fish" => Some(ShellRcTarget {
            kind: ShellKind::Fish,
            rc_file: home_dir.join(".config/fish/config.fish"),
        }),
        _ => None,
    }
}

const BEGIN: &str = "# >>> uze shims path >>>";
const END: &str = "# <<< uze shims path <<<";

fn desired_line(kind: ShellKind, shims_dir: &Path) -> String {
    match kind {
        ShellKind::Bash | ShellKind::Zsh => {
            format!("export PATH=\"{}:$PATH\"", shims_dir.display())
        }
        // fish has no `export`; `fish_add_path` is its idiomatic,
        // duplicate-safe equivalent.
        ShellKind::Fish => format!("fish_add_path {}", shims_dir.display()),
    }
}

/// Idempotently ensures a marked block containing exactly the right line
/// is the final PATH-affecting content in `target.rc_file`. Keeping UZE's
/// owned block last means a later vendor installer cannot shadow its shims
/// by prepending its own bin directory after this block. Returns `Ok(true)`
/// if it wrote a change, `Ok(false)` if the file already had exactly this
/// content in the required position. Refuses to touch the file (returns
/// `Err`) if it finds only one of the two markers — that shape means
/// something other than this function edited it last, and guessing at a fix
/// would risk corrupting content that isn't ours.
pub fn ensure_path_line(target: &ShellRcTarget, shims_dir: &Path) -> Result<bool> {
    let wanted = desired_line(target.kind, shims_dir);
    let existing = match fs::read_to_string(&target.rc_file) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(UzeError::Read {
                path: target.rc_file.clone(),
                source: error,
            });
        }
    };
    let lines: Vec<&str> = existing.lines().collect();
    let begin = lines.iter().position(|line| *line == BEGIN);
    let end = lines.iter().position(|line| *line == END);

    match (begin, end) {
        (Some(b), Some(e)) if e > b => {
            if e == b + 2 && lines[b + 1] == wanted && e + 1 == lines.len() {
                return Ok(false);
            }
            let mut rebuilt: Vec<&str> = lines[..b].to_vec();
            rebuilt.extend(&lines[e + 1..]);
            rebuilt.push(BEGIN);
            rebuilt.push(&wanted);
            rebuilt.push(END);
            write_atomic(
                &target.rc_file,
                format!("{}\n", rebuilt.join("\n")).as_bytes(),
            )?;
            Ok(true)
        }
        (None, None) => {
            let mut content = existing;
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(BEGIN);
            content.push('\n');
            content.push_str(&wanted);
            content.push('\n');
            content.push_str(END);
            content.push('\n');
            write_atomic(&target.rc_file, content.as_bytes())?;
            Ok(true)
        }
        _ => Err(UzeError::ManagedRegionDrift(target.rc_file.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_fresh_marked_block_into_an_empty_or_missing_rc_file() {
        let root = uze_testkit::temp::scratch("fresh");
        let target = ShellRcTarget {
            kind: ShellKind::Zsh,
            rc_file: root.join(".zshrc"),
        };
        let changed = ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap();
        assert!(changed);
        let content = fs::read_to_string(&target.rc_file).unwrap();
        assert!(content.contains(BEGIN));
        assert!(content.contains(END));
        assert!(content.contains("export PATH=\"/home/x/.uze/shims:$PATH\""));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn preserves_existing_content_around_the_block() {
        let root = uze_testkit::temp::scratch("preserve");
        let rc_file = root.join(".bashrc");
        fs::write(&rc_file, "alias ll='ls -la'\n").unwrap();
        let target = ShellRcTarget {
            kind: ShellKind::Bash,
            rc_file: rc_file.clone(),
        };
        ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap();
        let content = fs::read_to_string(&rc_file).unwrap();
        assert!(content.starts_with("alias ll='ls -la'\n"));
        assert!(content.contains(BEGIN));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_call_with_the_same_shims_dir_is_a_no_op() {
        let root = uze_testkit::temp::scratch("idempotent");
        let target = ShellRcTarget {
            kind: ShellKind::Bash,
            rc_file: root.join(".bashrc"),
        };
        assert!(ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap());
        assert!(!ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn moves_the_owned_block_after_later_path_exports() {
        let root = uze_testkit::temp::scratch("moves-to-end");
        let rc_file = root.join(".zshrc");
        let target = ShellRcTarget {
            kind: ShellKind::Zsh,
            rc_file: rc_file.clone(),
        };
        let shims = Path::new("/home/x/.uze/shims");
        ensure_path_line(&target, shims).unwrap();
        fs::write(
            &rc_file,
            format!(
                "{}export PATH=\"/home/x/.local/bin:$PATH\"\n",
                fs::read_to_string(&rc_file).unwrap()
            ),
        )
        .unwrap();

        assert!(ensure_path_line(&target, shims).unwrap());
        let content = fs::read_to_string(&rc_file).unwrap();
        assert!(content.starts_with("export PATH=\"/home/x/.local/bin:$PATH\"\n"));
        assert!(content.ends_with(&format!(
            "{BEGIN}\nexport PATH=\"/home/x/.uze/shims:$PATH\"\n{END}\n"
        )));
        assert!(!ensure_path_line(&target, shims).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_shims_dir_rewrites_only_the_marked_line() {
        let root = uze_testkit::temp::scratch("rewrite");
        let target = ShellRcTarget {
            kind: ShellKind::Bash,
            rc_file: root.join(".bashrc"),
        };
        ensure_path_line(&target, Path::new("/old/shims")).unwrap();
        let changed = ensure_path_line(&target, Path::new("/new/shims")).unwrap();
        assert!(changed);
        let content = fs::read_to_string(&target.rc_file).unwrap();
        assert!(content.contains("/new/shims"));
        assert!(!content.contains("/old/shims"));
        assert_eq!(content.matches(BEGIN).count(), 1);
        assert_eq!(content.matches(END).count(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_malformed_single_marker_is_left_untouched() {
        let root = uze_testkit::temp::scratch("malformed");
        let rc_file = root.join(".bashrc");
        fs::write(&rc_file, format!("{BEGIN}\nsomething odd\n")).unwrap();
        let target = ShellRcTarget {
            kind: ShellKind::Bash,
            rc_file: rc_file.clone(),
        };
        let result = ensure_path_line(&target, Path::new("/home/x/.uze/shims"));
        assert!(result.is_err());
        let content = fs::read_to_string(&rc_file).unwrap();
        assert_eq!(content, format!("{BEGIN}\nsomething odd\n"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fish_gets_fish_add_path_not_export() {
        let root = uze_testkit::temp::scratch("fish");
        let target = ShellRcTarget {
            kind: ShellKind::Fish,
            rc_file: root.join("config.fish"),
        };
        ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap();
        let content = fs::read_to_string(&target.rc_file).unwrap();
        assert!(content.contains("fish_add_path /home/x/.uze/shims"));
        assert!(!content.contains("export"));
        let _ = fs::remove_dir_all(&root);
    }

    /// `$SHELL` is process-global and `detect_shell_rc` reads it, so the
    /// three tests below cannot each set it to a different value and run at
    /// the same time. The testkit's scope both serializes them against every
    /// other env mutation in this binary and restores the previous value on
    /// drop — including on a panic.
    fn shell(value: &str) -> uze_testkit::env::ProcessEnvGuard<'static> {
        let mut scope = uze_testkit::env::scope();
        scope.set("SHELL", value);
        scope
    }

    #[test]
    fn unrecognized_shell_detects_to_none() {
        let _shell = shell("/bin/dash");
        let result = detect_shell_rc(Path::new("/home/x"));
        assert_eq!(result, None);
    }

    /// The file bash is told to read has to be the file bash reads. On macOS
    /// a terminal window is a login shell and never opens `.bashrc`, so
    /// writing there would leave the shims unreachable behind a file that
    /// says otherwise. Asserted per platform rather than skipped off Linux:
    /// the whole point is that the two answers differ.
    #[test]
    fn bash_is_pointed_at_the_file_this_platform_actually_reads() {
        let _shell = shell("/bin/bash");
        let target = detect_shell_rc(Path::new("/home/x")).expect("bash is recognized");
        let expected = if cfg!(target_os = "macos") {
            "/home/x/.bash_profile"
        } else {
            "/home/x/.bashrc"
        };
        assert_eq!(target.rc_file, Path::new(expected));
    }

    /// zsh, by contrast, reads `.zshrc` for any interactive shell — login or
    /// not — so it has one answer on every platform, and a change that gave
    /// it two would be a mistake this catches.
    #[test]
    fn zsh_reads_the_same_file_everywhere() {
        let _shell = shell("/bin/zsh");
        let target = detect_shell_rc(Path::new("/home/x")).expect("zsh is recognized");
        assert_eq!(target.rc_file, Path::new("/home/x/.zshrc"));
    }
}
