use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use crate::UzeHome;
use crate::application::{
    DoctorReport, MaintenanceReport, MarketplacePluginSummary, PluginSummary,
};

use super::hit::Hit;
use super::management::{clip_line, render};
use super::model::{
    Focus, Overlay, PREFERENCE_ROW_COUNT, ProfilePanel, ROUTES, RefreshData, Route, TrustedRetry,
    TuiModel,
};
use super::view::doctor::{Severity, classify_doctor};
use super::worker::{Intent, TrustGrant};
use super::{ACCENT, MUTED, hint_spans};

fn plugin(id: &str) -> PluginSummary {
    PluginSummary {
        id: id.to_owned(),
        active_name: id.to_owned(),
        source: "embedded:example".to_owned(),
        store_path: PathBuf::from("/store/example"),
        capability_count: 2,
        update_available: None,
    }
}

fn model_with_plugins(ids: &[&str]) -> TuiModel {
    TuiModel {
        plugins: ids.iter().map(|id| plugin(id)).collect(),
        focus: Focus::Content,
        route: Route::Plugins,
        ..TuiModel::default()
    }
}

/// A model with every route's list populated (plugins, marketplace,
/// harnesses) and a mixed-severity doctor report, so rendering each
/// route exercises its non-empty branch rather than only the
/// nothing-loaded-yet placeholder every other test leaves in place.
fn model_with_data() -> TuiModel {
    use crate::application::{
        HarnessContextDelivery, HarnessContextStatus, HarnessHealth, ManagedStateSummary,
        PackageManagedState, Portability, ProjectContextStatus, StoreHealth,
    };
    use uze_core::integration::{AttachmentState, HarnessDetection, PublicationStatus};
    use uze_core::router::HarnessCapabilities;

    let mut model = model_with_plugins(&["one", "two"]);
    model.plugins[0].update_available = Some(true);
    model.marketplace_count = 1;
    model.marketplace_plugins = vec![MarketplacePluginSummary {
        marketplace: "uze-official".to_owned(),
        name: "flow".to_owned(),
        description: Some("A flow plugin".to_owned()),
        keywords: vec!["flow".to_owned()],
        installed: true,
        update_available: Some(false),
        is_default: true,
    }];
    model.doctor = Some(DoctorReport {
        uze_home: PathBuf::from("/home/uze"),
        store: StoreHealth::Ready,
        plugins: model.plugins.clone(),
        harnesses: vec![
            HarnessHealth {
                integration: "claude-code".to_owned(),
                display_name: "Claude Code".to_owned(),
                description: "Anthropic's official coding agent CLI".to_owned(),
                detection: HarnessDetection {
                    present: true,
                    version: Some("1.0.0".to_owned()),
                },
                setup: "configured, verified".to_owned(),
                strategy: Some("managed-user-scope-skills-dir".to_owned()),
                provisioning: None,
                publication: PublicationStatus::Published,
                capabilities: HarnessCapabilities::default(),
                native_instructions: false,
                runtime_shim_active: true,
            },
            HarnessHealth {
                integration: "codex".to_owned(),
                display_name: "Codex".to_owned(),
                description: "OpenAI's coding agent CLI".to_owned(),
                detection: HarnessDetection {
                    present: true,
                    version: Some("0.9.0".to_owned()),
                },
                setup: "not configured".to_owned(),
                strategy: None,
                provisioning: None,
                publication: PublicationStatus::NotApplicable,
                capabilities: HarnessCapabilities::default(),
                native_instructions: true,
                runtime_shim_active: true,
            },
        ],
        attachments: vec![PackageManagedState {
            hooks: Vec::new(),
            plugin: "one".to_owned(),
            state: ManagedStateSummary {
                matched: 1,
                missing: 1,
                drifted: 1,
                conflicts: 1,
                blocked: 0,
                ledger_error: None,
            },
        }],
        ledger_error: None,
        integration_state_error: None,
        provisioning_state_error: None,
        maintenance: MaintenanceReport::default(),
    });
    model.context_status = Some(ProjectContextStatus {
        root: PathBuf::from("/home/project"),
        canonical: PathBuf::from("/home/project/AGENTS.md"),
        sources: Vec::new(),
        contributions: Vec::new(),
        orphaned_regions: Vec::new(),
        malformed_regions: Vec::new(),
        harnesses: vec![
            // Claude Code only ever reads context through a `CLAUDE.md`
            // bridge (never natively) — `needed: false` here means
            // AGENTS.md currently has no matched package contribution
            // to bridge, not that the bridge itself is unhealthy. The
            // regression this guards: a `Matched` bridge must still
            // read "Bridged", never collapse to "Not needed".
            HarnessContextStatus {
                integration: "claude-code".to_owned(),
                display_name: "Claude Code".to_owned(),
                delivery: HarnessContextDelivery::Bridge {
                    needed: false,
                    state: AttachmentState::Matched,
                },
            },
            HarnessContextStatus {
                integration: "codex".to_owned(),
                display_name: "Codex".to_owned(),
                delivery: HarnessContextDelivery::Native,
            },
        ],
        portability: Portability::Portable,
        warnings: vec![
            "AGENTS.md carries a region for a plugin that is no longer installed".to_owned(),
        ],
    });
    model.harnesses_selected = 0;
    model.profiles = vec![
        crate::application::ProfileSummary {
            id: "dev-autonomous".to_owned(),
            description: Some("My daily autonomous coding setup.".to_owned()),
            active: true,
            preferences: uze_core::preference::Preferences {
                autonomy: uze_core::preference::Autonomy::Auto,
                sandbox: uze_core::preference::SandboxScope::WorkspaceWrite,
                model: uze_core::preference::ModelPreference::Default,
            },
        },
        crate::application::ProfileSummary {
            id: "safe-mode".to_owned(),
            description: None,
            active: false,
            preferences: uze_core::preference::Preferences::default(),
        },
    ];
    model.profile_harness_selection = ["claude-code".to_owned(), "codex".to_owned()]
        .into_iter()
        .collect();
    model.profile_harness_defaulted = true;
    model
}

