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
    Result, UzeError,
    preference::{
        AxisPlan, KeyPlan, PlannedValue, PreferenceApplyDetail, PreferenceApplyOutcome,
        PreferenceAxis, PreferenceMapping, PreferencePlan, PreferenceTranslation, summarize_apply,
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
}

/// One native key an axis settles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Write {
    Set(&'static [&'static str], Value),
    /// Removes the key entirely. Not the same as writing a default: a
    /// harness that reads an absent key as "unset" must see it absent.
    Clear(&'static [&'static str]),
    /// Removes the key only while it holds one of these values — the ones
    /// UZE itself writes there. A value outside the list is the operator's
    /// own choice, and "use the harness's default" is no reason to erase
    /// it; a value inside it is UZE's earlier answer, which would otherwise
    /// outlive the preference that produced it.
    Release(&'static [&'static str], &'static [&'static str]),
}

impl Write {
    fn path(&self) -> &'static [&'static str] {
        match self {
            Self::Set(path, _) | Self::Clear(path) | Self::Release(path, _) => path,
        }
    }

    fn changed_key(&self) -> Option<String> {
        // Only a `Set` is reported. What the report answers is "which
        // native settings does this harness now carry because of you",
        // and a removed key is not one — this is the behaviour every
        // vertical already had, kept deliberately.
        match self {
            Self::Set(path, _) => Some(path.join(".")),
            Self::Clear(_) | Self::Release(..) => None,
        }
    }

    /// Whether this write removes a key currently holding `current`.
    fn removes(&self, current: Option<&Scalar>) -> bool {
        match self {
            Self::Set(..) => false,
            Self::Clear(_) => true,
            Self::Release(_, owned) => {
                matches!(current, Some(Scalar::Text(text)) if owned.contains(&text.as_str()))
            }
        }
    }

    /// What the key holds once this write is applied over `held`.
    fn fold(&self, held: Held) -> Held {
        match self {
            Self::Set(_, value) => Held::Written(Some(Scalar::from(*value))),
            _ if self.removes(held.value()) => Held::Written(None),
            _ => held,
        }
    }
}

/// A key's value while the writes to it are folded over it, remembering
/// whether it is still what the file held or something a write left there.
enum Held {
    Disk(Option<Scalar>),
    Written(Option<Scalar>),
}

impl Held {
    fn value(&self) -> Option<&Scalar> {
        match self {
            Self::Disk(value) | Self::Written(value) => value.as_ref(),
        }
    }

    fn planned(self) -> PlannedValue {
        match self {
            Self::Written(Some(value)) => PlannedValue::Set(value.render()),
            Self::Written(None) | Self::Disk(None) => PlannedValue::Removed,
            Self::Disk(Some(_)) => PlannedValue::Kept,
        }
    }
}

/// A configuration value normalised across JSON and TOML, so "is this
/// already in effect" is a comparison of meaning, not of spelling — a TOML
/// literal string `'never'` holds the same value as `"never"`.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Scalar {
    Text(String),
    Flag(bool),
    List(Vec<Scalar>),
    /// Anything the preference vocabulary never writes, kept as its own
    /// rendering so it can be shown but never mistaken for a planned value.
    Other(String),
}

impl From<Value> for Scalar {
    fn from(value: Value) -> Self {
        match value {
            Value::Text(text) => Self::Text(text.to_owned()),
            Value::Flag(flag) => Self::Flag(flag),
        }
    }
}

