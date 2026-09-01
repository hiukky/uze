//! The workspace's one stable, dependency-free content digest.
//!
//! FNV-1a rather than a cryptographic hash on purpose: every caller here
//! *identifies* content — a cache directory name, the identity of a managed
//! region — and authenticates none of it. It needs no dependency and its
//! output is stable forever, unlike `std`'s `DefaultHasher`, whose algorithm
//! the standard library explicitly does not promise to keep across versions.
//!
//! Stability is the requirement that makes this a shared module rather than
//! a helper per call site: a digest that changed between releases would
//! silently orphan every artifact previously named by it.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_is_stable_and_fixed_width() {
        assert_eq!(short_hex(b"").len(), 16);
        assert_eq!(short_hex(b"uze"), short_hex(b"uze"));
        assert_ne!(short_hex(b"uze"), short_hex(b"uze "));
    }
}