#[test]
fn every_route_renders_without_panicking() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let base = model_with_data();
    for route in ROUTES {
        let model = TuiModel {
            route,
            plugins: base.plugins.clone(),
            plugins_selected: base.plugins_selected,
            marketplace_count: base.marketplace_count,
            marketplace_plugins: base.marketplace_plugins.clone(),
            doctor: base.doctor.clone(),
            harnesses_selected: base.harnesses_selected,
            profiles: base.profiles.clone(),
            profile_harness_selection: base.profile_harness_selection.clone(),
            focus: Focus::Content,
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
    }
}

#[test]
fn every_overlay_renders_without_panicking() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let base = model_with_data();
    let overlays = [
        Overlay::Help,
        Overlay::HarnessHelp,
        Overlay::ConfirmRemove {
            id: "one".to_owned(),
            focus: 1,
        },
        Overlay::ConfirmUpdate("one".to_owned()),
        Overlay::ConfirmInstall {
            name: "flow".to_owned(),
            marketplace: "uze-official".to_owned(),
        },
        Overlay::ConfirmContextApply,
        Overlay::ProtectedPlugin("one".to_owned()),
        Overlay::AddMarketplace("/home/user/marketplace".to_owned()),
        Overlay::NewProfile("dev-autonomous".to_owned()),
        Overlay::ConfirmDeleteProfile {
            id: "default".to_owned(),
            focus: 1,
        },
        Overlay::TrustRequired {
            plugin: "one".to_owned(),
            detail: "one -> mcp-server".to_owned(),
            retry: TrustedRetry::Install {
                name: "one".to_owned(),
                marketplace: "uze-official".to_owned(),
            },
        },
    ];
    for overlay in overlays {
        let model = TuiModel {
            overlay,
            plugins: base.plugins.clone(),
            marketplace_plugins: base.marketplace_plugins.clone(),
            doctor: base.doctor.clone(),
            focus: Focus::Overlay,
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
    }
}

#[test]
fn sidebar_keyboard_navigation_cycles_routes() {
    let mut model = TuiModel {
        focus: Focus::Sidebar,
        ..TuiModel::default()
    };
    assert_eq!(model.route, Route::Overview);
    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(model.route, Route::Marketplace);
    model.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(model.route, Route::Plugins);
    model.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(model.route, Route::Marketplace);
}

#[test]
fn tab_toggles_focus_between_sidebar_and_content() {
    let mut model = TuiModel::default();
    assert_eq!(model.focus, Focus::Sidebar);
    model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(model.focus, Focus::Content);
    model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(model.focus, Focus::Sidebar);
}

#[test]
fn content_navigation_and_inspect_intent() {
    let mut model = model_with_plugins(&["one", "two"]);
    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(model.plugins_selected, 1);
    assert_eq!(
        model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Intent::InspectPlugin("two".to_owned())
    );
}

#[test]
fn remove_confirmation_flow() {
    let mut model = model_with_plugins(&["one"]);
    model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(model.overlay, Overlay::ConfirmRemove { ref id, .. } if id == "one"));
    assert_eq!(model.focus, Focus::Overlay);
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
    assert_eq!(model.overlay, Overlay::None);
    assert_eq!(model.focus, Focus::Content);
}

#[test]
fn remove_confirmed_emits_remove_intent() {
    let mut model = model_with_plugins(&["one"]);
    model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    let intent = model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(intent, Intent::Remove("one".to_owned()));
}

#[test]
fn update_only_offered_when_available() {
    let mut model = model_with_plugins(&["one"]);
    model.apply_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
    assert_eq!(
        model.overlay,
        Overlay::None,
        "no update available, no overlay"
    );
    model.plugins[0].update_available = Some(true);
    model.apply_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
    assert!(matches!(model.overlay, Overlay::ConfirmUpdate(ref id) if id == "one"));
}

