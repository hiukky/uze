//! Every profile a person can build, applied to every harness.
//!
//! The preference space is small enough to walk whole — four autonomy
//! levels, three sandbox scopes, three model tiers — so nothing here
//! samples it. For each combination and each registered preference adapter
//! this proves what a profile promises the machine it lands on:
//!
//! - applying leaves nothing pending in the adapter's own preview, so the
//!   preview and the write describe the same file;
//! - applying twice changes nothing the second time;
//! - the operator's own content in that file survives;
//! - no value the real harness binary is known to reject is left behind,
//!   including one an earlier UZE wrote there itself.
//!
//! The rejected values are evidence, not guesses: each was reproduced
//! against the shipped binary (see the adapter's module docs for how).

use std::{fs, path::Path};

use uze_core::{
    home::UzeHome,
    preference::{
        Autonomy, ModelPreference, PlannedValue, PreferenceApplyOutcome, Preferences, SandboxScope,
    },
};
use uze_integrations::registry::IntegrationRegistry;

/// (adapter, top-level key, value) a harness refuses or cannot resolve.
const REJECTED: &[(&str, &str, &str)] = &[
    // Claude Code 2.1.268: `unrecognized_model`, and the literal is sent on.
    ("claude-code", "model", "default"),
    // Codex 0.153.4: "no longer supported; remove this setting" — it will
    // not start.
    ("codex", "approval_policy", "untrusted"),
    // agy 1.2.0: "invalid settings", and the whole file is dropped.
    ("antigravity", "agentMode", "default"),
];

fn every_preference() -> Vec<Preferences> {
    let mut all = Vec::new();
    for autonomy in [
        Autonomy::Manual,
        Autonomy::Balanced,
        Autonomy::Auto,
        Autonomy::Unattended,
    ] {
        for sandbox in [
            SandboxScope::ReadOnly,
            SandboxScope::WorkspaceWrite,
            SandboxScope::FullAccess,
        ] {
            for model in [
                ModelPreference::Default,
                ModelPreference::Fast,
                ModelPreference::Capable,
            ] {
                all.push(Preferences {
                    autonomy,
                    sandbox,
                    model,
                });
            }
        }
    }
    all
}

fn is_toml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "toml")
}

/// The file as an operator who ran an earlier UZE might have it: their own
/// content, plus whatever rejected value that release wrote.
fn seed(path: &Path, adapter: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rejected: Vec<_> = REJECTED
        .iter()
        .filter(|(owner, ..)| *owner == adapter)
        .collect();
    let contents = if is_toml(path) {
        let mut text = String::from("# the operator's own comment\nforeign_key = \"keep\"\n");
        // A note right above a key UZE overwrites is the operator's too.
        for (_, key, value) in &rejected {
            text.push_str(&format!(
                "# the operator's note on {key}\n{key} = \"{value}\"\n"
            ));
        }
        text.push_str("\n[foreign_table]\nkept = 1\n");
        text
    } else {
        let mut object = serde_json::json!({"foreignKey": "keep", "foreignTable": {"kept": 1}});
        for (_, key, value) in &rejected {
            object[*key] = serde_json::json!(value);
        }
        object.to_string()
    };
    fs::write(path, contents).unwrap();
}

fn top_level(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).unwrap();
    if is_toml(path) {
        // Top-level keys are the lines before the first table header.
        text.lines()
            .take_while(|line| !line.trim_start().starts_with('['))
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| name.trim() == key)
            .map(|(_, value)| value.trim().trim_matches(['"', '\'']).to_owned())
    } else {
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value.get(key).and_then(|v| v.as_str()).map(str::to_owned)
    }
}

fn foreign_content_survived(path: &Path, adapter: &str) -> bool {
    let text = fs::read_to_string(path).unwrap();
    if is_toml(path) {
        REJECTED
            .iter()
            .filter(|(owner, ..)| *owner == adapter)
            .all(|(_, key, _)| text.contains(&format!("# the operator's note on {key}")))
            && text.contains("# the operator's own comment")
            && text.contains("foreign_key = \"keep\"")
            && text.contains("[foreign_table]")
    } else {
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["foreignKey"] == "keep" && value["foreignTable"]["kept"] == 1
    }
}

#[test]
fn every_profile_applies_to_every_harness_as_its_preview_says() {
    let root = uze_testkit::temp::scratch("preferences-space");
    let home = UzeHome::at(root.join("uze"));
    let (_, adapters) = IntegrationRegistry::isolated(&root, &home).into_parts();
    assert_eq!(adapters.len(), 4, "every harness has a preference adapter");

    for adapter in &adapters {
        let id = adapter.preference_id();
        for preferences in every_preference() {
            let path = adapter.plan(&preferences).unwrap().config_path;
            seed(&path, id);
            let context = format!("{id} with {preferences:?}");

            let before = adapter.plan(&preferences).unwrap();
            assert_eq!(
                top_level(&path, "foreignKey").or_else(|| top_level(&path, "foreign_key")),
                Some("keep".to_owned()),
                "{context}: planning wrote to the file"
            );

            let outcome = adapter.apply(&preferences).unwrap();
            assert!(
                !matches!(
                    outcome,
                    PreferenceApplyOutcome::Failed { .. }
                        | PreferenceApplyOutcome::Unsupported { .. }
                ),
                "{context}: {outcome:?}"
            );

            let after = adapter.plan(&preferences).unwrap();
            assert_eq!(after.pending(), 0, "{context}: still pending {after:#?}");
            let promised = before.axes.iter().flat_map(|axis| &axis.keys);
            let landed = after.axes.iter().flat_map(|axis| &axis.keys);
            for (promise, key) in promised.zip(landed) {
                assert_eq!(promise.key, key.key, "{context}");
                let expected = match &promise.planned {
                    PlannedValue::Set(value) => Some(value.clone()),
                    PlannedValue::Removed => None,
                    PlannedValue::Kept => promise.current.clone(),
                };
                assert_eq!(
                    key.current, expected,
                    "{context}: `{}` is not what the preview promised",
                    key.key
                );
            }

            let written = fs::read(&path).unwrap();
            adapter.apply(&preferences).unwrap();
            assert_eq!(
                fs::read(&path).unwrap(),
                written,
                "{context}: a second apply changed the file"
            );

            assert!(
                foreign_content_survived(&path, id),
                "{context}: the operator's content was lost"
            );
            for (_, key, value) in REJECTED.iter().filter(|(owner, ..)| *owner == id) {
                assert_ne!(
                    top_level(&path, key).as_deref(),
                    Some(*value),
                    "{context}: left `{key} = {value}`, which the harness rejects"
                );
            }
        }
    }
    let _ = fs::remove_dir_all(&root);
}
