//! The keymap file format.
//!
//! Everything here is what an operator *wrote*, not what uze will do:
//! chords stay as text until the resolver can report a bad one against the
//! action it was written for, and action and scope names are strings rather
//! than the enums for the same reason a theme file keeps token names as
//! strings — a keymap written for a newer uze names entries this build has
//! never heard of, and that has to be a warning rather than a file that
//! will not load.
//!
//! A file holds only what differs from the built-in default. That is what
//! lets a later release move a chord nobody had an opinion about, instead
//! of freezing every operator's keyboard at the version they first opened
//! the Keys screen on.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The schema version this build writes and understands.
pub const CURRENT_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeymapFile {
    /// The schema this file was written against. Absent means
    /// [`CURRENT_VERSION`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Scope name → action name → the chords that reach it there.
    ///
    /// An empty list is a deliberate unbinding: the action stays in the
    /// vocabulary, stays in the index, and stays reachable by pointer — it
    /// just has no key. That is a finished design, so it needs a spelling.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

impl KeymapFile {
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Records `chords` for `action` in `scope`, replacing whatever the
    /// file said before.
    pub fn set(&mut self, scope: &str, action: &str, chords: Vec<String>) {
        self.bindings
            .entry(scope.to_owned())
            .or_default()
            .insert(action.to_owned(), chords);
    }

    /// Forgets an entry, so the built-in default applies again.
    pub fn clear(&mut self, scope: &str, action: &str) {
        if let Some(scoped) = self.bindings.get_mut(scope) {
            scoped.remove(action);
            if scoped.is_empty() {
                self.bindings.remove(scope);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_carries_only_what_differs() {
        let mut file = KeymapFile::default();
        assert!(file.is_empty());
        file.set("workspace", "new-shell-tab", vec!["f4".to_owned()]);
        let json = serde_json::to_string(&file).expect("serializes");
        assert_eq!(
            json, r#"{"bindings":{"workspace":{"new-shell-tab":["f4"]}}}"#,
            "an untouched action is absent, not restated"
        );
        file.clear("workspace", "new-shell-tab");
        assert!(file.is_empty(), "clearing the last entry empties the scope");
    }

    #[test]
    fn an_empty_list_is_how_unbinding_is_written() {
        let file: KeymapFile =
            serde_json::from_str(r#"{"bindings":{"workspace":{"close-tab":[]}}}"#).expect("reads");
        assert_eq!(
            file.bindings["workspace"]["close-tab"],
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unreadable_chord_does_not_stop_the_file_from_parsing() {
        // It is reported against the action it was written for, which
        // needs the file to have parsed first.
        let file: KeymapFile =
            serde_json::from_str(r#"{"bindings":{"workspace":{"close-tab":["hyper+w"]}}}"#)
                .expect("reads");
        assert_eq!(file.bindings["workspace"]["close-tab"], vec!["hyper+w"]);
    }
}