#[test]
fn trust_required_overlay_confirm_regrants_with_trust() {
    let mut model = TuiModel {
        overlay: Overlay::TrustRequired {
            plugin: "acme".to_owned(),
            detail: "acme -> mcp-server".to_owned(),
            retry: TrustedRetry::Install {
                name: "acme".to_owned(),
                marketplace: "uze-official".to_owned(),
            },
        },
        focus: Focus::Overlay,
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(
        intent,
        Intent::Install {
            name: "acme".to_owned(),
            marketplace: "uze-official".to_owned(),
            grant: TrustGrant::Granted,
        }
    );
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn mouse_click_on_sidebar_route_switches_route_and_focus() {
    let mut model = TuiModel {
        hits: vec![(Rect::new(0, 1, 20, 1), Hit::Route(Route::Marketplace))],
        ..TuiModel::default()
    };
    let intent = model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(intent, Intent::None);
    assert_eq!(model.route, Route::Marketplace);
    assert_eq!(model.focus, Focus::Content);
}

#[test]
fn mouse_click_on_plugin_row_only_selects_no_fetch() {
    // Clicking a row must behave like arrow-key navigation — select
    // only, no async inspect fetch (and the "Inspecting…" status flash
    // that comes with it) on every single click. Enter still fetches
    // explicitly — see `content_navigation_and_inspect_intent`.
    let mut model = model_with_plugins(&["one", "two"]);
    model.hits = vec![
        (Rect::new(0, 0, 20, 1), Hit::PluginRow(0)),
        (Rect::new(0, 1, 20, 1), Hit::PluginRow(1)),
    ];
    let intent = model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(model.plugins_selected, 1);
    assert_eq!(intent, Intent::None);
}

#[test]
fn scroll_moves_selection_without_mutating_anything() {
    let mut model = model_with_plugins(&["one", "two", "three"]);
    let intent = model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(intent, Intent::None);
    assert_eq!(model.plugins_selected, 1);
}

#[test]
fn click_outside_overlay_dismisses_without_confirming() {
    let mut model = model_with_plugins(&["one"]);
    model.overlay = Overlay::ConfirmRemove {
        id: "one".to_owned(),
        focus: 1,
    };
    model.focus = Focus::Overlay;
    let intent = model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        intent,
        Intent::None,
        "a stray click must never confirm a destructive action"
    );
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn help_overlay_toggle_and_dismiss() {
    let mut model = TuiModel::default();
    model.apply_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(model.overlay, Overlay::Help);
    model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn empty_marketplace_and_no_harness_states_do_not_panic_rendering() {
    let model = TuiModel {
        route: Route::Marketplace,
        ..TuiModel::default()
    };
    assert_eq!(model.list_len(), 0);
    assert!(model.selected_marketplace_plugin().is_none());
    let model = TuiModel {
        route: Route::Harnesses,
        ..TuiModel::default()
    };
    assert!(model.selected_harness().is_none());
}

#[test]
fn read_only_navigation_never_produces_a_mutating_intent() {
    let mut model = model_with_plugins(&["one", "two"]);
    model.set_route(Route::Marketplace);
    model.marketplace_plugins = vec![MarketplacePluginSummary {
        marketplace: "uze-official".to_owned(),
        name: "uze".to_owned(),
        description: None,
        keywords: Vec::new(),
        installed: true,
        update_available: Some(false),
        is_default: true,
    }];
    for key in [
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Char('j'),
        KeyCode::Char('k'),
    ] {
        let intent = model.apply_key(KeyEvent::new(key, KeyModifiers::NONE));
        // Marketplace navigation may dispatch a read-only inspect fetch
        // (keeps the drawer's RESOURCES section populated as selection
        // moves) — that's not a mutation, so only reject the intents
        // that actually write something.
        assert!(
            matches!(
                intent,
                Intent::None | Intent::InspectMarketplacePlugin { .. }
            ),
            "navigation must never produce a mutating intent, got {intent:?}"
        );
    }
}

#[test]
fn profiles_read_only_navigation_never_produces_a_mutating_intent() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    for key in [
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Tab,
        KeyCode::BackTab,
    ] {
        let intent = model.apply_key(KeyEvent::new(key, KeyModifiers::NONE));
        assert_eq!(
            intent,
            Intent::None,
            "Profiles navigation must never mutate, got {intent:?}"
        );
    }
}

#[test]
fn tab_cycles_the_three_profile_panels_while_content_is_focused() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    assert_eq!(model.profile_panel, ProfilePanel::List);
    model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(model.profile_panel, ProfilePanel::Editor);
    model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(model.profile_panel, ProfilePanel::Harnesses);
    model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(model.profile_panel, ProfilePanel::List);
    model.apply_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
    assert_eq!(model.profile_panel, ProfilePanel::Harnesses);
}

#[test]
fn left_right_cycle_the_selected_preference_value_and_persist_it() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::Editor;
    model.profile_editor_selected = 0; // autonomy
    let before = model.profiles[0].preferences.autonomy;
    let intent = model.apply_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_ne!(
        model.profiles[0].preferences.autonomy, before,
        "cycling must mutate optimistically"
    );
    assert!(matches!(intent, Intent::UpdatePreferences { .. }));
    let after_right = model.profiles[0].preferences.autonomy;
    model.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(
        model.profiles[0].preferences.autonomy, before,
        "left must undo right's cycle step"
    );
    let _ = after_right;
}

#[test]
fn left_right_outside_the_editor_panel_falls_back_to_sidebar_focus() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::List;
    model.apply_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(model.focus, Focus::Sidebar);
}

#[test]
fn space_toggles_harness_selection_only_in_the_harnesses_panel() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_harness_selected = 0;
    let harness_id = model.doctor.as_ref().unwrap().harnesses[0]
        .integration
        .clone();
    let was_selected = model.profile_harness_selection.contains(&harness_id);

    // No-op outside the Harnesses panel.
    model.profile_panel = ProfilePanel::List;
    model.apply_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_eq!(
        model.profile_harness_selection.contains(&harness_id),
        was_selected
    );

    model.profile_panel = ProfilePanel::Harnesses;
    model.apply_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert_eq!(
        model.profile_harness_selection.contains(&harness_id),
        !was_selected
    );
}

