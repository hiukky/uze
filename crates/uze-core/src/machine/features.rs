//! What this build offers, and what it is still deciding.
//!
//! A feature here is a surface of the product that exists in the code but
//! is not finished enough to be part of what a person downloads. The
//! alternative it replaces is the one every project reaches for first —
//! leaving the half-built screen in and calling it Beta — which makes
//! every release a promise nobody meant to make.
//!
//! Resolved at runtime rather than through a Cargo feature, for three
//! reasons. The vocabularies it gates (a route, a scope, an action) are
//! exhaustive enums with tests that walk them, and conditional variants
//! would put `cfg` in every one of those walks. A build without the Cargo
//! feature would stop compiling the code it hides, so CI would stop
//! testing it — a flag whose purpose is to defer a decision must not
//! delete the evidence the decision needs. And a developer should see
//! their own unfinished work without passing a flag to build it.
//!
//! So: **off in a release build, on in a development one**, and
//! `UZE_FEATURES` overrides either way. `cargo install` (what `make
//! install` runs) and the release matrix are release builds, so what a
//! person downloads never carries one of these.

use std::{collections::BTreeSet, env, sync::OnceLock};

/// The environment variable that overrides this build's defaults: a
/// comma-separated list of ids, each optionally prefixed with `-` to turn
/// one off. `UZE_FEATURES=profiles` in a downloaded build, `-profiles` in
/// a development one. An id this build does not know is ignored, so a
/// shell profile written for a newer uze is not an error.
pub const FEATURES_ENV: &str = "UZE_FEATURES";

/// One surface this build has not committed to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Feature {
    /// The management modal's Profiles screen: the preference axes
    /// (autonomy, sandbox, model) UZE applies to a harness. The domain
    /// behind it is real; what is undecided is the surface — see the
    /// screen's own Beta badge, which this replaces.
    Profiles,
}

/// Every feature this build knows, for a listing that has to be complete.
pub const ALL_FEATURES: &[Feature] = &[Feature::Profiles];

impl Feature {
    /// The id the environment variable names it by.
    pub fn id(self) -> &'static str {
        match self {
            Feature::Profiles => "profiles",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Feature::Profiles => "The Profiles screen: preference axes applied to a harness",
        }
    }

    pub fn parse(id: &str) -> Option<Feature> {
        ALL_FEATURES
            .iter()
            .copied()
            .find(|feature| feature.id() == id)
    }
}

/// Whether this build offers `feature`.
///
/// Answered once per process: a surface that appeared halfway through a
/// session because the environment changed under it would be a stranger
/// bug than the one the flag exists to avoid, and this is read on every
/// frame that draws a list of screens.
pub fn enabled(feature: Feature) -> bool {
    static OFFERED: OnceLock<BTreeSet<Feature>> = OnceLock::new();
    OFFERED
        .get_or_init(|| {
            let overrides = env::var(FEATURES_ENV).ok();
            ALL_FEATURES
                .iter()
                .copied()
                .filter(|feature| resolve(*feature, overrides.as_deref()))
                .collect()
        })
        .contains(&feature)
}

/// The answer, given what the environment says — the whole rule, and
/// testable without an environment.
fn resolve(feature: Feature, overrides: Option<&str>) -> bool {
    // A development build is where unfinished work belongs, and its
    // author should not have to ask for it.
    let mut answer = cfg!(debug_assertions);
    for entry in overrides.unwrap_or_default().split(',') {
        let entry = entry.trim();
        let (wanted, id) = match entry.strip_prefix('-') {
            Some(id) => (false, id.trim()),
            None => (true, entry),
        };
        if Feature::parse(id) == Some(feature) {
            answer = wanted;
        }
    }
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_person_downloads_carries_no_unfinished_surface() {
        // The claim is about the build, not about the test: a release
        // build answers false and a development one answers true, and
        // this suite runs in both.
        assert_eq!(resolve(Feature::Profiles, None), cfg!(debug_assertions));
    }

    #[test]
    fn the_environment_answers_either_way() {
        assert!(resolve(Feature::Profiles, Some("profiles")));
        assert!(!resolve(Feature::Profiles, Some("-profiles")));
        assert!(
            resolve(Feature::Profiles, Some("worktrees, profiles")),
            "one entry among several, spaced the way a person writes it"
        );
        assert_eq!(
            resolve(Feature::Profiles, Some("holodeck")),
            cfg!(debug_assertions),
            "an id this build does not know leaves the default alone"
        );
        assert!(
            !resolve(Feature::Profiles, Some("profiles,-profiles")),
            "the last word on a feature is the one that counts"
        );
    }

    #[test]
    fn every_feature_names_itself_uniquely_and_reads_back() {
        for feature in ALL_FEATURES {
            assert_eq!(Feature::parse(feature.id()), Some(*feature));
            assert!(!feature.summary().is_empty());
        }
    }
}
