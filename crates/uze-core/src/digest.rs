//! The workspace's content digests, split by what the caller needs.
//!
//! **Identification** ([`fnv1a64`], [`short_hex`]) — a cache directory
//! name, the identity of a managed region. FNV-1a on purpose: these callers
//! ask "is this the same content as before", never "did somebody replace
//! it". It needs no dependency and its output is stable forever, unlike
//! `std`'s `DefaultHasher`, whose algorithm the standard library explicitly
//! does not promise to keep across versions.
//!
//! **Authentication** ([`tree_sha256`]) — the `integrity` a lock pins. Here
//! the question *is* "did somebody replace it", so a digest that is cheap to
//! collide is worse than none: it would state a guarantee it cannot keep.
//! This is the only reason `sha2` is a dependency, and the two must not be
//! confused at a call site — hence one module naming both, rather than a
//! helper wherever each is needed.
//!
//! Stability is the requirement that makes this shared: a digest that
//! changed between releases would silently orphan every artifact previously
//! named by it, and invalidate every `integrity` previously written.

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// The digest rendered as fixed-width lowercase hex — the form every caller
/// embeds in a name, so two call sites can never disagree on padding.
pub fn short_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// The authenticating digest of a directory tree, as `sha256:<hex>`.
///
/// Every file's path takes part, not only its bytes: moving a skill from
/// `skills/a/SKILL.md` to `skills/b/SKILL.md` changes what the package
/// *does*, so it must change the digest. Paths are sorted, and each field
/// is length-prefixed, so no arrangement of names and contents can be made
/// to produce the same stream as a different one. Directories contribute
/// nothing of their own — an empty one carries no behavior — and symlinks
/// are followed as the files they name, matching what the Store ingested.
pub fn tree_sha256(root: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};

    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for relative in &files {
        let spelled = relative.to_string_lossy();
        let contents = std::fs::read(root.join(relative))?;
        hasher.update(
            u64::try_from(spelled.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(spelled.as_bytes());
        hasher.update(
            u64::try_from(contents.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(&contents);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect_files(
    root: &std::path::Path,
    directory: &std::path::Path,
    into: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, into)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            into.push(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(label: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let root = uze_testkit::temp::scratch(label);
        for (relative, contents) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        root
    }

    #[test]
    fn the_same_tree_digests_the_same_whatever_order_it_was_written_in() {
        let one = tree("digest-order-a", &[("a.md", "A"), ("b/c.md", "C")]);
        let two = tree("digest-order-b", &[("b/c.md", "C"), ("a.md", "A")]);
        assert_eq!(tree_sha256(&one).unwrap(), tree_sha256(&two).unwrap());
        assert!(tree_sha256(&one).unwrap().starts_with("sha256:"));
    }

    #[test]
    fn changing_a_byte_changes_the_digest() {
        let before = tree("digest-bytes-a", &[("a.md", "A")]);
        let after = tree("digest-bytes-b", &[("a.md", "B")]);
        assert_ne!(tree_sha256(&before).unwrap(), tree_sha256(&after).unwrap());
    }

    /// Moving a skill changes what a package does, so it must change the
    /// digest even though every byte of content is the same.
    #[test]
    fn moving_a_file_changes_the_digest() {
        let before = tree("digest-move-a", &[("skills/a/SKILL.md", "s")]);
        let after = tree("digest-move-b", &[("skills/b/SKILL.md", "s")]);
        assert_ne!(tree_sha256(&before).unwrap(), tree_sha256(&after).unwrap());
    }

    /// The length prefixes exist for this: without them, a name ending
    /// where the next content begins could be rearranged into the same
    /// byte stream.
    #[test]
    fn adjacent_names_and_contents_cannot_be_rearranged_into_each_other() {
        let one = tree("digest-ambig-a", &[("ab", "cd")]);
        let two = tree("digest-ambig-b", &[("a", "bcd")]);
        assert_ne!(tree_sha256(&one).unwrap(), tree_sha256(&two).unwrap());
    }

    #[test]
    fn the_digest_is_stable_and_fixed_width() {
        assert_eq!(short_hex(b"").len(), 16);
        assert_eq!(short_hex(b"uze"), short_hex(b"uze"));
        assert_ne!(short_hex(b"uze"), short_hex(b"uze "));
    }
}