#[test]
fn n_opens_new_profile_overlay_and_submitting_creates_it() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.apply_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(model.overlay, Overlay::NewProfile(String::new()));
    assert_eq!(model.focus, Focus::Overlay);
    for ch in "Team Backend".chars() {
        model.apply_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let intent = model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(intent, Intent::CreateProfile("team-backend".to_owned()));
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn new_profile_overlay_esc_cancels_without_intent() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.apply_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    for ch in "x".chars() {
        model.apply_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let intent = model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn d_on_the_list_panel_opens_a_delete_confirmation_that_a_stray_click_cannot_confirm() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::List;
    model.profiles_selected = 0;
    let id = model.profiles[0].id.clone();
    model.apply_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    assert!(matches!(
        &model.overlay,
        Overlay::ConfirmDeleteProfile { id: confirmed_id, .. } if *confirmed_id == id
    ));
    assert_eq!(model.focus, Focus::Overlay);

    let intent = model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        intent,
        Intent::None,
        "a stray click must never confirm delete"
    );
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn confirming_delete_with_y_emits_delete_profile_intent() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::List;
    model.profiles_selected = 0;
    let id = model.profiles[0].id.clone();
    model.apply_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::DeleteProfile(id));
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn s_on_the_list_panel_sets_the_selected_profile_active() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::List;
    model.profiles_selected = 1;
    let id = model.profiles[1].id.clone();
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::SetActiveProfile(id));
}

#[test]
fn a_applies_the_selected_profile_to_every_checked_harness() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profiles_selected = 0;
    let id = model.profiles[0].id.clone();
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    let Intent::ApplyProfile {
        id: applied_id,
        harness_ids,
    } = intent
    else {
        panic!("expected ApplyProfile, got a different intent");
    };
    assert_eq!(applied_id, id);
    assert_eq!(harness_ids.len(), model.profile_harness_selection.len());
}

#[test]
fn a_is_inert_with_no_harnesses_selected() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_harness_selection.clear();
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
}

#[test]
fn editor_selection_clamps_to_the_preference_row_count() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.profile_panel = ProfilePanel::Editor;
    for _ in 0..10 {
        model.move_profile_selection(1);
    }
    assert_eq!(model.profile_editor_selected, PREFERENCE_ROW_COUNT - 1);
    for _ in 0..10 {
        model.move_profile_selection(-1);
    }
    assert_eq!(model.profile_editor_selected, 0);
}

#[test]
fn doctor_classifies_conflicts_as_high_and_missing_as_low() {
    use crate::application::{ManagedStateSummary, PackageManagedState};
    let doctor = DoctorReport {
        uze_home: PathBuf::from("/home"),
        store: crate::application::StoreHealth::Ready,
        plugins: Vec::new(),
        harnesses: Vec::new(),
        attachments: vec![PackageManagedState {
            hooks: Vec::new(),
            plugin: "acme".to_owned(),
            state: ManagedStateSummary {
                matched: 0,
                missing: 1,
                drifted: 0,
                conflicts: 1,
                blocked: 0,
                ledger_error: None,
            },
        }],
        ledger_error: None,
        integration_state_error: None,
        provisioning_state_error: None,
        maintenance: MaintenanceReport::default(),
    };
    let issues = classify_doctor(Some(&doctor));
    assert_eq!(issues[0].severity, Severity::High);
    assert!(issues.iter().any(|i| i.severity == Severity::Low));
}

fn marketplace_plugin(marketplace: &str, name: &str, installed: bool) -> MarketplacePluginSummary {
    MarketplacePluginSummary {
        marketplace: marketplace.to_owned(),
        name: name.to_owned(),
        description: None,
        keywords: Vec::new(),
        installed,
        update_available: None,
        is_default: false,
    }
}

#[test]
fn marketplace_filter_narrows_visible_selection() {
    let mut model = TuiModel {
        route: Route::Marketplace,
        focus: Focus::Content,
        marketplace_plugins: vec![
            marketplace_plugin("ai", "std", false),
            marketplace_plugin("ai", "flow", true),
        ],
        ..TuiModel::default()
    };
    assert_eq!(model.marketplace_visible_indices(), vec![0, 1]);

    model.apply_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(model.filtering);
    for c in "flow".chars() {
        model.apply_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(model.marketplace_visible_indices(), vec![1]);
    assert_eq!(model.selected_marketplace_plugin().unwrap().name, "flow");

    model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!model.filtering);
    assert!(model.marketplace_filter.is_empty());
    assert_eq!(model.marketplace_visible_indices(), vec![0, 1]);
}

#[test]
fn marketplace_group_collapse_hides_its_plugins() {
    let mut model = TuiModel {
        route: Route::Marketplace,
        marketplace_plugins: vec![marketplace_plugin("ai", "std", false)],
        ..TuiModel::default()
    };
    assert_eq!(model.list_len(), 1);
    model.marketplace_toggle_group("ai");
    assert_eq!(model.list_len(), 0);
    assert!(model.selected_marketplace_plugin().is_none());
    model.marketplace_toggle_group("ai");
    assert_eq!(model.list_len(), 1);
}

