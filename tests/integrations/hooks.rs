//! Canonical Hook capability routes remain explicit across every harness
//! (ADR-033): semantic compatibility, event-array config merging with
//! content-identity receipts, the generated Antigravity plugin, and the
//! owned OpenCode bridge lifecycle.

use std::fs;
use std::path::{Path, PathBuf};

use uze::integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};
use uze::{
    engine::package_resources_at,
    home::UzeHome,
    hook::HookEvent,
    integration::{AttachmentState, IntegrationPort, ManagedArtifact},
    project::Resource,
    router::CompatibilityRoute,
    state,
    store::PackageId,
};

fn temp(label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-hooks-integration-{label}-{}-{nonce}",
        std::process::id()
    ))
}

/// Builds a real package directory (plugin.json + hooks.json) and discovers
/// its Hook resources exactly like the Engine does.
fn hook_package(label: &str, manifest: &str) -> (PathBuf, Vec<Resource>) {
    let root = temp(label);
    let pkg = root.join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("plugin.json"),
        r#"{"name":"hook-demo","version":"1.0.0","description":"Hooks fixture"}"#,
    )
    .unwrap();
    fs::write(pkg.join("hooks.json"), manifest).unwrap();
    let id = PackageId::from_plugin_name("hook-demo", &pkg.join("plugin.json")).unwrap();
    let resources = package_resources_at(&id, &pkg).unwrap();
    (root, resources)
}

fn deny_group() -> &'static str {
    r#"{"hooks":{"PreToolUse":[{"id":"protect-env","matcher":"shell","effect":"deny","hooks":[{"type":"command","command":"${PLUGIN_ROOT}/scripts/check","timeout":10}]}]}}"#
}

fn manifest_with(groups: &str) -> String {
    format!(r#"{{"hooks":{{{groups}}}}}"#)
}

fn hook_resource<'a>(resources: &'a [Resource], id: &str) -> &'a Resource {
    resources
        .iter()
        .find(|resource| resource.resource_name.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("hook group `{id}` was discovered"))
}

fn event_configuration(root: &Path) -> PathBuf {
    root.join("claude").join("settings.json")
}

// ============================================================================
// Semantic compatibility (spec: "UZE calculates Hook compatibility
// semantically")
// ============================================================================

