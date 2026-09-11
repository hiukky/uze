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
    Focus, Overlay, PREFERENCE_ROW_COUNT, ProfilePanel, ROUTES, RefreshData, Route, Status,
    TrustedRetry, TuiModel,
};
use super::view::health::{Severity, actionable_alerts};
use super::worker::{Intent, TrustGrant};
use crate::ui::theme::{self, Token};

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
        Overlay::ActionIndex {
            scopes: vec![uze_keys::Scope::Global, uze_keys::Scope::Management],
            filter: String::new(),
            selected: 0,
        },
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
    // Against the sidebar's own order rather than against named routes:
    // what this proves is that the keys walk it and turn around, which is
    // still true the next time the order is argued over.
    assert_eq!(model.route, ROUTES[0]);
    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(model.route, ROUTES[1]);
    model.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(model.route, ROUTES[2]);
    model.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(model.route, ROUTES[1]);
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
fn a_return_visit_draws_what_the_last_one_resolved() {
    let mut model = model_with_plugins(&["one", "two"]);
    model.resolved_at = Some(std::time::Instant::now());
    model.marketplace_selected = 1;
    model.marketplace_drawer_open = false;
    model.prompt_history = Vec::new();
    // What one visit ends holding — including work it was in the middle
    // of, which the next visit must not inherit.
    model.status = Status::Working("Inspecting one…".to_owned());
    model.overlay = Overlay::ConfirmRemove {
        id: "one".to_owned(),
        focus: 0,
    };
    model.focus = Focus::Overlay;
    model.maintenance_in_flight = true;
    model.inspection_in_flight = Some(Intent::InspectPlugin("one".to_owned()));
    model.hits = vec![(Rect::new(0, 0, 1, 1), Hit::Route(Route::Plugins))];

    let layout = model.management_layout();
    let model = TuiModel::recall(Some(model.remember()), &layout);

    assert_eq!(
        model.plugins.len(),
        2,
        "the machine state the last visit resolved is still the truth about the machine"
    );
    assert!(
        model.resolved_at.is_some(),
        "and so is when it was resolved — the next visit decides on it"
    );
    assert_eq!(model.route, Route::Plugins);
    assert_eq!(model.marketplace_selected, 1);
    assert!(
        !model.marketplace_drawer_open,
        "a drawer stays as it was left"
    );
    assert!(matches!(model.status, Status::Idle));
    assert!(matches!(model.overlay, Overlay::None));
    assert_eq!(model.focus, Focus::Sidebar);
    assert!(
        !model.maintenance_in_flight && model.inspection_in_flight.is_none(),
        "work in flight belonged to a visit that ended, and its channel with it"
    );
    assert!(model.hits.is_empty());
}

#[test]
fn a_resolution_the_session_just_made_is_not_asked_for_again() {
    use super::management::{RESOLUTION_STANDS_FOR, opening_re_resolves};
    use std::time::Instant;

    assert!(
        opening_re_resolves(None),
        "nothing resolved yet is not an answer to stand on"
    );
    assert!(
        !opening_re_resolves(Some(Instant::now())),
        "the session's own warm-up answered a moment ago; opening the screen shows it"
    );
    assert!(
        opening_re_resolves(Some(Instant::now() - RESOLUTION_STANDS_FOR)),
        "past the window, opening the screen is a claim about the machine now"
    );
}

#[test]
fn a_first_visit_starts_from_the_default_model() {
    let model = TuiModel::recall(None, &uze_application::ManagementLayout::default());
    assert!(model.plugins.is_empty());
    assert_eq!(model.route, Route::Overview);
    assert!(
        model.marketplace_drawer_open && model.extension_drawer_open && model.harnesses_drawer_open,
        "the drawers a screen opens with are stated once, by Default"
    );
}

/// Which screen was open, and how its drawers were left, outlive the
/// process: the next run opens where the last one was, not on Overview.
#[test]
fn the_next_run_opens_on_the_screen_the_last_one_left() {
    let mut model = TuiModel::default();
    model.set_route(Route::Profiles);
    model.harnesses_drawer_open = false;
    model.profile_columns_width = Some(28);
    model
        .collapsed_marketplaces
        .insert("uze-official".to_owned());

    let layout = model.management_layout();
    assert_eq!(layout.route.as_deref(), Some("profiles"));

    let model = TuiModel::recall(None, &layout);
    assert_eq!(model.route, Route::Profiles);
    assert!(
        !model.harnesses_drawer_open,
        "a drawer stays as it was left"
    );
    assert_eq!(model.profile_columns_width, Some(28));
    assert!(model.collapsed_marketplaces.contains("uze-official"));

    let unknown = uze_application::ManagementLayout {
        route: Some("a screen this build does not have".to_owned()),
        ..uze_application::ManagementLayout::default()
    };
    assert_eq!(
        TuiModel::recall(None, &unknown).route,
        Route::Overview,
        "a screen the client no longer recognizes opens the default, not nothing"
    );
}

/// The management client reopens on the screen it was left on without
/// passing through a route change — and Appearance, which reads its lists
/// on arrival, used to open empty until it was clicked again.
#[test]
fn appearance_reopened_where_it_was_left_still_reads_its_lists() {
    let layout = uze_application::ManagementLayout {
        route: Some(Route::Appearance.id().to_owned()),
        ..uze_application::ManagementLayout::default()
    };
    let mut model = TuiModel::recall(None, &layout);
    assert_eq!(model.route, Route::Appearance);
    assert_eq!(model.appearance_intent(), Intent::LoadAppearance);
    model.appearance_read = true;
    assert_eq!(
        model.appearance_intent(),
        Intent::None,
        "read once, not every frame — an empty machine included"
    );
    model.set_route(Route::Overview);
    assert_eq!(
        model.appearance_intent(),
        Intent::None,
        "only that screen reads them"
    );
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

/// One key opens the index, and it is the same key in both modes. It used
/// to be F1 in the workspace and `?` here — and since a surface prints the
/// innermost chord it can find, the key that worked in both was the one
/// never shown.
#[test]
fn the_index_opens_and_closes_on_the_one_key_both_modes_share() {
    let mut model = TuiModel::default();
    model.apply_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert!(
        matches!(model.overlay, Overlay::ActionIndex { .. }),
        "{:?}",
        model.overlay
    );
    model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(model.overlay, Overlay::None);

    // And it is what management advertises, rather than a second key of
    // its own that the workspace would not answer.
    assert_eq!(
        uze_keys::active().chord_for(
            uze_keys::Action::OpenActionIndex,
            &[uze_keys::Scope::Global, uze_keys::Scope::Management],
        ),
        uze_keys::Chord::parse("f1").ok()
    );
}

/// Nothing the index prints is written down: the words come from the
/// action and the key from the keymap. The list it replaced was typed by
/// hand and had already fallen out of step with the dispatcher for nine
/// of its bindings.
#[test]
fn the_index_prints_the_key_the_keymap_actually_binds() {
    let mut model = model_with_plugins(&["one"]);
    model.focus = Focus::Content;
    model.act(uze_keys::Action::OpenActionIndex);
    let Overlay::ActionIndex { scopes, .. } = model.overlay.clone() else {
        panic!("the index is open");
    };
    let rows = model.action_index_rows(&scopes, "");
    let keymap = uze_keys::active();
    for (action, chord) in &rows {
        assert_eq!(
            *chord,
            keymap.chord_for(*action, &scopes),
            "the index invented a key for {action}"
        );
    }
    assert!(
        rows.iter()
            .any(|(action, _)| *action == uze_keys::Action::RemovePlugin),
        "what this screen can do is on offer: {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|(action, chord)| *action == uze_keys::Action::SwitchMode && chord.is_some()),
        "and so is what is reachable from everywhere"
    );
}

/// Typing narrows, and choosing a row performs it — so someone who does
/// not know the keyboard uses the index as a menu, and reads the key off
/// the row they just used.
#[test]
fn the_index_narrows_as_you_type_and_performs_what_you_choose() {
    let mut model = model_with_plugins(&["one"]);
    model.focus = Focus::Content;
    model.act(uze_keys::Action::OpenActionIndex);
    for character in "remove".chars() {
        model.apply_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let Overlay::ActionIndex { scopes, filter, .. } = model.overlay.clone() else {
        panic!("still open");
    };
    assert_eq!(filter, "remove");
    let rows = model.action_index_rows(&scopes, &filter);
    assert!(
        rows.iter().all(
            |(action, _)| action.label().to_lowercase().contains("remove")
                || action.description().to_lowercase().contains("remove")
        ),
        "{rows:?}"
    );
    model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(model.overlay, Overlay::ConfirmRemove { .. }),
        "choosing from the index performs it: {:?}",
        model.overlay
    );
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
fn the_drawers_delete_button_opens_the_delete_confirmation() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    let id = model.profiles[0].id.clone();
    model.hits = vec![(
        Rect::new(10, 4, 8, 1),
        Hit::OfferedAction(uze_keys::Action::DeleteProfile),
    )];

    assert_eq!(model.click(12, 4), Intent::None);
    assert!(matches!(
        &model.overlay,
        Overlay::ConfirmDeleteProfile { id: confirmed_id, .. } if *confirmed_id == id
    ));
    assert_eq!(model.focus, Focus::Overlay);
}

#[test]
fn the_drawers_apply_button_targets_checked_harnesses() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.profiles[0].active = false;
    let id = model.profiles[0].id.clone();
    model.hits = vec![(
        Rect::new(18, 4, 9, 1),
        Hit::OfferedAction(uze_keys::Action::ApplyProfile),
    )];

    let Intent::ApplyProfile {
        id: applied_id,
        preferences,
        harness_ids,
    } = model.click(19, 4)
    else {
        panic!("expected ApplyProfile");
    };
    assert_eq!(applied_id, id);
    assert_eq!(preferences, model.profiles[0].preferences);
    assert_eq!(
        harness_ids,
        vec!["claude-code".to_owned(), "codex".to_owned()]
    );
}