#[test]
fn add_marketplace_overlay_types_and_submits() {
    let mut model = TuiModel {
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
    assert!(matches!(model.overlay, Overlay::AddMarketplace(ref s) if s.is_empty()));

    for c in "/tmp/mp".chars() {
        model.apply_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert!(matches!(model.overlay, Overlay::AddMarketplace(ref s) if s == "/tmp/mp"));

    let intent = model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(intent, Intent::AddMarketplace("/tmp/mp".to_owned()));
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn add_marketplace_overlay_esc_cancels_without_intent() {
    let mut model = TuiModel {
        overlay: Overlay::AddMarketplace("abc".to_owned()),
        focus: Focus::Overlay,
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn r_refreshes_outside_plugins_but_still_removes_within_plugins() {
    let mut model = TuiModel {
        focus: Focus::Content,
        route: Route::Overview,
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::Refresh);

    let mut plugins_model = model_with_plugins(&["one"]);
    let intent = plugins_model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(plugins_model.overlay, Overlay::ConfirmRemove { ref id, .. } if id == "one"));
    assert_eq!(intent, Intent::None);
}

#[test]
fn entering_doctor_route_uses_the_cached_full_report() {
    // Every refresh carries the full doctor (attachments included, via
    // the inspection cache) — navigating to Doctor never needs a
    // special "deep" request, and no screen ever shows a masked
    // "unknown" placeholder for attachment health.
    let mut model = TuiModel {
        focus: Focus::Sidebar,
        ..TuiModel::default()
    };
    model.refreshed(RefreshData {
        doctor: Some(DoctorReport {
            uze_home: PathBuf::from("/home"),
            store: crate::application::StoreHealth::Ready,
            plugins: Vec::new(),
            harnesses: Vec::new(),
            attachments: Vec::new(),
            ledger_error: None,
            integration_state_error: None,
            provisioning_state_error: None,
            maintenance: MaintenanceReport::default(),
        }),
        ..RefreshData::default()
    });

    // Sidebar order: Overview → Marketplace → Plugins → Harnesses →
    // Profiles → Doctor. No deep-request intent anywhere.
    let mut last_intent = Intent::None;
    for _ in 0..5 {
        last_intent = model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(model.route, Route::Doctor);
    assert_eq!(
        last_intent,
        Intent::None,
        "entering Doctor must not spawn a second, separate deep reload"
    );
}

#[test]
fn attachment_health_is_never_unknown_after_a_refresh() {
    use crate::application::{ManagedStateSummary, PackageManagedState};
    // Every refresh carries the full doctor with attachments (served by
    // the inspection cache), so the Plugins screen derives "ready"
    // instead of showing the masked "unknown" placeholder.
    let mut model = model_with_plugins(&["one"]);
    model.route = Route::Plugins;
    model.doctor = Some(DoctorReport {
        uze_home: PathBuf::from("/home"),
        store: crate::application::StoreHealth::Ready,
        plugins: vec![plugin("one")],
        harnesses: Vec::new(),
        attachments: vec![PackageManagedState {
            hooks: Vec::new(),
            plugin: "one".to_owned(),
            state: ManagedStateSummary {
                matched: 2,
                missing: 0,
                drifted: 0,
                conflicts: 0,
                blocked: 0,
                ledger_error: None,
            },
        }],
        ledger_error: None,
        integration_state_error: None,
        provisioning_state_error: None,
        maintenance: MaintenanceReport::default(),
    });
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(
        rows.iter().any(|row| row.contains("ready")),
        "a refreshed report must render real health, got:\n{rows:#?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("unknown")),
        "attachment health must never read 'unknown' after a refresh"
    );
}

#[test]
fn footer_hint_styles_commands_with_accent_and_descriptions_muted() {
    use ratatui::{
        style::{Modifier, Style},
        text::Line,
    };

    let line = Line::from(hint_spans("↑↓ select · enter inspect · esc back"));
    let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(content, "↑↓ select · enter inspect · esc back");
    // Chunks split as key/description: command accent+bold, verb muted,
    // with raw " · " separators between chunks.
    assert_eq!(line.spans.len(), 8);
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    );
    assert_eq!(line.spans[0].content.as_ref(), "↑↓");
    assert_eq!(line.spans[1].style, Style::default().fg(MUTED));
    assert_eq!(line.spans[1].content.as_ref(), " select");
    assert_eq!(line.spans[2].content.as_ref(), " · ");
    assert_eq!(line.spans[6].content.as_ref(), "esc");
    assert_eq!(line.spans[6].style.fg, Some(ACCENT));

    // A command-only chunk (no verb) still carries the accent.
    let line = Line::from(hint_spans("tab switch · y/n"));
    assert_eq!(line.spans.len(), 4);
    assert_eq!(line.spans[3].content.as_ref(), "y/n");
    assert_eq!(line.spans[3].style.fg, Some(ACCENT));
}

#[test]
fn sidebar_work_toggle_click_mirrors_ctrl_o() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut model = TuiModel::default();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let intent = model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 8,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        intent,
        Intent::SwitchToWorkspace,
        "clicking the sidebar's 'work' segment must mirror the Ctrl+O keybinding"
    );
}

#[test]
fn sidebar_resize_drag_updates_width() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut model = TuiModel::default();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    // Mousedown on the sidebar's right-border drag handle (x=31 for a
    // 100-wide terminal: the default 32-column sidebar's right edge) arms
    // dragging, same as the workspace TUI's `WorkspaceHit::ResizeSidebar`.
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 31,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert!(model.dragging_sidebar);

    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 55,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        model.sidebar_width,
        Some(24),
        "dragging the handle to column 55 (24 columns past the sidebar's x=0 origin) must set that width"
    );
}