#[test]
fn compatibility_is_semantic_and_never_fabricates_a_stop_equivalence() {
    let (_root, resources) = hook_package("compat", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(temp("compat-home").join("uze"));
    let claude = ClaudeIntegration::new(temp("compat-home").join("claude"), home.clone());
    let codex = CodexIntegration::new(temp("compat-home").join("agents"), home.clone());
    let opencode = OpenCodeIntegration::new(
        temp("compat-home").join("agents"),
        temp("compat-home").join("config/opencode.json"),
        home.clone(),
    );
    let antigravity = AntigravityIntegration::new(temp("compat-home").join("agents"), home);

    // A Deny pre-tool hook: Native on the native hook harnesses, Adaptable
    // through the OpenCode bridge (the generated source is UZE's adapter),
    // package-delivered on Antigravity.
    assert_eq!(
        claude.exposure_plan(protect).route,
        CompatibilityRoute::Native
    );
    assert_eq!(
        codex.exposure_plan(protect).route,
        CompatibilityRoute::Native
    );
    assert_eq!(
        opencode.exposure_plan(protect).route,
        // OpenCode V2 exposes no input-based block (spec:
        // opencode.ai/v2/docs/build/plugins — the action-level deny lives in
        // the permission hook, which carries no tool input), so deny is
        // diagnosed Unsupported, never fabricated.
        CompatibilityRoute::Unsupported
    );
    assert_eq!(
        antigravity.exposure_plan(protect).route,
        CompatibilityRoute::Native
    );

    // Stop must never claim an OpenCode equivalence (spec scenario).
    let (_root_stop, stop_resources) = hook_package(
        "compat-stop",
        &manifest_with(r#""Stop":[{"id":"archive","hooks":[{"type":"command","command":"log"}]}]"#),
    );
    let archive = hook_resource(&stop_resources, "archive");
    let opencode_plan = opencode.exposure_plan(archive);
    assert_eq!(opencode_plan.route, CompatibilityRoute::Degraded);
    assert!(
        opencode_plan.evidence.contains("no `stop` semantic event"),
        "the opencode plan must state the exact semantic loss"
    );
    assert_eq!(
        claude.exposure_plan(archive).route,
        CompatibilityRoute::Native,
        "Claude documents a Stop event"
    );
    assert_eq!(
        antigravity.exposure_plan(archive).route,
        CompatibilityRoute::Native
    );

    // Ask cannot be enforced on Claude (not in its declared effect set) and
    // must never silently become an observation — Unsupported, not Degraded.
    let (_root_ask, ask_resources) = hook_package(
        "compat-ask",
        &manifest_with(
            r#""PreToolUse":[{"id":"prompt","matcher":"file.write","effect":"ask","hooks":[{"type":"command","command":"ask"}]}]"#,
        ),
    );
    let prompt = hook_resource(&ask_resources, "prompt");
    assert_eq!(
        claude.exposure_plan(prompt).route,
        CompatibilityRoute::Unsupported
    );
    assert_eq!(
        codex.exposure_plan(prompt).route,
        CompatibilityRoute::Unsupported
    );
    assert_eq!(
        opencode.exposure_plan(prompt).route,
        CompatibilityRoute::Unsupported,
        "ask is a hard denial in the bridge, never a faithful ask"
    );
    assert_eq!(
        antigravity.exposure_plan(prompt).route,
        CompatibilityRoute::Native,
        "Antigravity documents native allow/ask/deny decisions"
    );
}

#[test]
fn transform_is_adaptable_through_the_bridge_and_degraded_on_claude() {
    let (_root, resources) = hook_package(
        "compat-transform",
        &manifest_with(
            r#""PreToolUse":[{"id":"sandbox","matcher":"file.write","effect":"transform","hooks":[{"type":"command","command":"rewrite"}]}]"#,
        ),
    );
    let sandbox = hook_resource(&resources, "sandbox");
    let home = UzeHome::at(temp("compat-transform-home").join("uze"));
    let claude = ClaudeIntegration::new(temp("compat-transform-home").join("claude"), home.clone());
    let opencode = OpenCodeIntegration::new(
        temp("compat-transform-home").join("agents"),
        temp("compat-transform-home").join("config/opencode.json"),
        home,
    );
    assert_eq!(
        claude.exposure_plan(sandbox).route,
        CompatibilityRoute::Degraded,
        "an input rewrite Claude cannot enforce must degrade, never attach silently"
    );
    assert_eq!(
        opencode.exposure_plan(sandbox).route,
        CompatibilityRoute::Adaptable,
        "the bridge rewrites output.args"
    );
}

// ============================================================================
// Claude: settings.json event-array merge, content-identity receipts
// ============================================================================

#[test]
fn claude_merges_into_settings_json_preserving_foreign_content() {
    let (root, resources) = hook_package("claude-merge", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(root.join("uze"));
    let claude = ClaudeIntegration::new(root.join("claude"), home);
    let settings = event_configuration(&root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"foreign"}]}]},"theme":"dark"}"#,
    )
    .unwrap();

    let plan = claude.exposure_plan(protect);
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let uze::exposure::ExposureMechanism::ManagedHookConfig {
        config_file,
        entry_name,
        event,
        expected: _expected,
    } = &plan.mechanism
    else {
        panic!("Claude hook plan is a managed config entry");
    };
    assert_eq!(*config_file, settings);
    assert_eq!(entry_name, "hook-demo@local:protect-env");
    assert_eq!(*event, Some(HookEvent::PreToolUse));

    let receipt = claude
        .attach_receipt(protect)
        .expect("attach succeeds")
        .expect("attach produces a receipt");
    let ManagedArtifact::HookConfigEntry {
        config_file,
        entry_name,
        event,
        expected,
    } = &receipt.artifact
    else {
        panic!("Claude hook receipt is a HookConfigEntry");
    };
    assert_eq!(*config_file, settings);
    assert_eq!(entry_name, "hook-demo@local:protect-env");
    assert_eq!(*event, Some(HookEvent::PreToolUse));

    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    let groups = document["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "foreign group stays, UZE group appended");
    assert_eq!(groups[0]["hooks"][0]["command"], "foreign");
    assert_eq!(document["theme"], "dark");
    let entry = &groups[1];
    assert_eq!(entry["matcher"], "Bash");
    let command = entry["hooks"][0]["command"].as_str().unwrap();
    assert!(
        command.contains("hook-exec"),
        "the authored command runs through the wrapper"
    );
    assert!(command.contains("--adapter 'claude-code'"));
    assert!(command.contains("--event pre_tool_use"));
    assert!(command.contains("--effect deny"));
    assert!(
        command.contains("--command '${PLUGIN_ROOT}/scripts/check'"),
        "the authored command is retained verbatim"
    );
    assert_eq!(entry["hooks"][0]["timeout"], 11);
    assert_eq!(
        serde_json::to_string(entry).unwrap(),
        *expected,
        "the receipt's expected content is the exact rendered entry (fingerprint)"
    );

    // Idempotence: attaching again must not duplicate.
    claude.attach_receipt(protect).unwrap();
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(document["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);

    assert_eq!(
        claude.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );

    // Drift: user edits the exact entry → content identity no longer matches.
    let drifted = fs::read_to_string(&settings)
        .unwrap()
        .replace("\"timeout\": 11", "\"timeout\": 99");
    fs::write(&settings, drifted).unwrap();
    assert_eq!(
        claude.inspect_receipt(&receipt).state,
        AttachmentState::Missing,
        "a changed UZE entry is not treated as ours for removal"
    );
    assert_eq!(
        claude.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing,
        "removal refuses drift and preserves the file"
    );

    // Restore the exact UZE entry (alongside the foreign group) and remove:
    // only the UZE entry goes, foreign hooks and unrelated keys survive.
    fs::write(
        &settings,
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    serde_json::json!({"matcher":"Bash","hooks":[{"type":"command","command":"foreign"}]}),
                    serde_json::from_str::<serde_json::Value>(expected).unwrap(),
                ]
            },
            "theme": "dark",
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        claude.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    assert_eq!(
        claude.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(
        document["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "foreign",
        "only the UZE entry was removed"
    );
    assert_eq!(document["theme"], "dark");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn claude_removal_cleans_an_entry_when_the_shared_file_is_left_empty() {
    let (root, resources) = hook_package("claude-cleanup", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(root.join("uze"));
    let claude = ClaudeIntegration::new(root.join("claude"), home);
    let settings = event_configuration(&root);

    let receipt = claude
        .attach_receipt(protect)
        .expect("attach succeeds")
        .expect("attach produces a receipt");
    assert!(settings.is_file());
    assert_eq!(
        claude.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    assert!(
        !settings.exists(),
        "a settings file that held only UZE content is removed with the last entry"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_update_replaces_the_previous_version_of_the_samed_group() {
    let (root, resources) = hook_package("claude-update", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(root.join("uze"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let settings = event_configuration(&root);

    let first = claude
        .attach_receipt(protect)
        .expect("attach succeeds")
        .expect("attach produces a receipt");
    state::record_receipt(&home, "claude-update-1".to_owned(), first.clone()).unwrap();

    // The package is updated: same group id, new timeout. Ledger-driven
    // re-attach replaces the old entry instead of duplicating it.
    let mut updated = deny_group()
        .replace("\"timeout\":10", "\"timeout\":20")
        .to_owned();
    updated = updated.replace("check", "check-v2");
    let (_root2, updated_resources) = hook_package("claude-update-v2", &updated);
    let updated_hook = hook_resource(&updated_resources, "protect-env");
    let second = claude
        .attach_receipt(updated_hook)
        .expect("attach succeeds")
        .expect("attach produces a receipt");
    assert_ne!(first.artifact, second.artifact, "rendered content changed");
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    let groups = document["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(
        groups.len(),
        1,
        "the old version is replaced, not duplicated"
    );
    assert!(
        groups[0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("check-v2")
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(_root2);
}

// ============================================================================
// Codex: its own hooks.json command form
// ============================================================================

#[test]
fn codex_writes_its_own_hooks_json_command_form() {
    let (root, resources) = hook_package("codex-hooks", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(root.join("uze"));
    let codex = CodexIntegration::new(root.join("agents"), home);
    let hooks_file = root.join(".codex").join("hooks.json");

    let plan = codex.exposure_plan(protect);
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let uze::exposure::ExposureMechanism::ManagedHookConfig {
        config_file,
        entry_name,
        event,
        expected,
    } = &plan.mechanism
    else {
        panic!("Codex hook plan is a managed config entry");
    };
    assert_eq!(*config_file, hooks_file);
    assert_eq!(entry_name, "hook-demo@local:protect-env");
    assert_eq!(*event, Some(HookEvent::PreToolUse));

    let receipt = codex
        .attach_receipt(protect)
        .expect("attach succeeds")
        .expect("attach produces a receipt");
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&hooks_file).unwrap()).unwrap();
    let groups = document["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    let command = groups[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(command.contains("--adapter 'codex'"));
    assert_eq!(
        serde_json::to_string(&groups[0]).unwrap(),
        *expected,
        "the receipt fingerprint is the exact rendered entry"
    );
    assert_eq!(
        codex.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    assert_eq!(
        codex.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    assert!(
        !hooks_file.exists(),
        "a UZE-created hooks.json is removed when it holds nothing else"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn foreign_codex_hooks_survive_attach_and_detach() {
    let (root, resources) = hook_package("codex-foreign", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(root.join("uze"));
    let codex = CodexIntegration::new(root.join("agents"), home);
    let hooks_file = root.join(".codex").join("hooks.json");
    fs::create_dir_all(hooks_file.parent().unwrap()).unwrap();
    fs::write(
        &hooks_file,
        r#"{"hooks":{"Stop":[{"matcher":".*","hooks":[{"type":"command","command":"user-stop"}]}]},"other":true}"#,
    )
    .unwrap();

    let receipt = codex.attach_receipt(protect).unwrap().unwrap();
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&hooks_file).unwrap()).unwrap();
    assert_eq!(document["other"], true);
    assert_eq!(
        document["hooks"]["Stop"][0]["hooks"][0]["command"], "user-stop",
        "a user-written Stop hook is never touched"
    );
    assert_eq!(
        codex.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&hooks_file).unwrap()).unwrap();
    assert_eq!(
        document["hooks"]["Stop"][0]["hooks"][0]["command"],
        "user-stop"
    );
    assert_eq!(document["other"], true);
    assert!(hooks_file.exists(), "foreign content keeps the file alive");
    let _ = fs::remove_dir_all(root);
}

// ============================================================================
// OpenCode: owned regenerable bridge + managed plugin entry
// ============================================================================

fn opencode(home_root: &std::path::Path) -> OpenCodeIntegration {
    OpenCodeIntegration::new(
        home_root.join("agents"),
        home_root.join("config/opencode.json"),
        UzeHome::at(home_root.join("uze")),
    )
}

/// Ingests the test package into the integration's store, as real
/// installation would — OpenCode detach re-resolves the package bytes from
/// the Store, never from the resource that may no longer be in hand.
fn ingest_package(home: &UzeHome, pkg_root: &std::path::Path) {
    use uze::{
        acquisition::{MaterializedPackage, PackageSource, Provenance, ResolvedSource},
        store::UzeStore,
    };
    UzeStore::new(home.clone())
        .ingest(&MaterializedPackage::borrowed(
            pkg_root.to_path_buf(),
            Provenance {
                requested: PackageSource::Local {
                    path: pkg_root.to_path_buf(),
                },
                resolved: ResolvedSource::Local {
                    path: pkg_root.to_path_buf(),
                },
            },
        ))
        .expect("test package ingests into the store");
}

/// Re-discovers the package's resources from the Store, exactly where the
/// engine finds them after a real installation — attach, inspect and
/// detach all resolve Store bytes, so the fixture resources must point at
/// the Store too.
fn stored_resources(home: &UzeHome, name: &str) -> Vec<Resource> {
    use uze::{engine::package_resources_at, store::UzeStore};
    let package = UzeStore::new(home.clone())
        .package(&PackageId::from_plugin_name(name, std::path::Path::new("plugin.json")).unwrap())
        .expect("stored package exists");
    package_resources_at(&package.id, &package.root).expect("Store resources rediscover")
}

#[test]
fn opencode_bridge_lifecycle_preserves_foreign_plugins_in_the_directory() {
    let (root, _resources) = hook_package(
        "opencode-bridge",
        &manifest_with(
            r#""PreToolUse":[{"id":"watch","matcher":"shell","effect":"observe","hooks":[{"type":"command","command":"${PLUGIN_ROOT}/scripts/check"}]}]"#,
        ),
    );
    let home = UzeHome::at(root.join("uze"));
    ingest_package(&home, &root.join("pkg"));
    let resources = stored_resources(&home, "hook-demo");
    let protect = hook_resource(&resources, "watch");
    let integration = opencode(&root);
    let config = root.join("config/opencode.json");
    let bridge = root.join("config/plugins/uze-hooks-hook-demo@local.ts");
    // A foreign plugin file already lives in the harness's global plugin
    // directory; the config itself is never touched by hook delivery.
    fs::create_dir_all(config.parent().unwrap().join("plugins")).unwrap();
    fs::write(
        config.parent().unwrap().join("plugins/foreign.js"),
        "// foreign\n",
    )
    .unwrap();
    fs::write(&config, r#"{"mcp": {"servers": {}}}"#).unwrap();

    let plan = integration.exposure_plan(protect);
    assert_eq!(plan.route, CompatibilityRoute::Adaptable);
    let uze::exposure::ExposureMechanism::ManagedHookFile { path } = &plan.mechanism else {
        panic!("OpenCode hook plan is an owned bridge file");
    };
    assert_eq!(*path, bridge);

    let receipt = integration
        .attach_receipt(protect)
        .expect("attach succeeds")
        .expect("attach produces a receipt");
    // Production records the receipt right after the attach; inspection is
    // receipt-driven, so mirror that exactly.
    state::record_receipt(&home.clone(), "oc-lifecycle-1".to_owned(), receipt.clone()).unwrap();
    let ManagedArtifact::ManagedHookFile { path } = &receipt.artifact else {
        panic!("OpenCode hook receipt is a ManagedHookFile");
    };
    assert_eq!(*path, bridge);
    assert!(
        bridge.is_file(),
        "the bridge file exists in the auto-discovered directory"
    );
    // The config is untouched — a single load source (the directory), never
    // a second explicit registration that could double-load the bridge.
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    assert!(
        document.get("plugin").is_none(),
        "no redundant plugin entry"
    );
    assert!(document.get("mcp").is_some());
    assert!(
        config
            .parent()
            .unwrap()
            .join("plugins/foreign.js")
            .is_file()
    );
    let source = fs::read_to_string(&bridge).unwrap();
    assert!(source.contains("Plugin.define"));
    assert!(source.contains("tool.hook"));
    assert!(source.contains("\"effect\":\"observe\""));
    assert!(source.contains("\"matchers\":[\"bash\"]"));

    assert_eq!(
        integration.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );

    // A deleted bridge file is Missing, never a silent success — removal
    // refuses (returns the inspection) and re-install regenerates it.
    fs::remove_file(&bridge).unwrap();
    assert_eq!(
        integration.inspect_receipt(&receipt).state,
        AttachmentState::Missing
    );
    assert_eq!(
        integration.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing,
        "removal refuses a missing bridge"
    );

    // Restore by re-attach; removal then deletes only the owned file, keeps
    // the foreign plugin, and leaves the config untouched.
    integration.attach_receipt(protect).unwrap();
    assert_eq!(
        integration.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    assert!(!bridge.exists());
    assert!(
        config
            .parent()
            .unwrap()
            .join("plugins/foreign.js")
            .is_file()
    );
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    assert!(document.get("plugin").is_none());
    assert!(document.get("mcp").is_some());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn opencode_bridge_is_package_scoped_and_regenerates_across_groups() {
    let (_root, _resources) = hook_package(
        "opencode-multi",
        &manifest_with(
            r#""PreToolUse":[{"id":"observe-first","matcher":"shell","hooks":[{"type":"command","command":"first"}]},{"id":"observe-second","matcher":"shell","hooks":[{"type":"command","command":"second"}]}]"#,
        ),
    );
    let home = UzeHome::at(_root.join("uze"));
    ingest_package(&home, &_root.join("pkg"));
    let resources = stored_resources(&home, "hook-demo");
    let first = hook_resource(&resources, "observe-first").clone();
    let second = hook_resource(&resources, "observe-second").clone();
    let integration = opencode(&_root);
    let bridge = _root.join("config/plugins/uze-hooks-hook-demo@local.ts");

    let receipt_first = integration.attach_receipt(&first).unwrap().unwrap();
    // Production records each receipt right after its attach, so the next
    // group's attach can see the sibling as active — mirror that here.
    state::record_receipt(&home.clone(), "multi-1".to_owned(), receipt_first.clone()).unwrap();
    let receipt_second = integration.attach_receipt(&second).unwrap().unwrap();
    state::record_receipt(&home, "multi-2".to_owned(), receipt_second.clone()).unwrap();
    assert_eq!(
        receipt_first.artifact, receipt_second.artifact,
        "one owned bridge per package"
    );
    let ManagedArtifact::ManagedHookFile { path } = &receipt_first.artifact else {
        unreachable!();
    };
    assert_eq!(*path, bridge);
    let source = fs::read_to_string(&bridge).unwrap();
    assert!(source.contains("\"id\":\"observe-first\""));
    assert!(source.contains("\"id\":\"observe-second\""));

    // Detaching one group regenerates the bridge without it; detaching the
    // last group removes the file. The ledger entries above let detach
    // see the sibling receipt.
    let integration_too = opencode(&_root);
    assert_eq!(
        integration_too
            .detach_receipt(&receipt_first)
            .unwrap()
            .state,
        AttachmentState::Missing
    );
    // Production forgets a receipt only after a successful detach; the
    // sibling's later detach must not see the forgotten group as active.
    state::forget_receipt(&home.clone(), "multi-1").unwrap();
    let source = fs::read_to_string(&bridge).unwrap();
    assert!(
        !source.contains("observe-first"),
        "the detached group leaves the bridge"
    );
    assert!(
        source.contains("observe-second"),
        "the sibling group stays bridged"
    );
    assert_eq!(
        integration_too
            .detach_receipt(&receipt_second)
            .unwrap()
            .state,
        AttachmentState::Missing
    );
    assert!(!bridge.exists(), "the last group removes the owned file");
    let _ = fs::remove_dir_all(_root);
}

#[test]
fn opencode_unmatch_all_groups_carry_no_matcher_and_stop_is_never_bridged() {
    let (_root, resources) = hook_package(
        "opencode-nomatcher",
        &manifest_with(
            r#""PreToolUse":[{"id":"all-tools","hooks":[{"type":"command","command":"watch"}]}],"Stop":[{"id":"bye","hooks":[{"type":"command","command":"bye"}]}]"#,
        ),
    );
    let all_tools = hook_resource(&resources, "all-tools");
    let stop = hook_resource(&resources, "bye");
    let integration = opencode(&_root);

    let plan = integration.exposure_plan(stop);
    assert_eq!(plan.route, CompatibilityRoute::Degraded);
    assert!(matches!(
        plan.mechanism,
        uze::exposure::ExposureMechanism::Unsupported { .. }
    ));
    assert!(
        integration.attach_receipt(stop).unwrap().is_none(),
        "a degraded hook never attaches on OpenCode"
    );

    let receipt = integration.attach_receipt(all_tools).unwrap().unwrap();
    let ManagedArtifact::ManagedHookFile { path } = &receipt.artifact else {
        unreachable!();
    };
    let source = fs::read_to_string(path).unwrap();
    assert!(
        source.contains("\"matchers\":[]"),
        "an unmatch-all group runs for every tool"
    );
    let _ = fs::remove_dir_all(_root);
}

// ============================================================================
// Antigravity: hooks only inside the generated native plugin
// ============================================================================

#[test]
fn antigravity_plans_hooks_through_the_generated_named_plugin() {
    let (_root, resources) = hook_package("agy-hooks", deny_group());
    let protect = hook_resource(&resources, "protect-env");
    let home = UzeHome::at(_root.join("uze"));
    let antigravity = AntigravityIntegration::new(_root.join("agents"), home);

    let plan = antigravity
        .package_exposure_plan(
            &uze::store::StoredPackage {
                id: PackageId::from_plugin_name("hook-demo", &_root.join("pkg/plugin.json"))
                    .unwrap(),
                active_name: "hook-demo".to_owned(),
                root: _root.join("pkg"),
                manifest: _root.join("pkg/plugin.json"),
                provenance: uze::acquisition::Provenance {
                    requested: uze::acquisition::PackageSource::Local {
                        path: _root.join("pkg"),
                    },
                    resolved: uze::acquisition::ResolvedSource::Local {
                        path: _root.join("pkg"),
                    },
                },
            },
            &resources.iter().collect::<Vec<_>>(),
        )
        .expect("canonical hooks take the generated plugin route");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    assert!(
        plan.provided_resource_identities
            .contains(&protect.identity()),
        "the generated plugin covers the package's hooks"
    );
    assert!(plan.evidence.contains("named"));

    // Command-level fallback is honest about the package-only surface.
    let fallback = antigravity.exposure_plan(protect);
    assert_eq!(fallback.route, CompatibilityRoute::Native);
    assert!(matches!(
        fallback.mechanism,
        uze::exposure::ExposureMechanism::Unsupported { .. }
    ));
    let _ = fs::remove_dir_all(_root);
}