/// Active is not in effect: a preference edited after the last apply has
/// to be writable without first making some other profile active.
#[test]
fn the_active_profile_can_be_applied_again() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    assert!(model.profiles[0].active);
    model.profile_panel = ProfilePanel::Editor;
    model.profile_editor_selected = 2;
    model.cycle_selected_preference(true);
    let edited = model.profiles[0].preferences;

    let Intent::ApplyProfile { preferences, .. } = model.act(uze_keys::Action::ApplyProfile) else {
        panic!("expected ApplyProfile");
    };
    assert_eq!(
        preferences, edited,
        "the apply carries what is on screen, not what reached disk"
    );
}

#[test]
fn applying_with_no_harness_checked_says_why_and_writes_nothing() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.profile_harness_selection.clear();
    assert_eq!(model.act(uze_keys::Action::ApplyProfile), Intent::None);
    assert!(
        matches!(&model.status, Status::Success(message) if message.contains("harness")),
        "{:?}",
        model.status
    );
}

fn preview_fixture(model: &TuiModel) -> uze_application::application::ProfilePreview {
    use uze_application::application::HarnessPreview;
    use uze_application::{
        AxisPlan, CompatibilityRoute, KeyPlan, PlannedValue, PreferenceAxis, PreferencePlan,
    };
    let claude = PreferencePlan {
        config_path: PathBuf::from("/home/someone/.claude/settings.json"),
        axes: vec![
            AxisPlan {
                axis: PreferenceAxis::Autonomy,
                route: CompatibilityRoute::Native,
                summary: "permissions.defaultMode = auto".to_owned(),
                note: None,
                keys: vec![KeyPlan {
                    key: "permissions.defaultMode".to_owned(),
                    current: Some("\"auto\"".to_owned()),
                    planned: PlannedValue::Set("\"auto\"".to_owned()),
                }],
            },
            AxisPlan {
                axis: PreferenceAxis::Sandbox,
                route: CompatibilityRoute::Degraded,
                summary: "sandbox.enabled = true".to_owned(),
                note: Some("Claude cannot start its sandbox here without socat".to_owned()),
                keys: vec![KeyPlan {
                    key: "sandbox.enabled".to_owned(),
                    current: Some("true".to_owned()),
                    planned: PlannedValue::Set("true".to_owned()),
                }],
            },
            AxisPlan {
                axis: PreferenceAxis::Model,
                route: CompatibilityRoute::Native,
                summary: "model unset".to_owned(),
                note: None,
                keys: vec![KeyPlan {
                    key: "model".to_owned(),
                    current: Some("\"default\"".to_owned()),
                    planned: PlannedValue::Removed,
                }],
            },
        ],
    };
    uze_application::application::ProfilePreview {
        preferences: model.profiles[0].preferences,
        harnesses: vec![
            HarnessPreview {
                integration: "claude-code".to_owned(),
                plan: Ok(claude),
            },
            HarnessPreview {
                integration: "codex".to_owned(),
                plan: Err("`config.toml` is not valid TOML".to_owned()),
            },
        ],
    }
}

#[test]
fn the_profiles_screen_asks_for_its_preview_once_and_again_after_an_edit() {
    let mut model = model_with_data();
    assert_eq!(
        model.profile_preview_intent(),
        Intent::None,
        "only the Profiles screen reads it"
    );
    model.set_route(Route::Profiles);
    let Intent::PreviewProfile(question) = model.profile_preview_intent() else {
        panic!("the preview is read without being asked for");
    };
    assert_eq!(question.preferences, model.profiles[0].preferences);
    assert_eq!(question.harness_ids, vec!["claude-code", "codex"]);

    model.profile_preview_asked = Some(question);
    assert_eq!(model.profile_preview_intent(), Intent::None, "asked once");

    model.profile_panel = ProfilePanel::Editor;
    model.cycle_selected_preference(true);
    assert!(matches!(
        model.profile_preview_intent(),
        Intent::PreviewProfile(_)
    ));
}

fn answer_preview(model: &mut TuiModel) {
    let question = model.profile_preview_question().unwrap();
    let preview = preview_fixture(model);
    model.profile_previewed(question, Ok(preview));
}

#[test]
fn a_preview_of_other_preferences_is_never_shown_as_this_ones() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    answer_preview(&mut model);
    assert!(model.current_profile_preview().is_some());
    model.profiles_selected = 1;
    assert!(
        model.current_profile_preview().is_none(),
        "safe-mode's preferences are not dev-autonomous's"
    );
}

/// A read that started before an apply finished must never pass for one
/// made after it, even when it answers the very same preferences.
#[test]
fn a_preview_read_before_the_harnesses_changed_is_refused() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    let before = model.profile_preview_question().unwrap();
    model.profile_preview_asked = Some(before.clone());
    model.invalidate_profile_preview();
    model.profile_previewed(before, Ok(preview_fixture(&model)));
    assert!(model.current_profile_preview().is_none());
    assert!(
        matches!(model.profile_preview_intent(), Intent::PreviewProfile(_)),
        "and the screen asks again"
    );
}

#[test]
fn a_preview_that_could_not_be_read_says_so_and_is_not_retried_in_a_loop() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    let question = model.profile_preview_question().unwrap();
    model.profile_preview_asked = Some(question.clone());
    model.profile_previewed(question, Err("the store is locked".to_owned()));
    assert_eq!(model.profile_preview_intent(), Intent::None);
    let lines: Vec<String> = crate::ui::view::profiles::preview_lines(&model, 80)
        .lines
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("the store is locked")),
        "{lines:?}"
    );
    model.invalidate_profile_preview();
    assert!(
        matches!(model.profile_preview_intent(), Intent::PreviewProfile(_)),
        "a refresh is what asks again"
    );
}

#[test]
fn v_opens_the_preview_and_esc_closes_it_before_anything_else() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::Editor;
    model.apply_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
    assert!(model.profile_preview_open);
    model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!model.profile_preview_open);
    assert_eq!(
        model.profile_panel,
        ProfilePanel::Editor,
        "Esc closed one thing"
    );
}

/// A preference is one of several values; its row says so with steppers,
/// and the pointer changes it with them.
#[test]
fn a_preference_steps_through_its_values_from_its_arrows() {
    use ratatui::{Terminal, backend::TestBackend};
    use uze_core::preference::Autonomy;

    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    let next = crate::ui::theme::glyph(crate::ui::theme::Symbol::StepNext);
    let stepper_columns: Vec<usize> = ["autonomy", "sandbox", "model"]
        .iter()
        .map(|axis| {
            let row = rows
                .iter()
                .find(|row| row.contains(&format!("{axis}   ")))
                .unwrap_or_else(|| panic!("no {axis} row: {rows:#?}"));
            row.chars()
                .collect::<Vec<_>>()
                .iter()
                .rposition(|cell| next.starts_with(*cell))
                .unwrap_or_else(|| panic!("{axis} offers no next value: {row}"))
        })
        .collect();
    assert!(
        stepper_columns.windows(2).all(|pair| pair[0] == pair[1]),
        "the steppers stand in one column: {stepper_columns:?}"
    );

    model.hits = hits;
    let target = |forward: bool| {
        model
            .hits
            .iter()
            .find(|(_, hit)| *hit == Hit::StepPreference { index: 0, forward })
            .map(|(rect, _)| (rect.x, rect.y))
            .expect("each arrow is a target of its own")
    };
    let (next_x, next_y) = target(true);
    let (previous_x, previous_y) = target(false);
    assert_eq!(model.profiles[0].preferences.autonomy, Autonomy::Auto);
    let Intent::UpdatePreferences { preferences, .. } = model.click(next_x, next_y) else {
        panic!("the next arrow changes the value");
    };
    assert_eq!(preferences.autonomy, Autonomy::Unattended);
    assert_eq!(model.profile_panel, ProfilePanel::Editor);
    let Intent::UpdatePreferences { preferences, .. } = model.click(previous_x, previous_y) else {
        panic!("the previous arrow changes it back");
    };
    assert_eq!(preferences.autonomy, Autonomy::Auto);
}

/// A profile's actions are its drawer's buttons, as on every screen; the
/// row itself only names the profile.
#[test]
fn a_profile_row_carries_no_action_of_its_own() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    let row = rows
        .iter()
        .find(|row| row.contains("dev-autonomous"))
        .expect("the selected profile is drawn");
    let tree = row.split("Harnesses").next().unwrap_or(row);
    assert!(
        !tree.contains("apply") && !tree.contains("remove"),
        "{tree}"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("  Apply  ") && row.contains("Delete")),
        "the drawer offers both as buttons: {rows:#?}"
    );
    let offered: Vec<_> = hits
        .iter()
        .filter_map(|(_, hit)| match hit {
            Hit::OfferedAction(action) => Some(*action),
            _ => None,
        })
        .collect();
    assert!(offered.contains(&uze_keys::Action::ApplyProfile));
    assert!(offered.contains(&uze_keys::Action::DeleteProfile));
}