#[test]
fn sidebar_resize_drag_clamps_to_bounds() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut model = TuiModel::default();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 31,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );

    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 95,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        model.sidebar_width,
        Some(super::MAX_SIDEBAR_WIDTH),
        "dragging far past the terminal's edge must clamp to the shared max, same as the workspace sidebar"
    );

    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 1,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        model.sidebar_width,
        Some(super::MIN_SIDEBAR_WIDTH),
        "dragging past the left edge must clamp to the shared min"
    );
}

#[test]
fn clip_line_truncates_long_status_with_ellipsis() {
    use ratatui::text::Line;

    let mut line =
        Line::from("Installed plugin root: /home/user/.codex/plugins/cache/very/long/path");
    clip_line(&mut line, 20);
    let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(content, "Installed plugin ro…");
    assert_eq!(ratatui::text::Span::raw(&content).width(), 20);

    let mut line = Line::from("Installed uze");
    clip_line(&mut line, 20);
    let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(content, "Installed uze");
}

// --- Overview workspace awareness ---------------------------------------

use crate::application::{
    MarketplaceState, MemoryState, OverviewMarketplace, OverviewWorkspaceSummary,
    ProjectEnvironmentState, ProjectOverview, WorkspaceKind,
};
fn consumer_workspace(
    state: ProjectEnvironmentState,
    declared: usize,
    installed: usize,
    missing: &[&str],
    root: &std::path::Path,
) -> OverviewWorkspaceSummary {
    OverviewWorkspaceSummary {
        cwd: root.to_path_buf(),
        root: root.to_path_buf(),
        kind: WorkspaceKind::Consumer,
        project: ProjectOverview {
            environment: state,
            memory: MemoryState::Ready,
            declared_plugins: declared,
            installed_plugins: installed,
            missing_plugins: missing.iter().map(ToString::to_string).collect(),
        },
        marketplace: None,
    }
}

fn marketplace_workspace(root: &std::path::Path) -> OverviewWorkspaceSummary {
    OverviewWorkspaceSummary {
        cwd: root.to_path_buf(),
        root: root.to_path_buf(),
        kind: WorkspaceKind::Marketplace,
        project: ProjectOverview {
            environment: ProjectEnvironmentState::NotConfigured,
            memory: MemoryState::None,
            declared_plugins: 0,
            installed_plugins: 0,
            missing_plugins: Vec::new(),
        },
        marketplace: Some(OverviewMarketplace {
            name: Some("acme".to_owned()),
            package_count: 1,
            invalid_packages: 0,
            state: MarketplaceState::Valid,
        }),
    }
}

#[test]
fn overview_install_key_emits_install_intent_with_workspace_root() {
    let root = std::path::PathBuf::from("/tmp/project");
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::InstallRequired,
            4,
            3,
            &["flow"],
            &root,
        )),
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::InstallProjectEnvironment(root));
}

#[test]
fn overview_install_key_is_inert_when_environment_is_ready() {
    let root = std::path::PathBuf::from("/tmp/project");
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::Ready,
            2,
            2,
            &[],
            &root,
        )),
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
}

#[test]
fn overview_install_key_is_inert_outside_consumer_workspaces() {
    let root = std::path::PathBuf::from("/tmp/market");
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(marketplace_workspace(&root)),
        ..TuiModel::default()
    };
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert_eq!(
        intent,
        Intent::None,
        "marketplace health must not offer `uze install`"
    );
}

#[test]
fn refreshed_updates_workspace_state() {
    let root = std::path::PathBuf::from("/tmp/project");
    let mut model = TuiModel {
        route: Route::Overview,
        ..TuiModel::default()
    };
    assert!(model.workspace.is_none());
    model.refreshed(RefreshData {
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::InstallRequired,
            4,
            3,
            &["flow"],
            &root,
        )),
        ..RefreshData::default()
    });
    assert_eq!(model.overview_install_path(), Some(root.clone()));

    model.refreshed(RefreshData {
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::Ready,
            4,
            4,
            &[],
            &root,
        )),
        ..RefreshData::default()
    });
    assert_eq!(
        model.overview_install_path(),
        None,
        "refresh must reflect a completed install"
    );
}

/// All rows of the rendered buffer, right-trimmed — the cheap,
/// snapshot-free way to assert on what the TUI actually drew.
fn buffer_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (area.y..area.y + area.height)
        .map(|row| {
            let mut line = String::new();
            for column in area.x..area.x + area.width {
                line.push_str(buffer[(column, row)].symbol());
            }
            line.trim_end().to_string()
        })
        .collect()
}

fn overview_model(root: &std::path::Path) -> TuiModel {
    // Hybrid workspace: both columns exist, so layout tests can assert
    // on PROJECT vs MARKETPLACE placement.
    let mut workspace = consumer_workspace(
        ProjectEnvironmentState::InstallRequired,
        4,
        3,
        &["flow"],
        root,
    );
    workspace.kind = WorkspaceKind::Hybrid;
    workspace.marketplace = Some(OverviewMarketplace {
        name: Some("acme".to_owned()),
        package_count: 2,
        invalid_packages: 0,
        state: MarketplaceState::Valid,
    });
    TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(workspace),
        ..TuiModel::default()
    }
}

