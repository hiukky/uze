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
            rc_file: home_dir.join(".bashrc"),
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
/// exists in `target.rc_file`. Returns `Ok(true)` if it wrote a change,
/// `Ok(false)` if the file already had exactly this content. Refuses to
/// touch the file (returns `Err`) if it finds only one of the two markers —
/// that shape means something other than this function edited it last, and
/// guessing at a fix would risk corrupting content that isn't ours.
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
            if e == b + 2 && lines[b + 1] == wanted {
                return Ok(false);
            }
            let mut rebuilt: Vec<&str> = lines[..=b].to_vec();
            rebuilt.push(&wanted);
            rebuilt.extend(&lines[e..]);
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uze-shell-path-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn writes_a_fresh_marked_block_into_an_empty_or_missing_rc_file() {
        let root = scratch_dir("fresh");
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
        let root = scratch_dir("preserve");
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
        let root = scratch_dir("idempotent");
        let target = ShellRcTarget {
            kind: ShellKind::Bash,
            rc_file: root.join(".bashrc"),
        };
        assert!(ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap());
        assert!(!ensure_path_line(&target, Path::new("/home/x/.uze/shims")).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_shims_dir_rewrites_only_the_marked_line() {
        let root = scratch_dir("rewrite");
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
        let root = scratch_dir("malformed");
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
        let root = scratch_dir("fish");
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

    #[test]
    fn unrecognized_shell_detects_to_none() {
        let previous = env::var_os("SHELL");
        // SAFETY: test-only, restored immediately below.
        unsafe { env::set_var("SHELL", "/bin/dash") };
        let result = detect_shell_rc(Path::new("/home/x"));
        match previous {
            Some(value) => unsafe { env::set_var("SHELL", value) },
            None => unsafe { env::remove_var("SHELL") },
        }
        assert_eq!(result, None);
    }
}