#[test]
fn the_preview_shows_each_key_as_it_is_and_as_it_will_be() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    answer_preview(&mut model);
    model.profile_preview_open = true;
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    let row = |needle: &str| {
        rows.iter()
            .find(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no row with {needle}: {rows:#?}"))
    };
    let position = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no row with {needle}: {rows:#?}"))
    };
    let heading = row("autonomy   sandbox");
    assert!(
        heading.contains("model"),
        "the table names its axes: {heading}"
    );
    let claude_row = rows
        .iter()
        .position(|row| row.contains("Claude Code") && row.contains("as asked"))
        .unwrap_or_else(|| panic!("Claude's row in the table: {rows:#?}"));
    for cell in ["partial", "1 change"] {
        assert!(
            rows[claude_row].contains(cell),
            "{cell}: {}",
            rows[claude_row]
        );
    }
    let open = crate::ui::theme::glyph(crate::ui::theme::Symbol::ChevronExpanded);
    assert!(
        row(&format!("{open} Codex ")).contains("cannot apply"),
        "a harness whose file cannot be read says so on its row"
    );
    assert!(
        position(".claude/settings.json") > claude_row,
        "a harness with something to write opens by itself"
    );
    let model_row = row("\"default\"");
    assert!(
        model_row.contains("remove  ") && model_row.contains("unset"),
        "a setting says what applying does to it: {model_row}"
    );
    assert!(
        position("without socat") < position("remove  "),
        "what keeps an axis from being honoured is read before the settings"
    );
    assert!(
        position("remove  ") < position("permissions.defaultMode"),
        "what changes is read before what already holds"
    );
    row("not valid TOML");
    row("partial: not fully honoured");
    assert!(
        row("would change").contains("1 setting in 1 harness"),
        "the preview leads with its answer"
    );
    let header = row("+ new");
    assert!(
        header.contains("  Preview  "),
        "previewing is a button beside new, not a drawer action: {header}"
    );
    let buttons = row("  Apply  ");
    assert!(buttons.contains("Delete") && !buttons.contains("Preview"));
    let preview_button = hits
        .iter()
        .find(|(_, hit)| *hit == Hit::OfferedAction(uze_keys::Action::PreviewProfile))
        .map(|(rect, _)| *rect)
        .expect("the Preview button is a target");
    model.hits = hits;
    model.click(preview_button.x + 2, preview_button.y);
    assert!(
        !model.profile_preview_open,
        "and clicking it again closes the preview"
    );
    assert!(
        rows.iter().any(|row| row.contains("Active, not in effect")),
        "the drawer no longer claims the profile is in use: {rows:#?}"
    );
}

/// Nothing to write is one quiet row; opening it shows the file anyway,
/// and a click on the row does what Enter does.
#[test]
fn a_harness_in_the_preview_opens_and_closes() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    answer_preview(&mut model);
    model.profile_preview_open = true;
    let claude_open = |model: &TuiModel| {
        crate::ui::view::profiles::preview_lines(model, 120)
            .lines
            .iter()
            .any(|line| line.to_string().contains(".claude/settings.json"))
    };
    assert!(claude_open(&model), "open by itself: it has a change");
    model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        !claude_open(&model),
        "Enter closes the harness under the cursor"
    );
    model.hits = vec![(Rect::new(0, 5, 80, 1), Hit::PreviewHarness(0))];
    model.click(3, 5);
    assert!(claude_open(&model), "and a click on its row opens it again");
    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(model.profile_preview_cursor, 1);
    model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        model.profile_preview_cursor, 1,
        "the cursor stops at the last harness"
    );
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
fn applying_a_profile_is_offered_without_a_key() {
    let mut model = model_with_data();
    model.set_route(Route::Profiles);
    model.focus = Focus::Content;
    model.profile_panel = ProfilePanel::List;
    model.profiles_selected = 1;
    let id = model.profiles[1].id.clone();
    // `s` sets up a harness and nothing else; applying a profile is
    // offered by its row's own actions and its drawer's button — and it
    // writes, rather than only marking the profile active.
    let intent = model.act(uze_keys::Action::ApplyProfile);
    assert!(matches!(intent, Intent::ApplyProfile { id: applied, .. } if applied == id));
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
    let intent = model.apply_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
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

/// `r` used to remove a plugin on one screen and refresh the machine on
/// every other one — the collision that made the help overlay need an
/// aside column to explain itself. A letter now names one action.
#[test]
fn a_letter_names_one_action_and_refreshing_has_its_own() {
    let mut model = TuiModel {
        focus: Focus::Content,
        route: Route::Overview,
        ..TuiModel::default()
    };
    assert_eq!(
        model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        Intent::Refresh,
        "refreshing carries a modifier: it is not something done to a row"
    );
    assert_eq!(
        model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        Intent::None,
        "`r` removes, and there is nothing here to remove"
    );

    let mut plugins_model = model_with_plugins(&["one"]);
    let intent = plugins_model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(plugins_model.overlay, Overlay::ConfirmRemove { ref id, .. } if id == "one"));
    assert_eq!(intent, Intent::None);
    assert_eq!(
        model_with_plugins(&["one"])
            .apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        Intent::Refresh,
        "and refreshing means the same thing on every screen"
    );
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

/// The foot of the sidebar carries a list of things worth trying once, in
/// the same collapsible shape as the workspace's commit timeline: a header
/// that folds it and says how far along you are, and a row per step with
/// the key that reaches it and a mark once you have taken it.
#[test]
fn the_sidebars_foot_lists_the_first_steps_and_ticks_the_taken_ones() {
    let mut model = model_with_plugins(&["flow"]);
    let taken = crate::ui::management::FIRST_STEPS[0];
    model.steps_taken = [taken.name()].into_iter().collect();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let drawn = buffer_rows(&terminal);

    let header = drawn
        .iter()
        .find(|row| row.contains("first steps"))
        .expect("the section names itself");
    assert!(
        header.contains(&format!(
            "1 of {}",
            crate::ui::management::FIRST_STEPS.len()
        )),
        "and how far along: {header:?}"
    );

    let tick = theme::glyph(theme::Symbol::MarkDone);
    for action in crate::ui::management::FIRST_STEPS {
        let (rect, _) = hits
            .iter()
            .find(|(_, hit)| *hit == Hit::OfferedAction(action))
            .unwrap_or_else(|| panic!("{action} is a step you can click"));
        let row = &drawn[usize::from(rect.y)];
        assert!(row.contains(&action.label()), "{row:?}");
        assert_eq!(
            row.contains(&tick),
            action == taken,
            "only what has been done is ticked: {row:?}"
        );
    }

    // Folding it leaves the header, and the header alone.
    let (header, _) = hits
        .iter()
        .find(|(_, hit)| *hit == Hit::ToggleFirstSteps)
        .expect("the header folds it");
    model.hits = hits.clone();
    model.click(header.x, header.y);
    assert!(model.first_steps_collapsed);
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    assert!(
        hits.iter().any(|(_, hit)| *hit == Hit::ToggleFirstSteps),
        "the header stays"
    );
    assert!(
        !hits
            .iter()
            .any(|(_, hit)| *hit == Hit::OfferedAction(uze_keys::Action::OpenActionIndex)),
        "and its steps are folded away"
    );
}

/// The same in this mode: the mark appears only once the list is finished,
/// and it puts the section away for good rather than folding it.
#[test]
fn a_finished_list_offers_to_leave() {
    let mut model = model_with_plugins(&["flow"]);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    assert!(
        !hits.iter().any(|(_, hit)| *hit == Hit::CloseFirstSteps),
        "unfinished, so nothing to close"
    );

    model.steps_taken = crate::ui::management::FIRST_STEPS
        .iter()
        .map(|action| action.name())
        .collect();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let (close, _) = hits
        .iter()
        .find(|(_, hit)| *hit == Hit::CloseFirstSteps)
        .expect("finished, so the header offers the way out");
    model.hits = hits.clone();
    model.click(close.x, close.y);
    assert!(model.first_steps_closed);

    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    assert!(
        !hits.iter().any(|(_, hit)| *hit == Hit::ToggleFirstSteps),
        "closed for good, header and all"
    );
}

/// A step is recorded wherever it was performed from — the key, a button,
/// the index — because every action this client performs goes through one
/// place, and that is where the list learns.
#[test]
fn taking_a_step_any_way_at_all_marks_it_taken() {
    let mut model = model_with_plugins(&["flow"]);
    assert!(model.steps_taken.is_empty());
    model.act(uze_keys::Action::OpenActionIndex);
    assert!(
        model
            .steps_taken
            .contains(&uze_keys::Action::OpenActionIndex.name())
    );

    // And an action that is not a step leaves the list alone.
    let before = model.steps_taken.clone();
    model.act(uze_keys::Action::StartFilter);
    assert_eq!(model.steps_taken, before);
}

/// The property the list needs and nothing was checking: a step must be
/// takeable from wherever the list is drawn, which is every screen.
///
/// Two steps were screen-specific — asking a row what can be done to it,
/// and searching a list — so on the screen uze opens on they were rows
/// that did nothing when clicked, in a checklist that could never be
/// finished from there.
#[test]
fn every_first_step_can_be_taken_from_every_screen() {
    for route in ROUTES {
        for action in crate::ui::management::FIRST_STEPS {
            let mut model = TuiModel {
                route,
                focus: Focus::Content,
                ..model_with_data()
            };
            model.act(action);
            assert!(
                model.steps_taken.contains(&action.name()),
                "{action} did not land on {route:?} — a step the list offers \
                 everywhere has to be takeable everywhere"
            );
        }
    }
}

/// An action that belongs to a screen with a list still answers on one
/// without, rather than doing nothing — which is what a broken key looks
/// like — and is never recorded as something that happened.
#[test]
fn a_key_with_nothing_to_act_on_here_says_so() {
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        ..model_with_data()
    };

    model.act(uze_keys::Action::StartFilter);
    assert!(!model.filtering, "the Overview has nothing to search");
    assert!(
        matches!(&model.status, Status::Success(said) if said.contains("search")),
        "and the key says so: {:?}",
        model.status
    );
    assert!(
        model.steps_taken.is_empty(),
        "it is not a first step, and nothing was recorded either way"
    );

    // On a screen that has them, both land.
    let mut model = TuiModel {
        route: Route::Plugins,
        focus: Focus::Content,
        ..model_with_data()
    };
    model.act(uze_keys::Action::StartFilter);
    assert!(model.filtering);
}