#[test]
fn wide_terminal_stacks_workspace_rows() {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let model = overview_model(std::path::Path::new("/tmp/project"));
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    let project_row = rows
        .iter()
        .position(|row| row.contains("PROJECT"))
        .expect("PROJECT heading");
    let marketplace_row = rows
        .iter()
        .position(|row| row.contains("MARKETPLACE"))
        .expect("MARKETPLACE heading");
    assert!(
        project_row < marketplace_row,
        "workspace blocks always stack: PROJECT above MARKETPLACE, on wide terminals too"
    );
}

#[test]
fn narrow_terminal_stacks_workspace_rows() {
    let mut terminal = Terminal::new(TestBackend::new(60, 40)).unwrap();
    let model = overview_model(std::path::Path::new("/tmp/project"));
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    let project_row = rows
        .iter()
        .position(|row| row.contains("PROJECT"))
        .expect("PROJECT heading");
    let marketplace_row = rows
        .iter()
        .position(|row| row.contains("MARKETPLACE"))
        .expect("MARKETPLACE heading");
    assert!(
        project_row < marketplace_row,
        "narrow layout stacks PROJECT above MARKETPLACE"
    );
}

#[test]
fn install_action_only_rendered_when_dependencies_are_missing() {
    let root = std::path::Path::new("/tmp/project");
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::InstallRequired,
            4,
            3,
            &["flow"],
            root,
        )),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(rows.iter().any(|row| row.contains("i install")));
    assert!(rows.iter().any(|row| row.contains("! install required")));
    assert!(rows.iter().any(|row| row.contains("! 3/4 installed")));

    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::Ready,
            4,
            4,
            &[],
            root,
        )),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(
        !rows.iter().any(|row| row.contains("i install")),
        "a ready environment must not offer a useless install action"
    );
    assert!(rows.iter().any(|row| row.contains("✓ ready")));
    assert!(rows.iter().any(|row| row.contains("4 installed")));
}

#[test]
fn overview_renders_application_verdicts_verbatim() {
    let root = std::path::Path::new("/tmp/project");
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();

    // Invalid lock: the Application says Invalid, the view renders it —
    // and there is no install action for a state the system cannot read.
    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::Invalid,
            0,
            0,
            &[],
            root,
        )),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(rows.iter().any(|row| row.contains("× invalid")));
    assert!(rows.iter().any(|row| row.contains("— unknown")));
    assert!(!rows.iter().any(|row| row.contains("i install")));

    // Marketplace with an invalid manifest: state rendered verbatim.
    let mut bad_market = marketplace_workspace(root);
    bad_market.marketplace = Some(OverviewMarketplace {
        name: None,
        package_count: 0,
        invalid_packages: 0,
        state: MarketplaceState::InvalidManifest,
    });
    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(bad_market),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(rows.iter().any(|row| row.contains("× invalid manifest")));
    assert!(
        rows.iter().any(|row| row.contains("— unknown")),
        "an unparseable manifest cannot name itself"
    );
    assert!(
        !rows.iter().any(|row| row.contains("PROJECT")),
        "a pure marketplace workspace must not show a PROJECT column"
    );
}

#[test]
fn consumer_only_workspace_hides_marketplace_column() {
    let root = std::path::Path::new("/tmp/project");
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::Ready,
            2,
            2,
            &[],
            root,
        )),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(rows.iter().any(|row| row.contains("PROJECT")));
    assert!(
        !rows.iter().any(|row| row.contains("MARKETPLACE")),
        "no marketplace.json → no MARKETPLACE column, even in a consumer"
    );
}

