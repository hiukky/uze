//! What can be done to a thing, decided once.
//!
//! A screen that acts on rows needs its surfaces to agree about the same
//! question: the buttons in its detail view, the index of everything, and
//! the keyboard. They agree because they read one list — this one — rather
//! than each re-deriving it from the same fields and drifting.
//!
//! It also settles where the decision lives. Whether a plugin can be
//! updated is a fact about the plugin, not a rendering concern, and a
//! presentation layer that filtered on `installed && update_available`
//! itself was one refactor away from disagreeing with the CLI about it.
//!
//! An offer that is *not* available carries why, for a surface with room to
//! say it; the detail view draws only what can be done now.

use serde::Serialize;
use uze_keys::Action;

use super::lifecycle::remove::is_protected_plugin;
use super::profile::ProfileSummary;
use super::read_models::{FreshnessState, HarnessHealth, MarketplacePluginSummary};

/// One thing that can be done to one entity, and whether it can be done
/// now.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ActionOffer {
    pub action: Action,
    pub availability: Availability,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    Available,
    /// Why not, in words a person reads.
    Unavailable(String),
}

impl ActionOffer {
    fn available(action: Action) -> ActionOffer {
        ActionOffer {
            action,
            availability: Availability::Available,
        }
    }

    fn unavailable(action: Action, reason: impl Into<String>) -> ActionOffer {
        ActionOffer {
            action,
            availability: Availability::Unavailable(reason.into()),
        }
    }

    pub fn is_available(&self) -> bool {
        self.availability == Availability::Available
    }

    /// Why this cannot be done now, or `None` when it can.
    pub fn reason(&self) -> Option<&str> {
        match &self.availability {
            Availability::Available => None,
            Availability::Unavailable(reason) => Some(reason),
        }
    }
}

impl MarketplacePluginSummary {
    pub fn offers(&self) -> Vec<ActionOffer> {
        vec![
            if self.installed {
                ActionOffer::unavailable(Action::InstallPlugin, "already installed")
            } else {
                ActionOffer::available(Action::InstallPlugin)
            },
            match (self.installed, &self.freshness.state) {
                (false, _) => ActionOffer::unavailable(Action::UpdatePlugin, "not installed"),
                (true, FreshnessState::Behind { .. }) => {
                    ActionOffer::available(Action::UpdatePlugin)
                }
                (true, FreshnessState::UpToDate) => {
                    ActionOffer::unavailable(Action::UpdatePlugin, "already up to date")
                }
                (true, FreshnessState::Linked { .. }) => ActionOffer::unavailable(
                    Action::UpdatePlugin,
                    "linked to a checkout on this machine, which is what it follows",
                ),
                (true, FreshnessState::Unpinned) => ActionOffer::unavailable(
                    Action::UpdatePlugin,
                    "installed from a source no marketplace tracks",
                ),
                (true, FreshnessState::NotChecked) => ActionOffer::unavailable(
                    Action::UpdatePlugin,
                    "its marketplace could not be compared against",
                ),
            },
            if !self.installed {
                ActionOffer::unavailable(Action::RemovePlugin, "not installed")
            } else if is_protected_plugin(&self.marketplace, &self.name) {
                ActionOffer::unavailable(
                    Action::RemovePlugin,
                    "part of the official set, which re-seeds itself",
                )
            } else {
                ActionOffer::available(Action::RemovePlugin)
            },
        ]
    }
}

impl HarnessHealth {
    /// One action, and it is always on offer, because setting a harness up
    /// is what UZE does to *any* card in the catalog: `setup` provisions
    /// through the vendor's own official route — installing what is not on
    /// the machine, updating what is — and then configures it to receive
    /// plugins.
    ///
    /// Refusing it for a harness that is absent read the state backwards.
    /// The catalog then offered setup only where setup had already been
    /// done, and offered nothing at all on the one card where there was
    /// something to do; the CLI meanwhile listed exactly those absent
    /// harnesses to pick from (`choose_harnesses`), which is the same
    /// question answered two ways.
    pub fn offers(&self) -> Vec<ActionOffer> {
        vec![ActionOffer::available(Action::SetupHarness)]
    }
}

impl ProfileSummary {
    pub fn offers(&self) -> Vec<ActionOffer> {
        // Applying stays available on the active profile: its preferences
        // may have been edited since, or a harness's configuration changed
        // by hand — being active is not the same as being in effect.
        vec![
            ActionOffer::available(Action::ApplyProfile),
            ActionOffer::available(Action::DeleteProfile),
        ]
    }
}