/// The Keys screen draws a search field and answers clicks on it, but `/`
/// did not reach it: the one screen whose whole subject is keys had a key
/// that did nothing.
#[test]
fn the_keys_screen_is_searchable_by_its_own_key() {
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    model.act(uze_keys::Action::StartFilter);
    assert!(model.filtering);
    for character in "quit".chars() {
        model.type_character(character);
    }
    assert_eq!(model.keys_filter, "quit");
    assert!(
        !model.key_rows().is_empty(),
        "and the list narrowed to something"
    );
    assert!(
        model.key_rows().len() < TuiModel::default().key_rows().len(),
        "narrower than the whole keyboard"
    );
}

/// A hint names a key and what it does, and it asks the keymap for both.
/// The strings this replaced were typed by hand — which is how the help
/// overlay came to omit nine of the keys it was supposed to document.
#[test]
fn a_hint_line_reads_its_keys_off_the_keymap() {
    use ratatui::text::Line;
    use uze_keys::{Action, Scope};

    let scopes = [Scope::Global, Scope::Management, Scope::Plugins];
    let line: Line<'static> = crate::ui::hint_for(
        &scopes,
        &[
            Action::RemovePlugin,
            Action::Refresh,
            Action::OpenActionIndex,
        ],
    );
    let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let keymap = uze_keys::active();
    for action in [Action::RemovePlugin, Action::Refresh] {
        let chord = keymap.chord_for(action, &scopes).expect("bound here");
        assert!(
            content.contains(&chord.to_string()),
            "the hint names {action} without its key: {content}"
        );
        assert!(
            content.contains(&action.label().to_lowercase()),
            "{content}"
        );
    }
    assert_eq!(line.spans[0].style, theme::fg_bold(Token::Accent));
    assert_eq!(line.spans[1].style, theme::fg(Token::TextMuted));

    // An action with no key here is skipped rather than printed keyless:
    // a hint is a list of shortcuts, and what has none is offered where a
    // pointer can reach it.
    let unbound: Line<'static> = crate::ui::hint_for(&scopes, &[Action::NewSpace]);
    assert!(unbound.spans.is_empty());
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
        theme::color(Token::TextMuted),
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
    super::push_trailing(
        &mut spans,
        20,
        "main".to_owned(),
        theme::color(Token::TextMuted),
    );
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
            drift: Default::default(),
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
            drift: Default::default(),
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
fn installing_the_projects_environment_carries_the_workspace_root() {
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
    // Offered by the Overview's own card and by the index, with no letter
    // spent on it: `i` installs a *plugin*, and one letter names one
    // action.
    let intent = model.act(uze_keys::Action::InstallProjectEnvironment);
    assert_eq!(intent, Intent::InstallProjectEnvironment(root));
}

#[test]
fn installing_the_projects_environment_is_inert_when_it_is_ready() {
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
    let intent = model.act(uze_keys::Action::InstallProjectEnvironment);
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

/// The buffer a model draws, kept whole — colour included, which
/// [`buffer_rows`] deliberately throws away.
fn drawn(model: &TuiModel) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, model, &mut hits))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// How far a colour sits from the backdrop everything is drawn on. The
/// scrim's whole job is to make this number smaller for the screen a modal
/// interrupts, so it is the number the test asks about.
fn distance_from_the_backdrop(color: ratatui::style::Color) -> u32 {
    let ground = uze_theme::active().color(Token::SurfaceBackground);
    let ratatui::style::Color::Rgb(red, green, blue) = color else {
        panic!("every colour this TUI draws is resolved from a token: {color:?}");
    };
    u32::from(red.abs_diff(ground.0))
        + u32::from(green.abs_diff(ground.1))
        + u32::from(blue.abs_diff(ground.2))
}

