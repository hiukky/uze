//! The shape every harness's preference translation already had.
//!
//! Each vertical was writing the same procedure with different data: map
//! the three universal axes onto native keys, merge those keys into the
//! harness's own configuration file, then report one
//! [`PreferenceApplyDetail`] per axis. Only the mapping differs — which
//! keys, which values, which route, and why a route is not `Native`.
//!
//! So the mapping is what a vertical declares here, and the procedure
//! stops being written four times. Two things fall out of that:
//!
//! - `changed_keys` is *derived* from the writes rather than listed beside
//!   them. It was a hand-maintained parallel list in every vertical, and a
//!   parallel list of what you just wrote is a bug waiting for the day the
//!   two disagree.
//! - The config format stops being the vertical's problem: the same
//!   [`Mapping`] applies to JSON or TOML, because a key path and a scalar
//!   are all either format needs from it.

use std::path::Path;

use uze_core::{
    Result,
    preference::{
        PreferenceApplyDetail, PreferenceApplyOutcome, PreferenceMapping, PreferenceTranslation,
        summarize_apply,
    },
    router::CompatibilityRoute,
};

use super::{json_config, toml_config};

/// A scalar a harness configuration can carry. Deliberately tiny: this is
/// what the four verticals actually write, and a wider vocabulary would be
/// speculation about a fifth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Value {
    Text(&'static str),
    Flag(bool),
    /// An explicitly empty list — "allow nothing", which is a different
    /// statement from the key being absent.
    EmptyList,
}

/// One native key an axis settles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Write {
    Set(&'static [&'static str], Value),
    /// Removes the key entirely. Not the same as writing a default: a
    /// harness that reads an absent key as "unset" must see it absent.
    Clear(&'static [&'static str]),
}

impl Write {
    fn changed_key(&self) -> Option<String> {
        // Only a `Set` is reported. What the report answers is "which
        // native settings does this harness now carry because of you",
        // and a removed key is not one — this is the behaviour every
        // vertical already had, kept deliberately.
        match self {
            Self::Set(path, _) => Some(path.join(".")),
            Self::Clear(_) => None,
        }
    }
}

/// How one universal preference axis lands in one harness.
pub(crate) struct Axis {
    pub(crate) route: CompatibilityRoute,
    /// Empty when the harness cannot express this axis at all — which is
    /// a real answer, not a gap: guessing a key risks overwriting a
    /// setting the operator made themselves.
    pub(crate) writes: Vec<Write>,
    /// What the native configuration reads as, for a plan shown before
    /// anything is written.
    pub(crate) summary: String,
    /// Why the route is not `Native`. Required reading whenever it is not.
    pub(crate) note: Option<String>,
}

impl Axis {
    pub(crate) fn new(route: CompatibilityRoute, summary: impl Into<String>) -> Self {
        Self {
            route,
            writes: Vec::new(),
            summary: summary.into(),
            note: None,
        }
    }

    pub(crate) fn set(mut self, path: &'static [&'static str], value: Value) -> Self {
        self.writes.push(Write::Set(path, value));
        self
    }

    pub(crate) fn clear(mut self, path: &'static [&'static str]) -> Self {
        self.writes.push(Write::Clear(path));
        self
    }

    pub(crate) fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    fn mapping(&self) -> PreferenceMapping {
        PreferenceMapping {
            route: self.route,
            native_summary: self.summary.clone(),
        }
    }

    fn detail(&self) -> PreferenceApplyDetail {
        PreferenceApplyDetail {
            route: self.route,
            changed_keys: self.writes.iter().filter_map(Write::changed_key).collect(),
            note: self.note.clone(),
        }
    }
}

/// One harness's answer for all three axes.
pub(crate) struct Mapping {
    pub(crate) autonomy: Axis,
    pub(crate) sandbox: Axis,
    pub(crate) model: Axis,
}

impl Mapping {
    fn axes(&self) -> [&Axis; 3] {
        [&self.autonomy, &self.sandbox, &self.model]
    }

    /// What would be written, without writing it.
    pub(crate) fn translate(&self) -> PreferenceTranslation {
        PreferenceTranslation {
            autonomy: self.autonomy.mapping(),
            sandbox: self.sandbox.mapping(),
            model: self.model.mapping(),
        }
    }

    fn outcome(&self) -> PreferenceApplyOutcome {
        summarize_apply(self.axes().map(Axis::detail))
    }

    /// Merges every axis into a JSON configuration, preserving whatever
    /// the operator already had there.
    pub(crate) fn apply_json(&self, path: &Path) -> Result<PreferenceApplyOutcome> {
        json_config::merge(path, |config| {
            for write in self.axes().into_iter().flat_map(|axis| &axis.writes) {
                match write {
                    Write::Set(keys, value) => {
                        json_config::set_path(config, keys, json_value(*value))?;
                    }
                    Write::Clear(keys) => json_config::remove_path(config, keys),
                }
            }
            Ok(())
        })?;
        Ok(self.outcome())
    }

    /// The same, for a TOML configuration — comments and foreign tables
    /// survive, which is `toml_config::merge`'s whole point.
    pub(crate) fn apply_toml(&self, path: &Path) -> Result<PreferenceApplyOutcome> {
        toml_config::merge(path, |document| {
            for write in self.axes().into_iter().flat_map(|axis| &axis.writes) {
                match write {
                    Write::Set(keys, Value::Text(text)) => {
                        toml_config::set_path(document, keys, *text)?;
                    }
                    Write::Set(keys, Value::Flag(flag)) => {
                        toml_config::set_path(document, keys, *flag)?;
                    }
                    Write::Set(keys, Value::EmptyList) => {
                        toml_config::set_path(document, keys, toml_edit::Array::new())?;
                    }
                    Write::Clear(keys) => toml_config::remove_path(document, keys),
                }
            }
            Ok(())
        })?;
        Ok(self.outcome())
    }
}

fn json_value(value: Value) -> serde_json::Value {
    match value {
        Value::Text(text) => serde_json::json!(text),
        Value::Flag(flag) => serde_json::json!(flag),
        Value::EmptyList => serde_json::json!([]),
    }
}
