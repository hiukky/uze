use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};

use uze_application::UzeHome;
use uze_application::application::{
    DoctorReport, MaintenanceReport, MarketplacePluginSummary, MarketplaceSummary, PluginSummary,
};

use super::hit::Hit;
use super::management::{clip_line, render};
use super::model::{
    Focus, Overlay, PREFERENCE_ROW_COUNT, ProfilePanel, ROUTES, RefreshData, Route, TrustedRetry,
    TuiModel,
};
use super::view::health::{Severity, actionable_alerts};
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
    use uze_application::application::{
        ContextMechanism, HarnessContextDelivery, HarnessContextStatus, HarnessContextSupport,
        HarnessHealth, ManagedStateSummary, PackageManagedState, Portability, ProjectContextStatus,
        StoreHealth,
    };
    use uze_core::integration::{AttachmentState, HarnessDetection, PublicationStatus};
    use uze_core::router::HarnessCapabilities;

    let mut model = model_with_plugins(&["one", "two"]);
    model.plugins[0].update_available = Some(true);
    model.marketplaces = vec![MarketplaceSummary {
        name: "uze-official".to_owned(),
        source: "embedded:uze-official".to_owned(),
        homepage: Some("https://github.com/hiukky/uze".to_owned()),
        plugin_count: 1,
    }];
    model.marketplace_plugins = vec![MarketplacePluginSummary {
        marketplace: "uze-official".to_owned(),
        name: "flow".to_owned(),
        description: Some("A flow plugin".to_owned()),
        keywords: vec!["flow".to_owned()],
        installed: true,
        update_available: Some(false),
        is_default: true,
    }];
    // Renders the "Updated" badge branch on every route that shows plugin
    // rows, alongside the "Update available" one `plugins[0]` carries.
    model.update_badges = vec![super::model::UpdateBadge {
        plugin: "flow@uze-official".to_owned(),
        seen_at: None,
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
                runtime_shim_active: true,
                context_support: HarnessContextSupport {
                    instructions: ContextMechanism::RuntimeShim,
                    agents_directory: ContextMechanism::RuntimeShim,
                },
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
                runtime_shim_active: true,
                context_support: HarnessContextSupport {
                    instructions: ContextMechanism::RuntimeShim,
                    agents_directory: ContextMechanism::RuntimeShim,
                },
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
        worktrees: None,
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
        uze_application::application::ProfileSummary {
            id: "dev-autonomous".to_owned(),
            description: Some("My daily autonomous coding setup.".to_owned()),
            active: true,
            preferences: uze_core::preference::Preferences {
                autonomy: uze_core::preference::Autonomy::Auto,
                sandbox: uze_core::preference::SandboxScope::WorkspaceWrite,
                model: uze_core::preference::ModelPreference::Default,
            },
        },
        uze_application::application::ProfileSummary {
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
            marketplaces: base.marketplaces.clone(),
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
        Overlay::ConfirmClearPromptHistory,
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
    assert_eq!(model.route, Route::Plugins);
    model.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(model.route, Route::Extensions);
    model.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(model.route, Route::Plugins);
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
    assert_eq!(model.marketplace_selected, 1);
    assert_eq!(
        model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Intent::InspectPlugin("two".to_owned())
    );
}

/// The drawer opens by default on whichever row is selected, and the
/// list itself lands from a background refresh — so the first selection
/// is never "navigated to", and nothing else would ask for its detail.
#[test]
fn an_open_drawer_asks_for_the_detail_it_is_missing_exactly_once() {
    let mut model = model_with_plugins(&["one", "two"]);
    let wanted = Intent::InspectPlugin("one".to_owned());
    assert_eq!(model.drawer_inspect_intent(), wanted, "nothing fetched yet");

    model.inspection_in_flight = Some(wanted.clone());
    assert_eq!(
        model.drawer_inspect_intent(),
        Intent::None,
        "the same fetch is not queued again while it runs"
    );

    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        model.drawer_inspect_intent(),
        Intent::InspectPlugin("two".to_owned()),
        "moving the selection wants the new row's detail even mid-flight"
    );

    model.marketplace_drawer_open = false;
    assert_eq!(
        model.drawer_inspect_intent(),
        Intent::None,
        "a closed drawer needs nothing"
    );
    model.marketplace_drawer_open = true;
    model.route = Route::Overview;
    assert_eq!(
        model.drawer_inspect_intent(),
        Intent::None,
        "nor does another screen"
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
fn an_auto_updated_plugin_badges_until_the_plugins_screen_has_shown_it() {
    use super::model::{RefreshData, UPDATE_BADGE_TTL, UpdateBadge};

    let mut model = model_with_plugins(&["one"]);
    model.route = Route::Overview;
    model.refreshed(RefreshData {
        auto_updated: vec!["one".to_owned()],
        ..RefreshData::default()
    });
    assert!(model.was_just_updated("one"));

    // Off the Plugins screen the badge never starts its countdown — an
    // operator who has not looked at it yet has not been told anything.
    for _ in 0..3 {
        model.expire_update_badges();
    }
    assert!(
        model.update_badges[0].seen_at.is_none(),
        "the countdown starts on sight, not on the update"
    );
    assert!(model.was_just_updated("one"));

    model.route = Route::Plugins;
    model.expire_update_badges();
    assert!(model.update_badges[0].seen_at.is_some());

    // Once seen, it comes down on its own.
    model.update_badges[0].seen_at = Some(std::time::Instant::now() - UPDATE_BADGE_TTL);
    model.expire_update_badges();
    assert!(
        !model.was_just_updated("one"),
        "the badge expires after its TTL"
    );

    // An ordinary refresh reports no auto-updates and must not re-raise
    // a badge that already had its moment.
    model.update_badges.push(UpdateBadge {
        plugin: "two".to_owned(),
        seen_at: None,
    });
    model.refreshed(RefreshData::default());
    assert!(
        model.was_just_updated("two"),
        "a live badge survives a plain refresh"
    );
    assert!(!model.was_just_updated("one"));
}

#[test]
fn a_route_action_key_works_from_the_sidebar_too() {
    let mut model = model_with_plugins(&["one"]);
    model.plugins[0].update_available = Some(true);
    model.focus = Focus::Sidebar;
    model.apply_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
    assert!(
        matches!(model.overlay, Overlay::ConfirmUpdate(ref id) if id == "one"),
        "`u` must not be swallowed just because the sidebar holds focus"
    );
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
        hits: vec![(Rect::new(0, 1, 20, 1), Hit::Route(Route::Plugins))],
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
    assert_eq!(model.route, Route::Plugins);
    assert_eq!(model.focus, Focus::Content);
}

#[test]
fn mouse_click_on_extension_row_selects_and_opens_drawer_without_fetch() {
    // Clicking an extension row behaves like arrow-key navigation —
    // selection opens the drawer, but never an async fetch (there is
    // nothing to fetch: the catalog is static, and no "Inspecting…"
    // status flash belongs on every click).
    let mut model = TuiModel {
        focus: Focus::Content,
        ..TuiModel::default()
    };
    model.hits = vec![
        (Rect::new(0, 0, 20, 1), Hit::ExtensionRow(0)),
        (Rect::new(0, 1, 20, 1), Hit::ExtensionRow(1)),
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
    assert_eq!(model.extensions_selected, 1);
    assert!(model.extension_drawer_open);
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
    // Scroll on the Plugins tree is read-only navigation: it moves the
    // selection and fetches the newly selected (installed, local) row's
    // detail — never a mutation.
    assert_eq!(intent, Intent::InspectPlugin("two".to_owned()));
    assert_eq!(model.marketplace_selected, 1);
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
        route: Route::Plugins,
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
    model.set_route(Route::Plugins);
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
        // Plugins navigation may dispatch a read-only inspect fetch
        // (keeps the drawer's RESOURCES/deliveries sections populated as
        // selection moves) — that's not a mutation, so only reject the
        // intents that actually write something.
        assert!(
            matches!(
                intent,
                Intent::None | Intent::InspectMarketplacePlugin { .. } | Intent::InspectPlugin(..)
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
fn clicking_new_profile_opens_the_profile_overlay() {
    let mut model = model_with_data();
    model.hits = vec![(Rect::new(10, 4, 5, 1), Hit::NewProfile)];

    assert_eq!(model.click(12, 4), Intent::None);
    assert_eq!(model.overlay, Overlay::NewProfile(String::new()));
    assert_eq!(model.focus, Focus::Overlay);
}

#[test]
fn clicking_remove_profile_opens_the_delete_confirmation() {
    let mut model = model_with_data();
    let id = model.profiles[0].id.clone();
    model.hits = vec![(Rect::new(10, 4, 8, 1), Hit::DeleteSelectedProfile)];

    assert_eq!(model.click(12, 4), Intent::None);
    assert!(matches!(
        &model.overlay,
        Overlay::ConfirmDeleteProfile { id: confirmed_id, .. } if *confirmed_id == id
    ));
    assert_eq!(model.focus, Focus::Overlay);
}

#[test]
fn clicking_apply_on_an_inactive_profile_targets_checked_harnesses() {
    let mut model = model_with_data();
    model.profiles[0].active = false;
    let id = model.profiles[0].id.clone();
    model.hits = vec![(Rect::new(18, 4, 3, 1), Hit::ApplySelectedProfile)];

    let Intent::ApplyProfile {
        id: applied_id,
        harness_ids,
    } = model.click(19, 4)
    else {
        panic!("expected ApplyProfile");
    };
    assert_eq!(applied_id, id);
    assert_eq!(harness_ids.len(), model.profile_harness_selection.len());
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
fn a_is_inert_on_the_profiles_screen() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profiles_selected = 0;
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
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
fn overview_alerts_classify_conflicts_as_high_and_missing_as_low() {
    use uze_application::application::{ManagedStateSummary, PackageManagedState};
    let doctor = DoctorReport {
        uze_home: PathBuf::from("/home"),
        store: uze_application::application::StoreHealth::Ready,
        plugins: Vec::new(),
        harnesses: Vec::new(),
        attachments: vec![
            PackageManagedState {
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
            },
            PackageManagedState {
                hooks: Vec::new(),
                plugin: "example".to_owned(),
                state: ManagedStateSummary {
                    matched: 0,
                    missing: 1,
                    drifted: 0,
                    conflicts: 0,
                    blocked: 0,
                    ledger_error: None,
                },
            },
        ],
        ledger_error: None,
        integration_state_error: None,
        provisioning_state_error: None,
        maintenance: MaintenanceReport::default(),
    };
    let alerts = actionable_alerts(Some(&doctor));
    assert_eq!(alerts[0].severity, Severity::High);
    assert!(alerts.iter().any(|alert| alert.severity == Severity::Low));
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
        route: Route::Plugins,
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
fn extension_filter_narrows_visible_selection() {
    use uze_extensions::registry::BuiltinExtension;

    let mut model = TuiModel {
        route: Route::Extensions,
        focus: Focus::Content,
        extensions: vec![
            BuiltinExtension {
                id: "git",
                name: "Git",
                description: "Review the working tree",
                surface: "Workspace TUI",
                usage: "Open from the tab strip",
            },
            BuiltinExtension {
                id: "task-list",
                name: "Task List",
                description: "Track workspace tasks",
                surface: "Management TUI",
                usage: "Open from the sidebar",
            },
        ],
        ..TuiModel::default()
    };

    model.apply_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for c in "task".chars() {
        model.apply_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(model.extension_visible_indices(), vec![1]);
    assert_eq!(model.selected_extension().unwrap().name, "Task List");

    model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(model.extension_filter.is_empty());
    assert_eq!(model.extension_visible_indices(), vec![0, 1]);
}

#[test]
fn marketplace_group_collapse_hides_its_plugins() {
    let mut model = TuiModel {
        route: Route::Plugins,
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

/// The Source card names where a plugin's marketplace actually lives, and
/// the address itself is what opens it — a whole row of target, not a
/// one-column glyph you miss by moving the mouse one cell. The card used
/// to show no address at all, and its "↗" only ever jumped to a group
/// header in the list below.
#[test]
fn the_source_card_shows_the_marketplace_link_and_offers_to_open_it() {
    let mut model = model_with_plugins(&["one"]);
    model.route = Route::Plugins;
    model.marketplace_drawer_open = true;
    model.marketplaces = vec![MarketplaceSummary {
        name: "uze-official".to_owned(),
        source: "embedded:uze-official".to_owned(),
        homepage: Some("https://github.com/hiukky/uze".to_owned()),
        plugin_count: 1,
    }];
    model.marketplace_plugins = vec![MarketplacePluginSummary {
        marketplace: "uze-official".to_owned(),
        name: "flow".to_owned(),
        description: Some("A flow plugin".to_owned()),
        keywords: Vec::new(),
        installed: true,
        update_available: Some(false),
        is_default: true,
    }];

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let rows = buffer_rows(&terminal);
    assert!(
        rows.iter()
            .any(|row| row.contains("https://github.com/hiukky/uze")),
        "the address reads on the card: {rows:#?}"
    );

    let rect = model
        .hits
        .iter()
        .find(|(_, hit)| matches!(hit, Hit::OpenLink(name) if name == "uze-official"))
        .map(|(rect, _)| *rect)
        .expect("the address is a target of its own");
    assert!(
        rect.width > 20,
        "and the whole row of it, not one column: {rect:?}"
    );
    for column in [rect.x, rect.x + rect.width / 2, rect.right() - 1] {
        assert_eq!(
            model.click(column, rect.y),
            Intent::OpenLink("https://github.com/hiukky/uze".to_owned()),
            "clicking anywhere along it hands the address over"
        );
    }
}

/// A description long enough to fold used to push every drawn row of the
/// drawer down while the hit rects stayed where the authored line count
/// put them: the address read as a link and answered nothing, because the
/// row the reader clicked was two rows below the target.
#[test]
fn the_source_link_is_clickable_on_the_row_it_is_drawn_on() {
    let mut model = model_with_plugins(&["one"]);
    model.route = Route::Plugins;
    model.marketplace_drawer_open = true;
    model.marketplaces = vec![MarketplaceSummary {
        name: "uze-official".to_owned(),
        source: "embedded:uze-official".to_owned(),
        homepage: Some("https://github.com/hiukky/uze".to_owned()),
        plugin_count: 1,
    }];
    model.marketplace_plugins = vec![MarketplacePluginSummary {
        marketplace: "uze-official".to_owned(),
        name: "flow".to_owned(),
        description: Some(
            "Makes this project's instructions portable across every harness \
             uze knows about, so switching agents never costs the context \
             the project already wrote down."
                .to_owned(),
        ),
        keywords: vec!["context".to_owned(), "portability".to_owned()],
        installed: true,
        update_available: Some(false),
        is_default: true,
    }];

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    let rows = buffer_rows(&terminal);
    let drawn = rows
        .iter()
        .position(|row| row.contains("https://github.com/hiukky/uze"))
        .expect("the address reads on the card") as u16;
    let rect = model
        .hits
        .iter()
        .find(|(_, hit)| matches!(hit, Hit::OpenLink(name) if name == "uze-official"))
        .map(|(rect, _)| *rect)
        .expect("the address is a target of its own");
    assert_eq!(
        rect.y, drawn,
        "the target sits on the row the address is drawn on: {rows:#?}"
    );
    assert_eq!(
        model.click(rect.x + 1, drawn),
        Intent::OpenLink("https://github.com/hiukky/uze".to_owned()),
    );
}

/// The address is chrome until the pointer is on it: muted at rest, accent
/// under the pointer. Hover and click read the same hit list, so a row that
/// lights up is a row that answers.
#[test]
fn the_source_link_lights_up_only_under_the_pointer() {
    let mut model = model_with_plugins(&["one"]);
    model.route = Route::Plugins;
    model.marketplace_drawer_open = true;
    model.marketplaces = vec![MarketplaceSummary {
        name: "uze-official".to_owned(),
        source: "embedded:uze-official".to_owned(),
        homepage: Some("https://github.com/hiukky/uze".to_owned()),
        plugin_count: 1,
    }];
    model.marketplace_plugins = vec![MarketplacePluginSummary {
        marketplace: "uze-official".to_owned(),
        name: "flow".to_owned(),
        description: Some("A flow plugin".to_owned()),
        keywords: Vec::new(),
        installed: true,
        update_available: Some(false),
        is_default: true,
    }];

    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let rect = model
        .hits
        .iter()
        .find(|(_, hit)| matches!(hit, Hit::OpenLink(name) if name == "uze-official"))
        .map(|(rect, _)| *rect)
        .expect("the address is a target of its own");

    assert!(
        !model.source_link_hovered,
        "muted until the pointer arrives"
    );
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: rect.x + 1,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert!(
        model.source_link_hovered,
        "and lit while the pointer is on it"
    );
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: rect.x + 1,
            row: rect.y + 1,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert!(!model.source_link_hovered, "muted again once it leaves");
}

#[test]
fn attachment_health_is_never_unknown_after_a_refresh() {
    use uze_application::application::{ManagedStateSummary, PackageManagedState};
    // Every refresh carries the full doctor with attachments (served by
    // the inspection cache), so the Plugins drawer's status line derives
    // real health from it instead of the masked "unknown" placeholder.
    let mut model = model_with_plugins(&["one"]);
    model.route = Route::Plugins;
    model.marketplace_drawer_open = true;
    model.doctor = Some(DoctorReport {
        uze_home: PathBuf::from("/home"),
        store: uze_application::application::StoreHealth::Ready,
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
        rows.iter().any(|row| row.to_lowercase().contains("ready")),
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

    // The sidebar always starts at column 0, so the width should track the
    // mouse's own column directly.
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 32,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        model.sidebar_width,
        Some(32),
        "dragging the handle to column 32 (the sidebar's x=0 origin) must set that width"
    );

    // A regression check for a real bug: width used to be computed as a
    // delta from the *previous* frame's border position (this hit rect's
    // stale x), not the mouse's absolute column — so once the border moved,
    // every further drag step measured from the wrong reference and the
    // sidebar edge fought the mouse instead of tracking it. Re-rendering at
    // the new width (as the real run loop does every tick) before a second,
    // independent drag catches that: the width must still land exactly on
    // the column dragged to, not drift from where the border now sits.
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 35,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(
        model.sidebar_width,
        Some(35),
        "a second drag after a re-render must still track the mouse's absolute column, not drift"
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

/// A caption pinned to the right edge is elided to what is left of the
/// row. It used to be appended whole and cut by the frame, which is how a
/// long branch name on the Git section header lost both its ending and any
/// sign that it had one.
#[test]
fn a_trailing_caption_is_elided_to_the_room_the_row_has_left() {
    use ratatui::text::Span;

    let mut spans = vec![Span::raw("▾ Git")];
    super::push_trailing(
        &mut spans,
        20,
        "agent/a-very-long-branch-name".to_owned(),
        MUTED,
    );
    let row: String = spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(
        row.ends_with("… "),
        "the caption says it was shortened: {row}"
    );
    assert!(
        Span::raw(&row).width() <= 20,
        "and the row still fits the column: {row}"
    );

    let mut spans = vec![Span::raw("▾ Git")];
    super::push_trailing(&mut spans, 20, "main".to_owned(), MUTED);
    let row: String = spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(
        row.contains("main"),
        "a caption that fits is left alone: {row}"
    );
    assert!(!row.contains('…'), "{row}");
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

use uze_application::application::{
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
        agents_directory_present: true,
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
        agents_directory_present: false,
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

#[test]
fn overview_does_not_render_project_context() {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        workspace: Some(consumer_workspace(
            ProjectEnvironmentState::InstallRequired,
            4,
            3,
            &["flow"],
            std::path::Path::new("/tmp/project"),
        )),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    for forbidden in [
        "PROJECT",
        "MARKETPLACE",
        "Environment",
        "Memory",
        "Context bridges",
        "context bridges verified",
    ] {
        assert!(
            !rows.iter().any(|row| row.contains(forbidden)),
            "Overview must not render project context: {forbidden}"
        );
    }
}

#[test]
fn overview_render_does_not_mutate_project_state() {
    use ratatui::{Terminal, backend::TestBackend};

    let base = uze_testkit::temp::scratch("ui-overview-immutable");
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
    // The machine dashboard still renders while leaving the project untouched.
    assert!(rows.iter().any(|row| row.contains("Overview")));
    assert!(rows.iter().any(|row| row.contains("Harnesses detected")));
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn no_workspace_render_creates_nothing() {
    use ratatui::{Terminal, backend::TestBackend};

    let base = uze_testkit::temp::scratch("ui-noworkspace");
    let root = base.join("random");
    std::fs::create_dir_all(&root).unwrap();

    let model = TuiModel {
        route: Route::Overview,
        context_root: root.clone(),
        workspace: Some(OverviewWorkspaceSummary {
            cwd: root.clone(),
            root: root.clone(),
            kind: WorkspaceKind::NoWorkspace,
            agents_directory_present: false,
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
    assert!(rows.iter().any(|row| row.contains("Overview")));
    assert!(!rows.iter().any(|row| row.contains("PROJECT")));
    assert!(!rows.iter().any(|row| row.contains("MARKETPLACE")));

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

    let base = uze_testkit::temp::scratch("ui-install-dispatch");
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
        worktrees: None,
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

// --- Prompt history -----------------------------------------------------

fn prompt(tab_id: u64, preview: &str) -> uze_core::prompt_history::PromptEntry {
    uze_core::prompt_history::PromptEntry {
        space_label: "space 1".to_owned(),
        tab_id,
        tab_label: format!("tab {tab_id}"),
        agent_binary: "agent".to_owned(),
        preview: preview.to_owned(),
        timestamp_secs: 0,
    }
}

fn overview_with_prompts(count: u64) -> TuiModel {
    TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        prompt_history: (0..count)
            .map(|index| prompt(index + 1, &format!("prompt {index}")))
            .collect(),
        ..TuiModel::default()
    }
}

#[test]
fn overview_arrows_move_the_prompt_selection_within_bounds() {
    let mut model = overview_with_prompts(3);

    for _ in 0..5 {
        model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(model.overview_prompt_selected, 2);

    for _ in 0..5 {
        model.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    }
    assert_eq!(model.overview_prompt_selected, 0);
}

#[test]
fn overview_arrows_still_navigate_routes_from_the_sidebar() {
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Sidebar,
        prompt_history: vec![prompt(1, "prompt")],
        ..TuiModel::default()
    };

    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(model.route, Route::Plugins);
    assert_eq!(model.overview_prompt_selected, 0);
}

#[test]
fn activating_a_prompt_returns_to_its_tab() {
    let mut model = overview_with_prompts(3);
    model.overview_prompt_selected = 2;

    let intent = model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(intent, Intent::SwitchToWorkspaceTab(3));
}

#[test]
fn an_empty_history_leaves_enter_to_the_routes_own_action() {
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        ..TuiModel::default()
    };

    assert_ne!(
        model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Intent::SwitchToWorkspaceTab(0)
    );
}

#[test]
fn clearing_the_history_is_confirmed_before_it_happens() {
    let mut model = overview_with_prompts(2);

    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::None);
    assert_eq!(model.overlay, Overlay::ConfirmClearPromptHistory);

    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(intent, Intent::ClearPromptHistory);
    assert_eq!(model.overlay, Overlay::None);
}

#[test]
fn declining_the_clear_confirmation_does_nothing() {
    let mut model = overview_with_prompts(2);
    model.apply_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    let intent = model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(intent, Intent::None);
    assert_eq!(model.overlay, Overlay::None);
    assert_eq!(model.prompt_history.len(), 2);
}

#[test]
fn a_prompt_row_is_clickable_and_hoverable_at_the_same_rect() {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut model = overview_with_prompts(3);
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    let (rect, _) = model
        .hits
        .iter()
        .find(|(_, hit)| matches!(hit, Hit::PromptHistory(1)))
        .expect("the second prompt row registers a hit");
    let (column, row) = (rect.x + 1, rect.y);

    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );
    assert_eq!(model.overview_prompt_hovered, Some(1));

    assert_eq!(model.click(column, row), Intent::SwitchToWorkspaceTab(2));
    assert_eq!(model.overview_prompt_selected, 1);
}

#[test]
fn moving_off_every_row_drops_the_hover() {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut model = overview_with_prompts(2);
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    model.overview_prompt_hovered = Some(0);

    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 99,
            row: 39,
            modifiers: KeyModifiers::NONE,
        },
        100,
    );

    assert_eq!(model.overview_prompt_hovered, None);
}

#[test]
fn a_refresh_that_shrinks_the_history_clamps_selection_and_hover() {
    let mut model = overview_with_prompts(5);
    model.overview_prompt_selected = 4;
    model.overview_prompt_hovered = Some(4);

    model.refreshed(RefreshData {
        prompt_history: vec![prompt(1, "only one")],
        ..RefreshData::default()
    });

    assert_eq!(model.overview_prompt_selected, 0);
    assert_eq!(model.overview_prompt_hovered, None);
}

#[test]
fn the_prompt_table_groups_rows_by_age_and_marks_the_selection() {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let recent = |tab_id: u64, agent: &str, preview: &str| uze_core::prompt_history::PromptEntry {
        agent_binary: agent.to_owned(),
        timestamp_secs: now - 8 * 60,
        ..prompt(tab_id, preview)
    };
    let model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        overview_prompt_selected: 1,
        prompt_history: vec![
            recent(1, "claude", "first prompt"),
            recent(2, "codex", "second prompt"),
            prompt(3, "from long ago"),
        ],
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);

    let title = rows
        .iter()
        .find(|row| row.contains("Recent prompts — 3 recorded"))
        .expect("the title counts the entries");
    assert!(title.ends_with("claude 1 · codex 1 · agent 1"), "{title}");
    assert!(
        rows.iter()
            .any(|row| row.contains("HARNESS") && row.contains("WHEN") && row.contains("PROMPT")),
        "column headings are drawn"
    );
    let selected = rows
        .iter()
        .find(|row| row.contains("second prompt"))
        .expect("the selected entry is drawn");
    let content = selected.rsplit('│').next().unwrap().trim_start();
    assert!(content.starts_with("❯ codex"), "{selected}");
    assert!(selected.contains("8m"), "{selected}");
    assert!(selected.contains("space 1/tab 2"), "{selected}");
    let first = rows
        .iter()
        .find(|row| row.contains("first prompt"))
        .unwrap();
    assert!(!first.contains('❯'), "{first}");

    let older_heading = rows
        .iter()
        .position(|row| row.contains("── OLDER"))
        .expect("entries from before yesterday sit under their own heading");
    let older_entry = rows
        .iter()
        .position(|row| row.contains("from long ago"))
        .unwrap();
    assert_eq!(older_entry, older_heading + 1);
    let content_of = |row: &String| row.rsplit('│').next().unwrap().trim().to_owned();
    assert!(
        content_of(&rows[older_heading - 1]).is_empty(),
        "a blank separates groups"
    );
    assert!(
        !rows[older_heading].ends_with('…'),
        "the heading's rule stops at the edge instead of being clipped"
    );
    assert!(
        !rows.iter().any(|row| row.contains("── EARLIER TODAY")),
        "recent entries open the listing without a heading"
    );
}

#[test]
fn a_selection_below_the_fold_scrolls_the_prompt_table() {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let mut model = overview_with_prompts(40);
    model.overview_prompt_selected = 30;
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();

    assert!(
        hits.iter()
            .any(|(_, hit)| matches!(hit, Hit::PromptHistory(30))),
        "the selected row is drawn even though it is not among the newest"
    );
    assert!(
        !hits
            .iter()
            .any(|(_, hit)| matches!(hit, Hit::PromptHistory(0))),
        "rows above scroll away to make room"
    );
}

#[test]
fn an_overview_with_no_room_for_the_history_still_renders() {
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    let model = overview_with_prompts(40);
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
}