/// A modal answers for the whole screen — nothing behind it responds until
/// it is dealt with — and until the scrim existed the only thing saying so
/// was the dialog's own border, which on a full screen is one hairline.
#[test]
fn a_modal_pushes_the_screen_it_interrupts_behind_it() {
    let quiet = drawn(&model_with_plugins(&["flow"]));
    let asked = drawn(&TuiModel {
        overlay: Overlay::ConfirmRemove {
            id: "flow".to_owned(),
            focus: 0,
        },
        ..model_with_plugins(&["flow"])
    });

    // A cell in the sidebar: far from any centred dialog, and written in a
    // colour the theme answers for, so both halves of the claim are about
    // the same drawn thing rather than about whatever happened to be there.
    let (column, row) = (0..40u16)
        .flat_map(|row| (0..24u16).map(move |column| (column, row)))
        .find(|position| {
            quiet[*position].symbol().trim() != "" && theme::token_of(quiet[*position].fg).is_some()
        })
        .expect("the sidebar drew something");

    let before = distance_from_the_backdrop(quiet[(column, row)].fg);
    let after = distance_from_the_backdrop(asked[(column, row)].fg);
    assert!(
        after < before,
        "the screen behind a question recedes: {before} -> {after}"
    );
    assert!(
        theme::token_of(asked[(column, row)].fg).is_none(),
        "and it recedes to a colour between two tokens rather than to another token"
    );

    // The question itself is untouched: it is drawn over the scrim, not
    // under it, which is the whole shape of the thing.
    let border = (0..40u16)
        .flat_map(|row| (24..100u16).map(move |column| (column, row)))
        .find(|position| theme::token_of(asked[*position].fg) == Some(Token::BorderDefault))
        .expect("the dialog drew its border at full contrast");
    assert!(
        asked[border].symbol().trim() != "",
        "and drew a border glyph there, not an empty cell"
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
                drift: Default::default(),
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

/// The real `git` on the ambient PATH, for a test that isolates PATH but
/// still needs to clone a marketplace.
fn which_git() -> std::path::PathBuf {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("git must be on PATH for this test")
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
    let revision = uze_testkit::git::commit_everything_in(&market);
    let lock = uze_core::project_lock::ProjectLock {
        marketplaces: std::iter::once((
            "test".to_owned(),
            uze_core::project_lock::LockedMarketplace {
                git: market.display().to_string(),
                r#ref: None,
                subdirectory: None,
                revision,
            },
        ))
        .collect(),
        plugins: std::iter::once((
            "flow".to_owned(),
            uze_core::project_lock::LockedPlugin {
                marketplace: "test".to_owned(),
                integrity: None,
            },
        ))
        .collect(),
        ..Default::default()
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
    // Git alone, since a marketplace is a Git repository and installing
    // from one clones it. Everything else must read as absent.
    let empty_path_dir = base.join("empty-path");
    std::fs::create_dir_all(&empty_path_dir).unwrap();
    let git = which_git();
    std::os::unix::fs::symlink(&git, empty_path_dir.join("git")).unwrap();
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

/// The Overview's history is seeded before the first frame, and must find
/// what the workspace client wrote — keyed the same way, from anywhere
/// inside the workspace. Reading it out of the startup worker instead is
/// what made an opened management screen say "no history yet" while
/// plugins were being seeded and the official snapshot auto-updated.
#[test]
fn the_seeded_history_reads_what_the_workspace_client_recorded() {
    let base = uze_testkit::temp::scratch("ui-prompt-history-seed");
    let home = UzeHome::at(base.join("home"));
    let project = base.join("project");
    let nested = project.join("crates").join("inner");
    std::fs::create_dir_all(&nested).unwrap();
    // The manifest is what anchors a project: the lock is derived, and a
    // derived file cannot be what identifies one. A fixture that only
    // resolved would not be found from a subdirectory at all.
    std::fs::write(project.join("agents.yaml"), "worktrees: {}\n").unwrap();

    let app = super::tui_application(home.clone()).unwrap();
    let root = app.workspace().root(&project);
    app.workspace()
        .record_prompt(
            &root,
            &uze_application::PromptOrigin {
                space_label: "project".to_owned(),
                tab_id: 7,
                tab_label: "agent 1".to_owned(),
                agent_binary: "claude".to_owned(),
            },
            "ship the thing",
        )
        .unwrap();

    // From a subdirectory, the way a `uze` launched deep inside one asks.
    let seeded = super::worker::recent_prompts(home, &nested);

    let previews: Vec<&str> = seeded.iter().map(|entry| entry.preview.as_str()).collect();
    assert_eq!(previews, ["ship the thing"]);
    assert_eq!(seeded[0].tab_id, 7);

    std::fs::remove_dir_all(&base).ok();
}

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

/// A row eliding text stays inside the width it was given, whatever the
/// active theme's elision marker costs.
///
/// The ASCII theme spends three cells on `...` where the default spends one
/// on `…`, and the old code reserved a hard-coded `1` — so this is the
/// property that says a theme can replace a symbol without shearing every
/// row that draws it.
#[test]
fn eliding_reserves_the_active_themes_own_marker_width() {
    let marker = theme::glyph(theme::Symbol::Ellipsis);
    let marker_width = usize::from(theme::width(theme::Symbol::Ellipsis));
    for width in (marker_width + 1)..12usize {
        let elided = super::elide_tail("a subject line long enough to be cut", width);
        let cells = elided.chars().count() - marker.chars().count() + marker_width;
        assert!(
            cells <= width,
            "elided to {elided:?} ({cells} cells) for a width of {width}"
        );
        assert!(
            elided.ends_with(&marker),
            "an elided row has to say it was cut: {elided:?}"
        );
    }
}

// The sidebar draws the badge into the label column, beside a
// right-aligned count, and the workspace tab measures the alias against
// the width it has left — so a label that changed length or cell count on
// its way to the screen would take a column with it. Cell count holds
// because every small capital is East Asian width *neutral*: a terminal
// gives each one cell whether or not its font has the glyph.
#[test]
fn small_caps_preserves_a_labels_length_and_its_cells() {
    use ratatui::text::Span;
    for label in ["Beta", "claude", "codex", "antigravity", "PATH shadowed"] {
        let drawn = crate::ui::small_caps(label);
        assert_eq!(
            drawn.chars().count(),
            label.chars().count(),
            "{label:?} changed length as {drawn:?}"
        );
        assert_eq!(
            Span::raw(drawn.clone()).width(),
            Span::raw(label).width(),
            "{label:?} changed cell count as {drawn:?}"
        );
        assert_eq!(
            drawn.split(' ').count(),
            label.split(' ').count(),
            "{label:?} lost a word boundary as {drawn:?}"
        );
    }
}

// Mixed case has to arrive as one even run — a full-height initial next to
// small capitals is the thing this exists to avoid. `q` and `x`, which
// Unicode has no small capital for, come out lowercase rather than
// vanishing or standing up as the one full-height letter in the run.
#[test]
fn small_caps_levels_mixed_case_and_keeps_what_it_cannot_fold() {
    assert_eq!(crate::ui::small_caps("Beta"), "ʙᴇᴛᴀ");
    assert_eq!(crate::ui::small_caps("PATH shadowed"), "ᴘᴀᴛʜ ꜱʜᴀᴅᴏᴡᴇᴅ");
    assert_eq!(crate::ui::small_caps("Query X2"), "qᴜᴇʀʏ x2");
}

// The sidebar is where someone decides which screen to open, so a route
// that is not settled has to say so there — selected or not, and in the
// narrow layout too, which drops the subtitle and is exactly where a badge
// is easiest to lose. The count has to survive beside it: the badge is
// drawn into the label column, and pushing the count off its own would
// trade one signal for another.
#[test]
fn the_unsettled_route_is_the_only_badged_one_in_either_layout() {
    use ratatui::{Terminal, backend::TestBackend};
    let badge = crate::ui::small_caps("Beta");
    for (width, height) in [(150u16, 26u16), (80, 20)] {
        for route in [Route::Profiles, Route::Plugins] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let model = TuiModel {
                route,
                focus: Focus::Content,
                overlay: Overlay::None,
                ..model_with_data()
            };
            let mut hits = Vec::new();
            terminal
                .draw(|frame| render(frame, &model, &mut hits))
                .unwrap();
            let badged: Vec<_> = buffer_rows(&terminal)
                .into_iter()
                .filter(|row| row.contains(&badge))
                .collect();
            assert_eq!(
                badged.len(),
                1,
                "at {width}x{height} on {route:?}, {} rows carry the badge: {badged:?}",
                badged.len()
            );
            assert!(
                badged[0].contains(Route::Profiles.label()),
                "the badge landed on the wrong row: {:?}",
                badged[0]
            );
            assert!(
                badged[0].contains(&crate::ui::small_digits(2)),
                "the badge pushed the route count off its row: {:?}",
                badged[0]
            );
        }
    }
}

/// The nav badge counts an inventory, and Keys is not one.
///
/// Its list holds a row per surface an action can be reached from, so the
/// same Enter, Esc and arrows are written out once per dialog and the
/// total says something about the shape of the table rather than about
/// uze. Beside the word "Keys" that number reads as how many shortcuts
/// there are to learn, which is both wrong and the impression the screen
/// exists to remove.
#[test]
fn the_keys_route_carries_no_count() {
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..model_with_data()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let nav = buffer_rows(&terminal)
        .into_iter()
        .find(|row| row.contains(Route::Keys.label()))
        .expect("the sidebar drew the route");
    assert!(
        !nav.chars().any(|glyph| "₀₁₂₃₄₅₆₇₈₉".contains(glyph)),
        "no count beside it: {nav:?}"
    );
    assert!(
        !model.key_rows().is_empty(),
        "and the screen it opens is not empty — the badge is absent by \
         choice, not for want of anything to count"
    );
}

// --- Actions where the thing they act on is -----------------------------

/// The drawer's buttons are the plugin's offers, clickable where the
/// plugin is described — and only what can run now.
#[test]
fn the_drawer_offers_what_can_be_done_as_buttons() {
    let summary = MarketplacePluginSummary {
        marketplace: "team".to_owned(),
        name: "kit".to_owned(),
        description: None,
        keywords: Vec::new(),
        installed: true,
        update_available: Some(true),
        is_default: false,
    };
    let mut model = TuiModel {
        route: Route::Plugins,
        focus: Focus::Content,
        marketplace_drawer_open: true,
        marketplace_plugins: vec![summary],
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let buttons: Vec<_> = model
        .hits
        .iter()
        .filter_map(|(rect, hit)| match hit {
            // The first-steps section shares the hit; its entries are not
            // this plugin's.
            Hit::OfferedAction(
                action @ (uze_keys::Action::InstallPlugin
                | uze_keys::Action::UpdatePlugin
                | uze_keys::Action::RemovePlugin),
            ) => Some((*rect, *action)),
            _ => None,
        })
        .collect();
    assert_eq!(
        buttons
            .iter()
            .map(|(_, action)| *action)
            .collect::<Vec<_>>(),
        [
            uze_keys::Action::UpdatePlugin,
            uze_keys::Action::RemovePlugin
        ],
        "what builds first, what destroys last, and nothing that cannot run"
    );

    // Soft at rest, the full hue under the pointer.
    let (remove, _) = buttons[1];
    let resting = terminal.backend().buffer()[(remove.x, remove.y)].bg;
    assert_ne!(
        theme::token_of(resting),
        Some(Token::StateDanger),
        "a resting button is not the full hue"
    );
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: remove.x,
            row: remove.y,
            modifiers: KeyModifiers::NONE,
        },
        120,
    );
    assert_eq!(model.hovered_offer, Some(uze_keys::Action::RemovePlugin));
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    assert_eq!(
        theme::token_of(terminal.backend().buffer()[(remove.x, remove.y)].bg),
        Some(Token::StateDanger),
        "and the one under the pointer is"
    );

    let (update, _) = buttons[0];
    model.click(update.x, update.y);
    assert!(
        matches!(model.overlay, Overlay::ConfirmUpdate(ref id) if id.contains("kit")),
        "the button asks, the same way the menu entry does: {:?}",
        model.overlay
    );
}

/// The drawer's resources are grouped by kind, so a skill never reads as
/// a hook because the two shared one comma-joined line.
#[test]
fn the_drawer_groups_resources_by_kind_and_leaves_actions_to_the_menu() {
    use uze_application::CapabilityKind;
    use uze_application::application::{MarketplacePluginDetail, PluginCapability};

    let summary = MarketplacePluginSummary {
        marketplace: "team".to_owned(),
        name: "kit".to_owned(),
        description: None,
        keywords: Vec::new(),
        installed: false,
        update_available: None,
        is_default: false,
    };
    let capability = |name: &str, kind| PluginCapability {
        identity: name.to_owned(),
        name: name.to_owned(),
        kind,
    };
    let mut model = TuiModel {
        route: Route::Plugins,
        focus: Focus::Content,
        marketplace_drawer_open: true,
        marketplace_plugins: vec![summary.clone()],
        marketplace_detail: Some(MarketplacePluginDetail {
            summary,
            capabilities: vec![
                capability("review", CapabilityKind::AgentSkill),
                capability("guard", CapabilityKind::Hook),
                capability("plan", CapabilityKind::AgentSkill),
            ],
        }),
        ..TuiModel::default()
    };
    model.marketplace_drawer_width = Some(52);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);

    let skills = rows
        .iter()
        .position(|row| row.contains("Skills") && row.contains("review, plan"))
        .unwrap_or_else(|| panic!("skills on a row of their own: {rows:#?}"));
    assert!(
        rows[skills + 1].contains("Hooks") && rows[skills + 1].contains("guard"),
        "hooks on the next: {rows:#?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("ACTIONS")),
        "no action list in the drawer: {rows:#?}"
    );
}

/// The search field is drawn on three screens and, until this, clicking it
/// did nothing at all — it was rendered without a hit of its own.
#[test]
fn clicking_the_search_field_starts_a_search() {
    for route in [Route::Plugins, Route::Extensions, Route::Harnesses] {
        let mut model = model_with_data();
        model.set_route(route);
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let (rect, _) = model
            .hits
            .iter()
            .find(|(_, hit)| *hit == crate::ui::hit::Hit::FocusFilter)
            .unwrap_or_else(|| panic!("{route:?} draws a search field nobody can click"))
            .clone();
        model.click(rect.x + 1, rect.y);
        assert!(model.filtering, "{route:?}");
    }
}

// --- The keyboard, as a thing you can look at ---------------------------

/// The keymap in force is process-wide, so the tests that replace it take
/// turns — otherwise one test's rebinding is another's flake.
static KEYBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The screen exists to be driven by the pointer: a screen about
/// rebinding that could only be worked by the bindings it is rebinding
/// would be a joke on itself.
#[test]
fn the_keys_screen_rebinds_from_a_click_and_a_keystroke() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        keyboard: crate::ui::keys::KeyboardSupport { enhanced: false },
        ..TuiModel::default()
    };
    let row = model
        .key_rows()
        .iter()
        .position(|row| row.action == uze_keys::Action::NewShellTab)
        .expect("the workspace's new-shell key is listed");
    model.keys_selected = row;

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let (rect, _) = model
        .hits
        .iter()
        .find(|(_, hit)| *hit == crate::ui::hit::Hit::OfferedAction(uze_keys::Action::ChangeKey))
        .expect("changing a key is a target, not only a keystroke")
        .clone();
    model.click(rect.x, rect.y);
    assert!(model.keys_capture, "the screen is waiting for a key");

    let intent = model.apply_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE));
    assert_eq!(
        intent,
        Intent::PersistKeymap,
        "a rebinding is remembered past this run"
    );
    assert!(!model.keys_capture);
    assert_eq!(
        uze_keys::active().chord_for(uze_keys::Action::NewShellTab, &[uze_keys::Scope::Workspace]),
        uze_keys::Chord::parse("f4").ok()
    );
    // And the screen now says it is the operator's own choice.
    assert!(
        model
            .key_rows()
            .iter()
            .any(|row| row.action == uze_keys::Action::NewShellTab && row.custom())
    );
    uze_keys::set_active(uze_keys::default_keymap().clone());
}