impl Scalar {
    fn from_json(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::String(text) => Self::Text(text.clone()),
            serde_json::Value::Bool(flag) => Self::Flag(*flag),
            serde_json::Value::Array(items) => {
                Self::List(items.iter().map(Self::from_json).collect())
            }
            other => Self::Other(other.to_string()),
        }
    }

    fn from_toml(item: &toml_edit::Item) -> Self {
        match item.as_value() {
            Some(toml_edit::Value::String(text)) => Self::Text(text.value().clone()),
            Some(toml_edit::Value::Boolean(flag)) => Self::Flag(*flag.value()),
            Some(toml_edit::Value::Array(items)) => Self::List(
                items
                    .iter()
                    .map(|value| Self::from_toml(&toml_edit::Item::Value(value.clone())))
                    .collect(),
            ),
            Some(other) => Self::Other(other.to_string().trim().to_owned()),
            None => Self::Other(item.to_string().trim().to_owned()),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Text(text) => serde_json::Value::String(text.clone()).to_string(),
            Self::Flag(flag) => flag.to_string(),
            Self::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Self::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Other(rendered) => rendered.clone(),
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

    pub(crate) fn release(
        mut self,
        path: &'static [&'static str],
        owned: &'static [&'static str],
    ) -> Self {
        self.writes.push(Write::Release(path, owned));
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

    fn writes(&self) -> impl Iterator<Item = &Write> {
        self.axes().into_iter().flat_map(|axis| &axis.writes)
    }

    /// Merges every axis into a JSON configuration, preserving whatever
    /// the operator already had there.
    pub(crate) fn apply_json(&self, path: &Path) -> Result<PreferenceApplyOutcome> {
        json_config::merge(path, |config| {
            for write in self.writes() {
                let current = json_config::get_path(config, write.path()).map(Scalar::from_json);
                match write {
                    Write::Set(keys, value) => {
                        json_config::set_path(config, keys, json_value(*value))?;
                    }
                    _ if write.removes(current.as_ref()) => {
                        json_config::remove_path(config, write.path());
                    }
                    _ => {}
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
            for write in self.writes() {
                let current = toml_config::get_path(document, write.path()).map(Scalar::from_toml);
                match write {
                    Write::Set(keys, Value::Text(text)) => {
                        toml_config::set_path(document, keys, *text)?;
                    }
                    Write::Set(keys, Value::Flag(flag)) => {
                        toml_config::set_path(document, keys, *flag)?;
                    }
                    _ if write.removes(current.as_ref()) => {
                        toml_config::remove_path(document, write.path());
                    }
                    _ => {}
                }
            }
            Ok(())
        })?;
        Ok(self.outcome())
    }

    /// What `apply_json` would leave in `path`, read against it as it is.
    pub(crate) fn plan_json(&self, path: &Path) -> Result<PreferencePlan> {
        let config = json_config::read_object(path).map_err(unreadable)?;
        self.plan(
            path,
            |keys| json_config::get_path(&config, keys).map(Scalar::from_json),
            |keys| json_config::writable(&config, keys),
        )
    }

    /// What `apply_toml` would leave in `path`, read against it as it is.
    pub(crate) fn plan_toml(&self, path: &Path) -> Result<PreferencePlan> {
        let document = toml_config::read_document(path).map_err(unreadable)?;
        self.plan(
            path,
            |keys| toml_config::get_path(&document, keys).map(Scalar::from_toml),
            |keys| toml_config::writable(&document, keys),
        )
    }

    /// Each key is reported once, under the last axis that writes it, with
    /// every write to it folded in order over what the file holds — the
    /// same order `apply` performs them in, so the two cannot disagree when
    /// two axes settle one key (OpenCode's read-only overriding autonomy's
    /// `permission.edit`).
    ///
    /// A key `apply` would refuse to write — its parent holds some other
    /// shape — fails the plan the way it would fail the apply, rather than
    /// promising a value that will never land.
    fn plan(
        &self,
        path: &Path,
        current: impl Fn(&'static [&'static str]) -> Option<Scalar>,
        writable: impl Fn(&'static [&'static str]) -> std::result::Result<(), String>,
    ) -> Result<PreferencePlan> {
        let axes = [
            (PreferenceAxis::Autonomy, &self.autonomy),
            (PreferenceAxis::Sandbox, &self.sandbox),
            (PreferenceAxis::Model, &self.model),
        ];
        let writes: Vec<(usize, &Write)> = axes
            .iter()
            .enumerate()
            .flat_map(|(index, (_, axis))| axis.writes.iter().map(move |write| (index, write)))
            .collect();
        let mut keys: [Vec<KeyPlan>; 3] = Default::default();
        for (position, (axis, write)) in writes.iter().enumerate() {
            let key = write.path();
            if writes[position + 1..]
                .iter()
                .any(|(_, later)| later.path() == key)
            {
                continue;
            }
            let on_disk = current(key);
            let mut held = Held::Disk(on_disk.clone());
            for (_, earlier) in writes.iter().filter(|(_, other)| other.path() == key) {
                if matches!(earlier, Write::Set(..)) {
                    writable(key).map_err(unwritable)?;
                }
                held = earlier.fold(held);
            }
            keys[*axis].push(KeyPlan {
                key: key.join("."),
                current: on_disk.as_ref().map(Scalar::render),
                planned: held.planned(),
            });
        }
        let axes = axes
            .into_iter()
            .zip(keys)
            .map(|((name, axis), keys)| AxisPlan {
                axis: name,
                route: axis.route,
                summary: axis.summary.clone(),
                note: axis.note.clone(),
                keys,
            })
            .collect();
        Ok(PreferencePlan {
            config_path: path.to_path_buf(),
            axes,
        })
    }
}

/// The same words `apply` fails with, so a preview and an apply refused
/// for one reason say so the same way.
fn unwritable(reason: String) -> UzeError {
    UzeError::ExposureUnavailable(format!("cannot update preferences: {reason}"))
}

fn unreadable(reason: String) -> UzeError {
    UzeError::ExposureUnavailable(format!("cannot read preferences: {reason}"))
}

fn json_value(value: Value) -> serde_json::Value {
    match value {
        Value::Text(text) => serde_json::json!(text),
        Value::Flag(flag) => serde_json::json!(flag),
    }
}