#[test]
fn overview_render_does_not_mutate_project_state() {
    use ratatui::{Terminal, backend::TestBackend};

    let base = std::env::temp_dir().join(format!(
        "uze-ui-overview-immutable-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let lock_path = root.join("agents.lock");
    let lock_bytes = b"version: 1\nplugins: {}\n";
    std::fs::write(&lock_path, lock_bytes).unwrap();
    let manifest_bytes = br#"{"name":"m","plugins":[]}"#;
    std::fs::write(root.join("marketplace.json"), manifest_bytes).unwrap();
    let agents_md = b"# hi\n";
    std::fs::write(root.join("AGENTS.md"), agents_md).unwrap();

    let model = TuiModel {
        route: Route::Overview,
        context_root: root.clone(),
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::Ready,
            2,
            2,
            &[],
            &root,
        )),
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);

    // The render must leave the workspace exactly as found.
    assert_eq!(std::fs::read(&lock_path).unwrap(), lock_bytes);
    assert_eq!(
        std::fs::read(root.join("marketplace.json")).unwrap(),
        manifest_bytes
    );
    assert_eq!(std::fs::read(root.join("AGENTS.md")).unwrap(), agents_md);
    // And the semantic rows actually rendered (sanity, not just no-op).
    assert!(rows.iter().any(|row| row.contains("Environment")));
    assert!(rows.iter().any(|row| row.contains("Memory")));
    assert!(rows.iter().any(|row| row.contains("Plugins")));
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn no_workspace_render_creates_nothing() {
    use ratatui::{Terminal, backend::TestBackend};

    let base = std::env::temp_dir().join(format!(
        "uze-ui-noworkspace-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = base.join("random");
    std::fs::create_dir_all(&root).unwrap();

    let model = TuiModel {
        route: Route::Overview,
        context_root: root.clone(),
        workspace: Some(OverviewWorkspaceSummary {
            cwd: root.clone(),
            root: root.clone(),
            kind: WorkspaceKind::NoWorkspace,
            project: ProjectOverview {
                environment: ProjectEnvironmentState::NotConfigured,
                memory: MemoryState::None,
                declared_plugins: 0,
                installed_plugins: 0,
                missing_plugins: Vec::new(),
            },
            marketplace: None,
        }),
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    assert!(rows.iter().any(|row| row.contains("— not configured")));
    assert!(rows.iter().any(|row| row.contains("PROJECT")));
    assert!(
        !rows.iter().any(|row| row.contains("MARKETPLACE")),
        "no marketplace.json in a plain directory → no MARKETPLACE column"
    );

    assert!(
        !root.join("agents.lock").exists(),
        "rendering must never create a project lock"
    );
    assert!(
        !root.join("marketplace.json").exists(),
        "rendering must never create a marketplace manifest"
    );
    let entries: Vec<_> = std::fs::read_dir(&root).unwrap().collect();
    assert!(
        entries.is_empty(),
        "a NoWorkspace render must leave the directory untouched"
    );
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn overview_install_intent_reaches_install_project_environment() {
    use std::sync::mpsc;
    use std::time::Duration;

    // `dispatch` builds its application through
    // `UzeApplication::from_env_with_runner`, whose integrations read
    // process-global environment. Use the testkit-wide guard so concurrent
    // tests which need a real executable on PATH cannot observe this setup.
    let mut environment = uze_testkit::env::scope();

    let base = std::env::temp_dir().join(format!(
        "uze-ui-install-dispatch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let home = base.join("home");
    let project = base.join("project");
    let market = base.join("market");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(market.join("flow/skills/uze-test")).unwrap();
    std::fs::write(
        market.join("marketplace.json"),
        r#"{"name":"test","plugins":[{"name":"flow","source":"flow"}]}"#,
    )
    .unwrap();
    std::fs::write(market.join("flow/plugin.json"), r#"{"name":"flow"}"#).unwrap();
    std::fs::write(market.join("flow/skills/uze-test/SKILL.md"), "# s\n").unwrap();
    let lock = uze_core::project_lock::ProjectLock {
        version: 1,
        worktrees_dir: None,
        marketplaces: std::iter::once((
            "test".to_owned(),
            uze_core::project_lock::LockedMarketplace {
                source: uze_core::project_lock::MarketplaceSource::Path { path: market },
                resolved: uze_core::project_lock::ResolvedMarketplace::default(),
            },
        ))
        .collect(),
        plugins: std::iter::once((
            "flow".to_owned(),
            uze_core::project_lock::LockedPlugin {
                source: uze_core::project_lock::PluginSource::Marketplace {
                    marketplace: "test".to_owned(),
                    plugin: "flow".to_owned(),
                },
                resolved: uze_core::project_lock::ResolvedPlugin {
                    revision: None,
                    version: None,
                    integrity: None,
                },
            },
        ))
        .collect(),
    };
    uze_core::project_lock::save_lock(&project, &lock).unwrap();

    environment.set("HOME", &base);
    environment.set("UZE_HOME", &home);
    // Isolate PATH to a directory with nothing on it: on a machine
    // where `uze setup claude` has ever actually run, the real
    // `~/.uze/shims/claude` sits ahead of everything else on the
    // ambient PATH this test process inherited. That shim resolves to
    // this very `uze` binary (not a vendor CLI), and it is excluded
    // from `resolve_real_executable`'s walk only by comparing against
    // *this test's* fake `shims_dir` — never the developer's real one.
    // Left unisolated, the install path below shells out to `uze`
    // itself expecting Claude Code's CLI and gets `uze`'s own `--help`
    // usage back. Every harness must read as absent here, matching a
    // clean machine.
    let empty_path_dir = base.join("empty-path");
    std::fs::create_dir_all(&empty_path_dir).unwrap();
    environment.set("PATH", &empty_path_dir);

    let uze_home = UzeHome::at(&home);
    let mut model = TuiModel {
        route: Route::Overview,
        context_root: project.clone(),
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::InstallRequired,
            1,
            0,
            &["flow"],
            &project,
        )),
        ..TuiModel::default()
    };
    let (sender, receiver) = mpsc::channel();
    super::worker::dispatch(
        Intent::InstallProjectEnvironment(project),
        &uze_home,
        &sender,
        &mut model,
    );
    let result = receiver.recv_timeout(Duration::from_secs(30)).unwrap();
    match result {
        super::worker::WorkerResult::Mutated(Ok((message, data))) => {
            assert!(
                message.contains("Installed"),
                "install must report success, got {message}"
            );
            let workspace = data.workspace.expect("refresh carries workspace state");
            let project = &workspace.project;
            assert_eq!(
                project.environment,
                ProjectEnvironmentState::Ready,
                "after install the Application must report Ready"
            );
            assert_eq!(
                (project.declared_plugins, project.installed_plugins),
                (1, 1)
            );
            assert!(project.missing_plugins.is_empty());
        }
        super::worker::WorkerResult::Mutated(Err(error)) => {
            panic!("expected Mutated(Ok(..)), got Mutated(Err({error}))")
        }
        super::worker::WorkerResult::TrustRequired { plugin, detail, .. } => {
            panic!(
                "expected Mutated(Ok(..)), got TrustRequired {{ plugin: {plugin}, detail: {detail} }}"
            )
        }
        _ => panic!("expected Mutated(Ok(..)), got a different WorkerResult variant"),
    }

    std::fs::remove_dir_all(&base).ok();
}