/// The list is long — a row per surface an action can be reached from —
/// so the window follows the selection. It did not, and every key past the
/// first screenful was invisible and unreachable at the same time.
#[test]
fn the_keys_list_follows_the_selection_past_the_fold() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let rows = model.key_rows();
    assert!(
        rows.len() > 60,
        "the premise: this list is far taller than any terminal"
    );
    let last = rows.len() - 1;
    model.keys_selected = last;
    let wanted = rows[last].action.label();

    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let drawn = buffer_rows(&terminal);
    assert!(
        drawn.iter().any(|row| row.contains(&wanted)),
        "the last key is on screen: {wanted}"
    );
    assert!(
        hits.iter()
            .any(|(_, hit)| *hit == crate::ui::hit::Hit::KeyRow(last)),
        "and it is a target, so the mouse reaches it too"
    );
    // The heading it belongs under travels with it — a key on screen under
    // no group is a key you cannot place.
    assert!(
        drawn
            .iter()
            .any(|row| row.contains(&rows[last].scope.heading().to_uppercase())),
        "{drawn:#?}"
    );
}

/// Moving between screens used to cost a detour: `left` to put the focus
/// back on the sidebar, then the arrows, then `right` to get into the
/// screen you chose. Three gestures for one intention, and nothing on
/// screen saying which half of it had the keyboard.
///
/// The sidebar is a vertical list of screens exactly as the workspace's is
/// a vertical list of spaces, so the same chord walks it — and it lands in
/// the screen, because choosing one is wanting to be on it.
#[test]
fn ctrl_and_an_arrow_walks_the_screens_from_wherever_you_are() {
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Content,
        ..model_with_data()
    };
    let step =
        |model: &mut TuiModel, code| model.apply_key(KeyEvent::new(code, KeyModifiers::CONTROL));

    assert_eq!(step(&mut model, KeyCode::Down), Intent::None);
    assert_eq!(model.route, Route::Plugins);
    assert_eq!(
        model.focus,
        Focus::Content,
        "and the keyboard is in the screen, not on its name"
    );
    step(&mut model, KeyCode::Up);
    assert_eq!(model.route, Route::Overview);
    step(&mut model, KeyCode::Up);
    assert_eq!(
        model.route,
        *ROUTES.last().expect("there are screens"),
        "it wraps, the way the sidebar's own arrows always have"
    );

    // From the sidebar too — the point is that it does not matter where
    // the focus was.
    let mut model = TuiModel {
        route: Route::Overview,
        focus: Focus::Sidebar,
        ..model_with_data()
    };
    step(&mut model, KeyCode::Down);
    assert_eq!(model.route, Route::Plugins);
    assert_eq!(model.focus, Focus::Content);
}

/// The selected key is a filled band the width of the list, the way every
/// other list in this mode marks its selection — not a brighter word
/// inside a row that otherwise looks like all the others.
#[test]
fn the_selected_key_is_a_band_across_the_list() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    model.keys_selected = 2;
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();

    let (rect, _) = hits
        .iter()
        .find(|(_, hit)| *hit == Hit::KeyRow(2))
        .expect("the selected key was drawn");
    let buffer = terminal.backend().buffer();
    let filled = (rect.x..rect.right())
        .filter(|column| {
            theme::token_of(buffer[(*column, rect.y)].bg) == Some(Token::SurfaceSelected)
        })
        .count();
    assert_eq!(
        filled,
        usize::from(rect.width),
        "every column of the row, not only the words on it"
    );

    let above = hits
        .iter()
        .find(|(_, hit)| *hit == Hit::KeyRow(1))
        .expect("its neighbour was drawn too")
        .0;
    assert_ne!(
        theme::token_of(buffer[(above.x, above.y)].bg),
        Some(Token::SurfaceSelected),
        "and only that row"
    );
}

/// The track looks like a scrollbar, so it answers like one: a click jumps
/// there and a drag keeps jumping. Something drawn as a control that does
/// nothing is worse than not drawing it.
#[test]
fn the_track_can_be_dragged() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;
    let track = model
        .hits
        .iter()
        .find_map(|(rect, hit)| matches!(hit, Hit::KeysTrack(_)).then_some(*rect))
        .expect("the track is a target");

    // The bottom of the track is the bottom of the list, whatever it is.
    let last = model.key_rows().len() - 1;
    model.click(track.x, track.bottom() - 1);
    assert_eq!(model.keys_selected, last);

    // And it keeps answering while the button is held, without the row
    // under the pointer having to be a target of its own.
    let drag = |model: &mut TuiModel, row| {
        model.apply_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: track.x,
                row,
                modifiers: KeyModifiers::NONE,
            },
            140,
        );
    };
    drag(&mut model, track.y);
    assert_eq!(model.keys_selected, 0, "back to the top");
    drag(&mut model, track.y + track.height / 2);
    assert!(
        model.keys_selected > 0 && model.keys_selected < last,
        "and to the middle: {}",
        model.keys_selected
    );

    // Releasing ends the gesture — a later move must not still scroll.
    model.apply_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: track.x,
            row: track.y,
            modifiers: KeyModifiers::NONE,
        },
        140,
    );
    let settled = model.keys_selected;
    drag(&mut model, track.bottom() - 1);
    assert_eq!(model.keys_selected, settled, "the drag was let go of");
}

/// A long list that gives no sign of being long is a list nobody scrolls.
/// The track says both things at once: that there is more, and where in it
/// the window sits.
#[test]
fn a_list_taller_than_the_screen_says_where_the_window_is() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let thumb = theme::glyph(theme::Symbol::ScrollThumb);
    let column = |terminal: &Terminal<TestBackend>| -> Vec<usize> {
        buffer_rows(terminal)
            .into_iter()
            .enumerate()
            .filter(|(_, row)| row.contains(&thumb))
            .map(|(index, _)| index)
            .collect()
    };

    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let top = column(&terminal);
    assert!(!top.is_empty(), "the track is drawn at all");

    model.keys_selected = model.key_rows().len() - 1;
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let bottom = column(&terminal);
    assert!(
        bottom.first() > top.first(),
        "and it moved down with the window: {top:?} -> {bottom:?}"
    );
    assert!(
        !bottom.is_empty() && bottom.len() < 20,
        "a fraction of the track, not all of it: {bottom:?}"
    );

    // A list that fits gets none: a scrollbar on a full view says the
    // opposite of what it is for.
    let short = TuiModel {
        route: Route::Profiles,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal
        .draw(|frame| render(frame, &short, &mut hits))
        .unwrap();
    assert!(column(&terminal).is_empty());
}

/// The wheel reaches this list too. It is the longest one uze draws, and
/// a screen you scroll with the keyboard alone is the thing this whole
/// change exists to stop shipping.
#[test]
fn the_wheel_walks_the_keys_list_and_the_window_follows() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let wheel = |model: &mut TuiModel, kind| {
        model.apply_mouse(
            MouseEvent {
                kind,
                column: 60,
                row: 10,
                modifiers: KeyModifiers::NONE,
            },
            100,
        );
    };

    for _ in 0..40 {
        wheel(&mut model, MouseEventKind::ScrollDown);
    }
    assert_eq!(model.keys_selected, 40, "the wheel walks the list");

    let rows = model.key_rows();
    let wanted = rows[40].action.label();
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    assert!(
        buffer_rows(&terminal)
            .iter()
            .any(|row| row.contains(&wanted)),
        "and what it walked to is on screen"
    );

    for _ in 0..80 {
        wheel(&mut model, MouseEventKind::ScrollUp);
    }
    assert_eq!(model.keys_selected, 0, "and back, stopping at the top");

    // Profiles was the other screen the wheel could not move, for the same
    // reason: its selection is three panels rather than one list, and the
    // mover the wheel called knew about neither.
    let mut profiles = TuiModel {
        route: Route::Profiles,
        focus: Focus::Content,
        ..model_with_data()
    };
    assert!(profiles.profiles.len() > 1, "there is somewhere to move to");
    wheel(&mut profiles, MouseEventKind::ScrollDown);
    assert_eq!(profiles.profiles_selected, 1, "the wheel moved it");
}