/// What can be done to a built-in extension.
///
/// A free function rather than a method: the catalog entry belongs to
/// `uze-extensions`, which knows nothing of this crate and should not.
/// The answer is short because extensions ship in the binary — but it is
/// not empty, and that matters: a row whose actions are empty is a
/// gesture that appears to do nothing, which is the defect this whole
/// mechanism exists to remove.
pub fn extension_offers() -> Vec<ActionOffer> {
    vec![
        ActionOffer::available(Action::Activate),
        ActionOffer::unavailable(
            Action::InstallPlugin,
            "extensions ship inside uze; there is nothing to install",
        ),
    ]
}

/// What can be done to one line of the Keys screen. A free function for
/// the same reason as [`extension_offers`]: the line is the keymap's, and
/// all this needs to know about it is whether the operator changed it.
pub fn key_offers(customised: bool) -> Vec<ActionOffer> {
    vec![
        ActionOffer::available(Action::ChangeKey),
        if customised {
            ActionOffer::available(Action::ResetKey)
        } else {
            ActionOffer::unavailable(Action::ResetKey, "already the key uze ships with")
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::read_models::Freshness;

    fn plugin(
        installed: bool,
        update: Option<bool>,
        marketplace: &str,
    ) -> MarketplacePluginSummary {
        MarketplacePluginSummary {
            marketplace: marketplace.to_owned(),
            name: "uze".to_owned(),
            description: None,
            keywords: Vec::new(),
            installed,
            freshness: match update {
                Some(true) => Freshness {
                    state: FreshnessState::Behind { commits: None },
                    established_at_unix: Some(0),
                },
                Some(false) => Freshness {
                    state: FreshnessState::UpToDate,
                    established_at_unix: Some(0),
                },
                None => Freshness::not_checked(),
            },
            is_default: false,
        }
    }

    fn offer(plugin: &MarketplacePluginSummary, action: Action) -> ActionOffer {
        plugin
            .offers()
            .into_iter()
            .find(|offer| offer.action == action)
            .expect("every action is offered, available or not")
    }

    #[test]
    fn an_uninstalled_plugin_can_be_installed_and_nothing_else() {
        let plugin = plugin(false, None, "community");
        assert!(offer(&plugin, Action::InstallPlugin).is_available());
        assert_eq!(
            offer(&plugin, Action::UpdatePlugin).reason(),
            Some("not installed")
        );
        assert_eq!(
            offer(&plugin, Action::RemovePlugin).reason(),
            Some("not installed")
        );
    }

    /// Pressing update on a plugin with no update used to do nothing at
    /// all, which reads as broken. It is now an offer that says why.
    #[test]
    fn an_action_that_cannot_run_says_why_rather_than_doing_nothing() {
        let current = plugin(true, Some(false), "community");
        assert!(!offer(&current, Action::UpdatePlugin).is_available());
        assert_eq!(
            offer(&current, Action::UpdatePlugin).reason(),
            Some("already up to date")
        );

        let unknown = plugin(true, None, "community");
        assert!(
            offer(&unknown, Action::UpdatePlugin)
                .reason()
                .is_some_and(|reason| reason.contains("compared")),
            "an unknown comparison is not the same answer as no update"
        );
    }

    #[test]
    fn the_official_set_is_not_removable_and_the_row_says_so() {
        let official = plugin(true, Some(false), uze_core::manifest::BUILT_IN_MARKETPLACE);
        assert_eq!(
            offer(&official, Action::RemovePlugin).reason(),
            Some("part of the official set, which re-seeds itself")
        );
        assert!(offer(&plugin(true, None, "community"), Action::RemovePlugin).is_available());
    }

    /// Every list row answers "what can be done to this" with something.
    /// A row that answered with nothing would be a menu that opens empty,
    /// which reads exactly like the silent no-op this replaced.
    #[test]
    fn a_row_always_has_at_least_one_thing_that_can_be_done_to_it() {
        for offers in [
            plugin(false, None, "community").offers(),
            plugin(true, Some(true), "community").offers(),
            extension_offers(),
        ] {
            assert!(offers.iter().any(ActionOffer::is_available), "{offers:?}");
        }
    }

    #[test]
    fn every_offer_carries_the_words_a_surface_prints() {
        for offer in plugin(true, Some(true), "community").offers() {
            assert!(!offer.action.label().is_empty());
        }
        assert!(
            Action::RemovePlugin.destructive(),
            "a menu decides where to put an entry from this, not from a list of its own"
        );
    }
}