/// A group opens with a blank line and its keys sit in from its name.
/// Without either, the headings read as rows in a different colour and the
/// whole screen reads as one block of text.
#[test]
fn a_group_of_keys_is_set_apart_from_the_one_above_it() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let drawn = buffer_rows(&terminal);
    // Columns rather than byte offsets: these rows carry the sidebar's own
    // glyphs, and half of them are more than one byte wide.
    let column_of = |row: &str, needle: &str| {
        row.find(needle)
            .map(|byte| row[..byte].chars().count())
            .unwrap_or_else(|| panic!("{needle} is not on {row:?}"))
    };
    let heading = drawn
        .iter()
        .position(|row| row.contains("MANAGEMENT"))
        .expect("the second group is on screen");
    let name = column_of(&drawn[heading], "MANAGEMENT");
    let above: String = drawn[heading - 1].chars().skip(name).take(20).collect();
    assert!(
        above.trim().is_empty(),
        "a blank line opens it: {:?}",
        drawn[heading - 1]
    );
    // Where the row's own content begins, measured from the column the
    // heading begins at — not from any one glyph, since the marker column
    // is blank on every row but the selected one.
    let inset = drawn[heading + 1]
        .chars()
        .skip(name)
        .take_while(|glyph| *glyph == ' ')
        .count();
    assert!(
        inset > 0,
        "and its keys sit in from it: {:?} / {:?}",
        drawn[heading],
        drawn[heading + 1]
    );
}

/// The list says what each action does, not only what it is called. The
/// sentence lived in the drawer alone, which made the list a column of
/// labels you had to open one at a time to read.
#[test]
fn a_key_is_listed_with_the_sentence_that_explains_it() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        ..TuiModel::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let drawn = buffer_rows(&terminal);
    let row = drawn
        .iter()
        .find(|row| row.contains("Switch mode"))
        .expect("the mode key is on screen");
    assert!(row.contains("Move between the"), "{row:?}");

    // Narrow enough and the sentence goes rather than being cut to a stub.
    let mut narrow = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    narrow
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let row = buffer_rows(&narrow)
        .into_iter()
        .find(|row| row.contains("Switch mode"))
        .expect("the mode key is still on screen");
    assert!(!row.contains("Move between"), "{row:?}");
}

/// Everything that could be wrong with a key is said before anything is
/// written. A screen that let someone lock themselves out would be worse
/// than one with no rebinding at all.
#[test]
fn a_key_that_would_break_something_is_refused_with_the_reason() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        keyboard: crate::ui::keys::KeyboardSupport { enhanced: false },
        ..TuiModel::default()
    };
    let row = model
        .key_rows()
        .iter()
        .position(|row| row.action == uze_keys::Action::NewShellTab)
        .expect("listed");
    model.keys_selected = row;
    model.keys_capture = true;

    // `ctrl+g` already opens the changes in this same keyboard.
    model.apply_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
    assert!(
        model
            .keys_problem
            .as_deref()
            .is_some_and(|problem| problem.contains("ctrl+g")),
        "{:?}",
        model.keys_problem
    );
    assert!(model.keys_capture, "still asking — nothing was written");
    assert_eq!(
        uze_keys::active().chord_for(uze_keys::Action::NewShellTab, &[uze_keys::Scope::Workspace]),
        uze_keys::Chord::parse("ctrl+t").ok()
    );

    // And whatever arrives is reported, which is the only honest answer
    // to "will this key reach uze on my terminal".
    assert!(
        model
            .keys_probe
            .as_deref()
            .is_some_and(|probe| probe.contains("ctrl+g")),
        "{:?}",
        model.keys_probe
    );
}

/// A chord this terminal has no way of sending is refused rather than
/// accepted and left looking alive.
#[test]
fn a_key_this_terminal_cannot_send_is_never_bound() {
    let _turn = KEYBOARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = TuiModel {
        route: Route::Keys,
        focus: Focus::Content,
        keyboard: crate::ui::keys::KeyboardSupport { enhanced: false },
        ..TuiModel::default()
    };
    model.keys_capture = true;
    // Ctrl+digit has no encoding at all without the enhancement protocol.
    model.apply_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL));
    assert!(
        model
            .keys_problem
            .as_deref()
            .is_some_and(|problem| problem.contains("cannot send")),
        "{:?}",
        model.keys_problem
    );
}

/// The drawer is where a row's actions are performed with the pointer, so
/// every screen that has actions draws them there, as buttons, for exactly
/// what can be done now.
#[test]
fn every_drawer_draws_what_its_row_can_do_as_buttons() {
    for route in [
        Route::Plugins,
        Route::Harnesses,
        Route::Profiles,
        Route::Keys,
    ] {
        let mut model = model_with_data();
        model.set_route(route);
        model.focus = Focus::Content;
        model.marketplace_drawer_open = true;
        model.harnesses_drawer_open = true;
        let available: Vec<_> = model
            .selected_offers()
            .into_iter()
            .filter(|offer| offer.is_available())
            .map(|offer| offer.action)
            .collect();
        // The sample's one plugin is the official one, installed and
        // current: nothing to do, so nothing is drawn — the buttons a
        // plugin does draw have a test of their own.
        assert!(
            route == Route::Plugins || !available.is_empty(),
            "{route:?} offers nothing to do"
        );
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        for action in available {
            assert!(
                hits.iter()
                    .any(|(_, hit)| *hit == Hit::OfferedAction(action)),
                "{route:?} drew no button for {action}"
            );
        }
    }
}

/// Every right-hand drawer is the same slab: it starts on the frame's own
/// top row and runs to the bottom. A drawer handed the area *below* the
/// screen header instead starts three rows down, which reads as a panel
/// that failed to open rather than as a deliberate inset.
#[test]
fn every_drawer_runs_the_full_height_of_its_screen() {
    let mut rules = Vec::new();
    for route in [
        Route::Plugins,
        Route::Extensions,
        Route::Harnesses,
        Route::Keys,
        Route::Appearance,
    ] {
        let mut model = model_with_data();
        model.set_route(route);
        model.focus = Focus::Content;
        model.marketplace_drawer_open = true;
        model.harnesses_drawer_open = true;
        model.extension_drawer_open = true;
        model.appearance_themes = vec![uze_application::application::ThemeSummary {
            id: "default".to_owned(),
            active: true,
            path: None,
        }];
        model.appearance_glyph_sets = vec![uze_application::application::GlyphSetSummary {
            id: "nerd".to_owned(),
            active: false,
        }];
        model.settle_appearance_selection();

        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rule = hits
            .iter()
            .find_map(|(rect, hit)| matches!(hit, Hit::ResizePanel(_)).then_some(*rect))
            .unwrap_or_else(|| panic!("{route:?} drew no drawer"));
        rules.push((route, rule));
    }

    let (first_route, first) = rules[0];
    for (route, rule) in &rules[1..] {
        assert_eq!(
            (rule.y, rule.height),
            (first.y, first.height),
            "{route:?}'s drawer does not run the height {first_route:?}'s does"
        );
    }
}

/// A dialog answered only by a key would be the one place in the product
/// where the keyboard is the way in rather than the accelerator.
#[test]
fn a_question_is_answered_with_the_pointer_too() {
    let mut model = model_with_plugins(&["one"]);
    model.focus = Focus::Content;
    model.overlay = Overlay::ConfirmRemove {
        id: "one".to_owned(),
        focus: 1,
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    let button = |model: &TuiModel, action: uze_keys::Action| {
        model
            .hits
            .iter()
            .find(|(_, hit)| *hit == crate::ui::hit::Hit::OfferedAction(action))
            .map(|(rect, _)| *rect)
    };
    let cancel = button(&model, uze_keys::Action::ConfirmNo).expect("a way out you can click");
    let confirm = button(&model, uze_keys::Action::ConfirmYes).expect("and a way through");

    // Anywhere else declines, which is what keeps a stray click from
    // agreeing to a deletion.
    assert_eq!(model.click(0, 0), Intent::None);
    assert_eq!(model.overlay, Overlay::None);

    model.overlay = Overlay::ConfirmRemove {
        id: "one".to_owned(),
        focus: 1,
    };
    assert_eq!(model.click(cancel.x, cancel.y), Intent::None);
    assert_eq!(model.overlay, Overlay::None);

    model.overlay = Overlay::ConfirmRemove {
        id: "one".to_owned(),
        focus: 1,
    };
    assert_eq!(
        model.click(confirm.x, confirm.y),
        Intent::Remove("one".to_owned())
    );
}

/// The claim the Appearance screen exists to make: each glyph set is drawn
/// in *its own* glyphs, not in the ones currently in force. Without that,
/// choosing a set is choosing a name and hoping — which is the guess the
/// whole change removes, since no terminal can be asked what font it has.
#[test]
fn each_glyph_set_is_previewed_in_its_own_glyphs() {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let model = TuiModel {
        route: Route::Appearance,
        focus: Focus::Content,
        appearance_glyph_sets: uze_theme::glyph_sets()
            .iter()
            .map(|id| uze_application::application::GlyphSetSummary {
                id: (*id).to_owned(),
                active: false,
            })
            .collect(),
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);

    // A card names the set in its frame and draws the preview two rows
    // below: the frame, the line saying what the set asks of the machine,
    // then the marks themselves.
    let row_for = |id: &str| -> String {
        let title = rows
            .iter()
            .position(|row| row.contains('\u{256d}') && row.contains(id))
            .unwrap_or_else(|| panic!("no card for the `{id}` set in {rows:#?}"));
        rows[title + 2].clone()
    };

    // ASCII's own marks, on the ASCII row, while the default theme — the
    // one actually in force here — draws none of them.
    let ascii = row_for("ascii");
    assert!(
        ascii.contains('*'),
        "the ascii set is not in ascii: {ascii:?}"
    );
    assert!(
        !ascii.contains('✓'),
        "the ascii row borrowed the active theme's glyphs: {ascii:?}"
    );

    // The default set draws its own, on its own row.
    let default = row_for("default");
    assert!(
        default.contains('✓'),
        "the default set is not in its own glyphs: {default:?}"
    );

    // And the nerd set draws Codicons, which live in the private-use area.
    let nerd = row_for("nerd");
    assert!(
        nerd.chars().any(|c| ('\u{ea60}'..='\u{ec84}').contains(&c)),
        "the nerd row carries no Codicon: {nerd:?}"
    );
}

/// The card the keyboard is on says so on its frame; the card in force says
/// so with its fill. Painting the fill over the frame rather than inside it
/// takes the frame away from every card that is in force but not selected —
/// which is every card in force, most of the time.
#[test]
fn the_fill_that_marks_what_is_in_force_leaves_the_frame_alone() {
    let mut model = TuiModel {
        route: Route::Appearance,
        focus: Focus::Content,
        appearance_glyph_sets: uze_theme::glyph_sets()
            .iter()
            .map(|id| uze_application::application::GlyphSetSummary {
                id: (*id).to_owned(),
                active: *id == "nerd",
            })
            .collect(),
        ..TuiModel::default()
    };
    model.settle_appearance_selection();
    let in_force = model
        .appearance_rows()
        .iter()
        .position(|row| {
            matches!(
                row,
                crate::ui::model::AppearanceRow::GlyphSet { active: true, .. }
            )
        })
        .expect("a set in force to draw");
    assert_ne!(
        in_force, model.appearance_selected,
        "the case this guards is the card in force that the keyboard is not on"
    );

    let mut terminal = Terminal::new(TestBackend::new(190, 30)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();

    let card = hits
        .iter()
        .find_map(|(rect, hit)| (*hit == Hit::AppearanceRow(in_force)).then_some(*rect))
        .expect("the set in force drew no card");
    let buffer = terminal.backend().buffer();
    let fill = crate::ui::theme::color(uze_theme::Token::SurfaceSelected);
    assert_eq!(
        buffer[(card.x + 1, card.y + 1)].bg,
        fill,
        "the card in force is not filled"
    );
    assert_ne!(
        buffer[(card.x, card.y + 1)].bg,
        fill,
        "the fill reached the frame, which is where the selection has to show"
    );
}

#[test]
fn choosing_a_glyph_set_is_a_different_intent_from_choosing_a_theme() {
    let mut model = TuiModel {
        route: Route::Appearance,
        focus: Focus::Content,
        appearance_themes: vec![uze_application::application::ThemeSummary {
            id: "dracula".to_owned(),
            active: false,
            path: None,
        }],
        appearance_glyph_sets: vec![uze_application::application::GlyphSetSummary {
            id: "nerd".to_owned(),
            active: false,
        }],
        ..TuiModel::default()
    };
    model.settle_appearance_selection();

    // The list opens on the first *choice*, never on the heading above it.
    assert_eq!(
        model.activate_appearance(),
        crate::ui::worker::Intent::SelectTheme("dracula".to_owned())
    );

    // Walking down crosses the second heading without stopping on it.
    model.move_appearance_selection(1);
    assert_eq!(
        model.activate_appearance(),
        crate::ui::worker::Intent::SelectGlyphSet("nerd".to_owned()),
        "the glyph set was reached as if it were a theme"
    );
}

/// Coming back to Appearance builds the model afresh, so the selection has
/// to be settled again — on the theme in force, not on the first card.
#[test]
fn returning_to_appearance_lands_on_the_theme_in_force() {
    let theme = |id: &str, active: bool| uze_application::application::ThemeSummary {
        id: id.to_owned(),
        active,
        path: None,
    };
    let mut model = TuiModel {
        route: Route::Appearance,
        focus: Focus::Content,
        appearance_themes: vec![
            theme("default", false),
            theme("dracula", false),
            theme("tokyo-night", true),
        ],
        ..TuiModel::default()
    };
    model.settle_appearance_selection();
    assert_eq!(
        model.activate_appearance(),
        crate::ui::worker::Intent::SelectTheme("tokyo-night".to_owned()),
        "the selection fell back to the first card"
    );
}

#[test]
fn opening_appearance_asks_for_the_lists_it_chooses_from() {
    let mut model = TuiModel::default();
    assert_eq!(
        model.set_route(Route::Appearance),
        crate::ui::worker::Intent::LoadAppearance
    );
    // Every other route asks for nothing on arrival, so this one is not
    // paying for a read it does not need.
    assert_eq!(
        model.set_route(Route::Keys),
        crate::ui::worker::Intent::None
    );
}

/// Arriving is the only moment Appearance asks for its lists, so every way
/// of arriving has to carry the ask. A gesture that dropped it left the
/// screen showing its two headings and nothing under them — and leaving and
/// coming back was no cure, because coming back was the gesture that dropped
/// it.
#[test]
fn every_way_of_reaching_appearance_carries_the_ask() {
    let steps = uze_keys::Action::NextScreen;
    let landing = Route::Appearance.index();

    let mut walked = TuiModel::default();
    let mut asked = None;
    for _ in 0..ROUTES.len() {
        let intent = walked.act(steps);
        if walked.route.index() == landing {
            asked = Some(intent);
            break;
        }
    }
    assert_eq!(
        asked,
        Some(crate::ui::worker::Intent::LoadAppearance),
        "walking the sidebar reached Appearance without asking for its lists"
    );

    let mut clicked = TuiModel::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &clicked, &mut hits))
        .unwrap();
    clicked.hits = hits;
    let (rect, _) = clicked
        .hits
        .iter()
        .find(|(_, hit)| *hit == crate::ui::hit::Hit::Route(Route::Appearance))
        .expect("Appearance is reachable from the sidebar")
        .clone();
    assert_eq!(
        clicked.click(rect.x + 1, rect.y),
        crate::ui::worker::Intent::LoadAppearance,
        "clicking into Appearance reached it without asking for its lists"
    );
}

#[test]
fn clicking_a_glyph_set_chooses_it() {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let mut model = TuiModel {
        route: Route::Appearance,
        focus: Focus::Content,
        appearance_glyph_sets: vec![uze_application::application::GlyphSetSummary {
            id: "ascii".to_owned(),
            active: false,
        }],
        ..TuiModel::default()
    };
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    model.hits = hits;

    let rows = model.appearance_rows();
    let index = rows
        .iter()
        .position(|row| matches!(row, crate::ui::model::AppearanceRow::GlyphSet { .. }))
        .expect("a set row");
    let (rect, _) = model
        .hits
        .iter()
        .find(|(_, hit)| *hit == crate::ui::hit::Hit::AppearanceRow(index))
        .expect("the set row is clickable")
        .clone();

    assert_eq!(
        model.click(rect.x + 3, rect.y),
        crate::ui::worker::Intent::SelectGlyphSet("ascii".to_owned())
    );
}

/// A confirmation reads top to bottom as one thing: what is asked (once),
/// of what, what it means — wrapped, never cut — and the answers on the
/// right, the affirmative last, with the keys for them in the border.
#[test]
fn a_confirmation_dialog_reads_as_heading_subject_body_and_answers() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut model = model_with_data();
    model.overlay = Overlay::ConfirmDeleteProfile {
        id: "default".to_owned(),
        focus: 1,
    };
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
    let mut hits = Vec::new();
    terminal
        .draw(|frame| render(frame, &model, &mut hits))
        .unwrap();
    let rows = buffer_rows(&terminal);
    let position = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle} is drawn: {rows:#?}"))
    };
    assert_eq!(
        rows.iter()
            .filter(|row| row.contains("Delete profile"))
            .count(),
        1,
        "the question is asked once, not in the border and again below it"
    );
    assert_eq!(position("default"), position("Delete profile") + 1);
    assert!(
        position("configuration is touched.") > position("default"),
        "the explanation wraps inside the dialog rather than off its edge"
    );
    let answers = position("  Delete  ");
    assert!(answers > position("configuration is touched."));
    let row = &rows[answers];
    assert!(
        row.find("Cancel").unwrap() < row.find("Delete").unwrap(),
        "the affirmative is last, where reading ends: {row}"
    );
    assert!(
        position("esc cancel") > answers,
        "how to answer from the keyboard sits in the bottom border"
    );
    for action in [uze_keys::Action::ConfirmYes, uze_keys::Action::ConfirmNo] {
        assert!(
            hits.iter()
                .any(|(_, hit)| *hit == Hit::OfferedAction(action)),
            "each answer is a target: {action:?}"
        );
    }
}
