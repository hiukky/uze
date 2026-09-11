//! TUI — background worker threads: every product operation runs here
//! against a short-lived application facade, then reports back over the
//! channel so the render loop never blocks.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
    time::{Duration, Instant},
};

use uze_application::Preferences;

use uze_application::{
    PromptEntry, Result, UzeApplication, UzeError, UzeHome,
    application::{
        ContextPlan, ContextReconciliationReport, InstallReport, ProfileApplyResult,
        ProfilePreview, ProjectContextStatus, RemovePluginReport, UpdatePluginReport,
    },
};

use super::model::{Focus, Overlay, RefreshData, Status, TrustedRetry, TuiModel};
use super::tui_application;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrustGrant {
    Ask,
    Granted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Intent {
    None,
    Quit,
    /// Mirrors the Ctrl+O keybinding — clicking the sidebar's "work" mode
    /// label detaches from management the same way pressing the key does.
    SwitchToWorkspace,
    /// Leave management and re-select this tab in the workspace. Carries
    /// no space id: `Session::select_tab` moves the selected space along
    /// with the tab when they differ.
    SwitchToWorkspaceTab(u64),
    /// Delete the current workspace's recorded prompts.
    ClearPromptHistory,
    /// Write the operator's keyboard to `keys.json`. The keymap is already
    /// in force when this is sent — the screen swapped it between frames,
    /// which is what lets a rebinding be felt immediately. This is only
    /// the part that has to survive the process.
    PersistKeymap,
    /// Show what UZE can be drawn in.
    OpenThemePicker,
    /// Draw in this theme from now on, here and in the CLI.
    SelectTheme(String),
    /// Draw every mark from this glyph set from now on. Independent of the
    /// theme in both directions — neither call reads the other's half.
    SelectGlyphSet(String),
    /// Read the themes and the glyph sets the Appearance screen chooses
    /// from. Sent on arriving there rather than per frame, so the list
    /// cannot change under the cursor between two frames.
    LoadAppearance,
    Refresh,
    InspectPlugin(String),
    InspectMarketplacePlugin {
        name: String,
        marketplace: String,
    },
    Remove(String),
    Update(String, TrustGrant),
    Install {
        name: String,
        marketplace: String,
        grant: TrustGrant,
    },
    Setup(String),
    AddMarketplace(String),
    /// Hand a URL to the reader's own browser (the plugin drawer's Source
    /// card). Not a product operation — nothing is read or written — but
    /// it spawns a process, which is not something the render thread
    /// should be doing.
    OpenLink(String),
    ContextAnalyze(PathBuf),
    ContextApply(PathBuf),
    /// Reproduce the detected consumer workspace's `agents.lock` through
    /// the exact same Application use case `uze install` invokes.
    InstallProjectEnvironment(PathBuf),
    CreateProfile(String),
    DeleteProfile(String),
    /// Read what applying these preferences would write into each harness.
    PreviewProfile(super::model::PreviewQuestion),
    /// Fired on every Editor-panel value cycle — deliberately silent/no
    /// refresh (see `dispatch`'s arm), since the model already applied the
    /// new value optimistically and a status toast per keystroke would be
    /// noisy.
    UpdatePreferences {
        id: String,
        preferences: Preferences,
    },
    /// Carries the preferences on screen, written before applying: the
    /// editor persists each change on a thread of its own, and an apply
    /// that read the profile back from disk could overtake that write.
    ApplyProfile {
        id: String,
        preferences: Preferences,
        harness_ids: Vec<String>,
    },
}

impl Intent {
    /// The name a trace shows for this intent.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Quit => "quit",
            Self::SwitchToWorkspace => "switch_to_workspace",
            Self::SwitchToWorkspaceTab(_) => "switch_to_workspace_tab",
            Self::ClearPromptHistory => "clear_prompt_history",
            Self::PersistKeymap => "persist_keymap",
            Self::OpenThemePicker => "open_theme_picker",
            Self::SelectGlyphSet(_) => "select_glyph_set",
            Self::LoadAppearance => "load_appearance",
            Self::SelectTheme(_) => "select_theme",
            Self::Refresh => "refresh",
            Self::InspectPlugin(_) => "inspect_plugin",
            Self::InspectMarketplacePlugin { .. } => "inspect_marketplace_plugin",
            Self::Remove(_) => "remove",
            Self::Update(..) => "update",
            Self::Install { .. } => "install",
            Self::Setup(_) => "setup",
            Self::AddMarketplace(_) => "add_marketplace",
            Self::OpenLink(_) => "open_link",
            Self::ContextAnalyze(_) => "context_analyze",
            Self::ContextApply(_) => "context_apply",
            Self::InstallProjectEnvironment(_) => "install_project_environment",
            Self::CreateProfile(_) => "create_profile",
            Self::DeleteProfile(_) => "delete_profile",
            Self::PreviewProfile(_) => "preview_profile",
            Self::UpdatePreferences { .. } => "update_preferences",
            Self::ApplyProfile { .. } => "apply_profile",
        }
    }
}

pub(crate) enum WorkerResult {
    Refreshed(std::result::Result<RefreshData, String>),
    PluginInspected(std::result::Result<uze_application::application::PluginInspection, String>),
    MarketplaceInspected(
        std::result::Result<uze_application::application::MarketplacePluginDetail, String>,
    ),
    Mutated(std::result::Result<(String, RefreshData), String>),
    TrustRequired {
        plugin: String,
        detail: String,
        retry: TrustedRetry,
    },
    ContextAnalyzed(std::result::Result<(ProjectContextStatus, ContextPlan), String>),
    ContextApplied(std::result::Result<(String, ContextReconciliationReport), String>),
    ProfileApplied(std::result::Result<(String, Vec<ProfileApplyResult>, RefreshData), String>),
    ProfilePreviewed(
        super::model::PreviewQuestion,
        std::result::Result<ProfilePreview, String>,
    ),
}

pub(crate) fn dispatch(
    intent: Intent,
    home: &UzeHome,
    sender: &Sender<WorkerResult>,
    model: &mut TuiModel,
) {
    if intent != Intent::None {
        model.status_expires_at = None;
    }
    // The key or click's span: every worker it starts captures this as its
    // parent, so a refresh's spans belong to the press that asked for it.
    let _span = tracing::info_span!("tui.intent", intent = intent.name()).entered();
    match intent {
        Intent::None
        | Intent::Quit
        | Intent::SwitchToWorkspace
        | Intent::SwitchToWorkspaceTab(_) => {}
        Intent::OpenThemePicker => {
            // Cheap enough to read here rather than on a thread: a JSON
            // read and a directory listing, the same work `uze theme list`
            // is budgeted for.
            let themes: Vec<(String, bool)> = tui_application(home.clone())
                .and_then(|app| app.themes().list(uze_theme::builtin_names()))
                .map(|themes| {
                    themes
                        .into_iter()
                        .map(|theme| (theme.id, theme.active))
                        .collect()
                })
                .unwrap_or_else(|_| {
                    uze_theme::builtin_names()
                        .iter()
                        .map(|id| ((*id).to_owned(), false))
                        .collect()
                });
            let selected = themes.iter().position(|(_, active)| *active).unwrap_or(0);
            model.overlay = crate::ui::model::Overlay::ThemePicker { themes, selected };
        }
        Intent::SelectTheme(id) => match select_theme(home, &id) {
            Ok(()) => {
                model.status = Status::Success(format!("Drawing in {id}"));
                load_appearance(home, model);
            }
            Err(error) => model.status = Status::Error(error),
        },
        Intent::SelectGlyphSet(id) => match select_glyph_set(home, &id) {
            Ok(()) => {
                model.status = Status::Success(format!("Drawing with the {id} glyphs"));
                load_appearance(home, model);
            }
            Err(error) => model.status = Status::Error(error),
        },
        Intent::LoadAppearance => load_appearance(home, model),
        Intent::PersistKeymap => {
            let file = uze_keys::difference_from_default(&uze_keys::active());
            let path = home.keymap_path();
            let written = if file.is_empty() {
                // An operator who put everything back leaves no file
                // behind: the default is not a thing to be written down.
                match std::fs::remove_file(&path) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                    _ => Ok(()),
                }
            } else {
                serde_json::to_string_pretty(&file)
                    .map_err(std::io::Error::other)
                    .and_then(|contents| std::fs::write(&path, contents + "\n"))
            };
            model.status = match written {
                Ok(()) => Status::Success("Keyboard saved".to_owned()),
                Err(error) => Status::Error(format!("{}: {error}", path.display())),
            };
        }
        Intent::ClearPromptHistory => {
            let root = model.workspace_root();
            match tui_application(home.clone())
                .and_then(|app| app.workspace().clear_prompt_history(&root))
            {
                Ok(()) => {
                    model.prompt_history.clear();
                    model.overview_prompt_selected = 0;
                    model.overview_prompt_hovered = None;
                    model.status = Status::Success("Prompt history cleared".to_owned());
                }
                Err(error) => model.status = Status::Error(error.to_string()),
            }
        }
        Intent::Refresh => {
            if model.maintenance_in_flight {
                return;
            }
            model.status = Status::Working("Refreshing environment…".to_owned());
            model.maintenance_in_flight = true;
            spawn_refresh(home.clone(), sender.clone(), model.context_root.clone());
        }
        Intent::InspectPlugin(id) => {
            model.inspection_in_flight = Some(Intent::InspectPlugin(id.clone()));
            model.status = Status::Working(format!("Inspecting {id}…"));
            let (home, sender) = (home.clone(), sender.clone());
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                let result = tui_application(home)
                    .and_then(|app| app.plugins().inspect(&id))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::PluginInspected(result));
            });
        }
        Intent::InspectMarketplacePlugin { name, marketplace } => {
            model.inspection_in_flight = Some(Intent::InspectMarketplacePlugin {
                name: name.clone(),
                marketplace: marketplace.clone(),
            });
            model.status = Status::Working(format!("Inspecting {name}…"));
            let (home, sender) = (home.clone(), sender.clone());
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                let result = tui_application(home)
                    .and_then(|app| app.marketplace().inspect_plugin(&marketplace, &name))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::MarketplaceInspected(result));
            });
        }
        Intent::OpenLink(url) => {
            model.status = match open_in_browser(&url) {
                // Present tense on purpose: the opener took the address,
                // which is all that can be known without waiting on it —
                // and nothing on this thread waits for anything.
                Some(opener) => Status::Success(format!("Opening {url} via {opener}")),
                // The address is already on screen beside the glyph that
                // was clicked, so a failure here costs the reader a
                // copy-paste, not the link.
                None => Status::Error(format!("No browser to open {url} with")),
            };
        }
        Intent::Remove(id) => {
            model.status = Status::Working(format!("Removing {id}…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| app.plugins().remove(&id).map(remove_message),
            );
        }
        Intent::Update(id, grant) => {
            model.status = Status::Working(format!("Updating {id}…"));
            let retry_id = id.clone();
            spawn_trust_sensitive(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                grant,
                id.clone(),
                move |app, authority| app.plugins().update(&id, authority).map(update_message),
                TrustedRetry::Update(retry_id),
            );
        }
        Intent::Install {
            name,
            marketplace,
            grant,
        } => {
            model.status = Status::Working(format!("Installing {name}…"));
            let retry_name = name.clone();
            let retry_marketplace = marketplace.clone();
            let spec = format!("{name}@{marketplace}");
            spawn_trust_sensitive(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                grant,
                name,
                move |app, authority| {
                    app.marketplace()
                        .install_plugin(&spec, authority)
                        .map(|report| format!("Installed {}", report.plugin.id))
                },
                TrustedRetry::Install {
                    name: retry_name,
                    marketplace: retry_marketplace,
                },
            );
        }
        Intent::Setup(harness) => {
            model.status = Status::Working(format!("Setting up {harness}…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| {
                    app.setup(Some(&harness)).map(|results| {
                        results
                            .into_iter()
                            .find(|r| r.integration == harness)
                            .map(|r| {
                                if r.configured {
                                    format!("{harness} ready")
                                } else {
                                    format!("{harness} setup {:?}", r.provisioning.status)
                                }
                            })
                            .unwrap_or_else(|| format!("{harness} setup attempted"))
                    })
                },
            );
        }
        Intent::AddMarketplace(source) => {
            model.status = Status::Working(format!("Adding marketplace from {source}…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| {
                    app.marketplace().add(&source).map(|added| {
                        if added {
                            format!("Added marketplace from {source}")
                        } else {
                            format!("Marketplace from {source} is already added")
                        }
                    })
                },
            );
        }
        Intent::ContextAnalyze(root) => {
            model.status = Status::Working("Analyzing project context…".to_owned());
            let (home, sender) = (home.clone(), sender.clone());
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                let result = tui_application(home).and_then(|app| {
                    let status = app.context().inspect(&root)?;
                    let plan = app.context().plan(&root)?;
                    Ok((status, plan))
                });
                let _ = sender.send(WorkerResult::ContextAnalyzed(
                    result.map_err(|error| error.to_string()),
                ));
            });
        }
        Intent::InstallProjectEnvironment(root) => {
            model.status = Status::Working("Installing project environment…".to_owned());
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| {
                    // Same use case and same default (no trust flag) as the
                    // CLI's `uze install`; the TUI adds no install logic.
                    app.project()
                        .install(&root, &uze_application::NoTrustAuthority)
                        .map(|report| match report {
                            InstallReport::NoChanges => {
                                "Project environment already up to date".to_owned()
                            }
                            InstallReport::Installed {
                                plugins,
                                reconciled,
                                ..
                            } => match (plugins.len(), reconciled) {
                                (0, _) => "Project context reconciled".to_owned(),
                                (count, _) => format!(
                                    "Installed {count} plugin{}",
                                    if count == 1 { "" } else { "s" }
                                ),
                            },
                        })
                },
            );
        }
        Intent::ContextApply(root) => {
            model.status = Status::Working("Applying context reconciliation…".to_owned());
            let (home, sender) = (home.clone(), sender.clone());
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                let result = tui_application(home)
                    .and_then(|app| app.context().reconcile(&root))
                    .map(|report| ("Context reconciled".to_owned(), report))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::ContextApplied(result));
            });
        }
        Intent::CreateProfile(id) => {
            model.status = Status::Working(format!("Creating profile \"{id}\"…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| {
                    app.profiles()
                        .create(&id, None, Preferences::default())
                        .map(|()| format!("Created profile \"{id}\""))
                },
            );
        }
        Intent::DeleteProfile(id) => {
            model.status = Status::Working(format!("Deleting profile \"{id}\"…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| {
                    app.profiles()
                        .delete(&id)
                        .map(|()| format!("Deleted profile \"{id}\""))
                },
            );
        }
        Intent::PreviewProfile(question) => {
            model.profile_preview_asked = Some(question.clone());
            let (home, sender) = (home.clone(), sender.clone());
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                let result = tui_application(home)
                    .map(|app| {
                        app.profiles()
                            .preview(&question.preferences, &question.harness_ids)
                    })
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::ProfilePreviewed(question, result));
            });
        }
        Intent::UpdatePreferences { id, preferences } => {
            let home = home.clone();
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                if let Ok(app) = tui_application(home) {
                    let _ = app.profiles().update_preferences(&id, preferences);
                }
            });
        }
        Intent::ApplyProfile {
            id,
            preferences,
            harness_ids,
        } => {
            model.status = Status::Working(format!("Applying \"{id}\"…"));
            let (home, sender, context_root) =
                (home.clone(), sender.clone(), model.context_root.clone());
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _span = tracing::info_span!("tui.worker").entered();
                let result = tui_application(home.clone())
                    .and_then(|app| {
                        app.profiles().update_preferences(&id, preferences)?;
                        app.profiles().set_active(&id)?;
                        let results = app.profiles().apply(&id, &harness_ids)?;
                        let data = load_refresh_data(home, &context_root)?;
                        Ok({
                            let message = apply_message(&id, &results);
                            (message, results, data)
                        })
                    })
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::ProfileApplied(result));
            });
        }
    }
}

pub(crate) fn spawn_refresh(home: UzeHome, sender: Sender<WorkerResult>, context_root: PathBuf) {
    let parent = tracing::Span::current();
    thread::spawn(move || {
        let _parent = parent.enter();
        let _span = tracing::info_span!("tui.refresh").entered();
        let result = load_refresh_data(home, &context_root).map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(result));
    });
}

/// The one-time startup path, run in the background so the terminal takes
/// over instantly instead of sitting blank while default plugins are
/// seeded. Before this moved here, `main` ran `ensure_default_plugins`
/// synchronously — several harness-detection subprocess spawns — *before*
/// the alternate screen was even entered, so the terminal appeared frozen
/// for that whole stretch.
///
/// Started once per session by [`super::management::ManagementMemory::warming`],
/// at launch rather than on the first Ctrl+O into the management client:
/// bootstrap and refresh share one worker, so the one answer it composes
/// is normally waiting by the time that screen is asked for. Every
/// subsequent refresh (`Intent::Refresh`) goes through `spawn_refresh` and
/// is coalesced while a worker is in flight.
pub(crate) fn spawn_startup(home: UzeHome, sender: Sender<WorkerResult>, context_root: PathBuf) {
    let parent = tracing::Span::current();
    thread::spawn(move || {
        let _parent = parent.enter();
        let _span = tracing::info_span!("tui.startup").entered();
        let mut applied = Vec::new();
        if let Ok(app) = tui_application(home.clone()) {
            let _ = app.ensure_default_plugins();
            // Opening uze is the explicit, interactive act the CLI's
            // read-only dispatch path deliberately isn't, so this is where
            // a pending official-snapshot update gets applied instead of
            // only reported. Anything it can't settle alone (new
            // executable capability, managed state it refuses to disturb)
            // stays reported — `update_available` survives, and the `u`
            // action with its trust dialog is still the way through.
            applied = app
                .plugins()
                .auto_update()
                .into_iter()
                .filter(|outcome| outcome.applied)
                .map(|outcome| outcome.plugin)
                .collect();
        }
        let refreshed = load_refresh_data(home, &context_root)
            .map(|data| RefreshData {
                auto_updated: applied,
                ..data
            })
            .map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(refreshed));
    });
}

/// How many prompts a listing carries.
const PROMPT_HISTORY_LIMIT: usize = 20;

/// The workspace's recent prompts, read on the caller's thread.
///
/// One small owner-only file under `UzeHome`, and the one part of a
/// refresh with no reason to wait on the rest of it. `spawn_startup` seeds
/// default plugins and runs the official-snapshot auto-update — harness
/// detection, possibly the network — before it composes a `RefreshData`,
/// so a management screen that learned its history only from that worker
/// opened reading "no history yet" for as long as those took. That is the
/// same words the Overview says when there genuinely is none, which made
/// prompts already on disk look lost.
pub(crate) fn recent_prompts(home: UzeHome, context_root: &std::path::Path) -> Vec<PromptEntry> {
    let Ok(app) = tui_application(home) else {
        return Vec::new();
    };
    // The root the workspace client records against — resolved the same
    // way `load_refresh_data` resolves it, so the seed and the refresh
    // that replaces it read one file.
    let root = app.workspace().root(context_root);
    app.workspace().prompt_history(&root, PROMPT_HISTORY_LIMIT)
}

fn load_refresh_data(home: UzeHome, context_root: &std::path::Path) -> Result<RefreshData> {
    let app = tui_application(home)?;
    let snapshot = app.machine_snapshot(context_root, PROMPT_HISTORY_LIMIT)?;
    let mut plugins = snapshot.plugins;
    // Official plugins always lead the list — a stable sort keeps every
    // other ordering (whatever `list_plugins` returns) untouched within
    // each of the two groups.
    plugins.sort_by_key(|plugin| !plugin.source.starts_with("embedded:"));
    Ok(RefreshData {
        plugins,
        doctor: Some(snapshot.doctor),
        marketplace_plugins: snapshot.marketplace_plugins,
        marketplaces: snapshot.marketplaces,
        profiles: snapshot.profiles,
        context_status: snapshot.context_status,
        workspace: snapshot.workspace,
        prompt_history: snapshot.prompt_history,
        // Only `spawn_startup` ever fills this in; an ordinary refresh
        // reports no auto-updates rather than re-raising old badges.
        auto_updated: Vec::new(),
    })
}

fn spawn_mutation(
    home: UzeHome,
    sender: Sender<WorkerResult>,
    context_root: PathBuf,
    operation: impl FnOnce(&UzeApplication) -> Result<String> + Send + 'static,
) {
    let parent = tracing::Span::current();
    thread::spawn(move || {
        let _parent = parent.enter();
        let _span = tracing::info_span!("tui.mutation").entered();
        let result = tui_application(home.clone()).and_then(|app| {
            let message = operation(&app)?;
            let data = load_refresh_data(home, &context_root)?;
            Ok((message, data))
        });
        let _ = sender.send(WorkerResult::Mutated(
            result.map_err(|error| error.to_string()),
        ));
    });
}

/// Like `spawn_mutation`, but the operation is one that can cross the trust
/// boundary. With `TrustGrant::Ask` it runs non-interactively
/// (`NoTrustAuthority`) and, on `TRUST_REQUIRED`, surfaces a dialog rather
/// than failing silently or granting on the operator's behalf.
/// `TrustGrant::Granted` is only ever reached by that dialog's own explicit
/// confirmation re-dispatching the same action.
fn spawn_trust_sensitive(
    home: UzeHome,
    sender: Sender<WorkerResult>,
    context_root: PathBuf,
    grant: TrustGrant,
    package_hint: String,
    operation: impl FnOnce(&UzeApplication, &dyn uze_application::TrustAuthority) -> Result<String>
    + Send
    + 'static,
    retry: TrustedRetry,
) {
    let parent = tracing::Span::current();
    thread::spawn(move || {
        let _parent = parent.enter();
        let _span = tracing::info_span!("tui.trust_sensitive").entered();
        let outcome = tui_application(home.clone()).and_then(|app| {
            let result = match grant {
                TrustGrant::Ask => operation(&app, &uze_application::NoTrustAuthority),
                TrustGrant::Granted => operation(&app, &uze_application::AlwaysTrust),
            };
            result.map(|message| (message, ()))
        });
        match outcome {
            Ok((message, ())) => match load_refresh_data(home, &context_root) {
                Ok(data) => {
                    let _ = sender.send(WorkerResult::Mutated(Ok((message, data))));
                }
                Err(error) => {
                    let _ = sender.send(WorkerResult::Mutated(Err(error.to_string())));
                }
            },
            Err(UzeError::TrustRequired { package, detail }) => {
                let _ = sender.send(WorkerResult::TrustRequired {
                    plugin: if package.is_empty() {
                        package_hint
                    } else {
                        package
                    },
                    detail,
                    retry,
                });
            }
            Err(error) => {
                let _ = sender.send(WorkerResult::Mutated(Err(error.to_string())));
            }
        }
    });
}

pub(crate) fn drain_worker_results(
    model: &mut TuiModel,
    receiver: &std::sync::mpsc::Receiver<WorkerResult>,
) {
    while let Ok(result) = receiver.try_recv() {
        match result {
            WorkerResult::Refreshed(Ok(data)) => {
                let repaired = data
                    .doctor
                    .as_ref()
                    .map(|doctor| doctor.maintenance.repaired_count())
                    .unwrap_or_default();
                let updated = data.auto_updated.len();
                model.refreshed(data);
                model.maintenance_in_flight = false;
                // The badge only exists on the Plugins screen, and startup
                // lands on Overview — this is how an operator who never
                // opens Plugins still learns something changed under them.
                if updated > 0 {
                    model.status = Status::Success(format!(
                        "Updated {updated} plugin{}",
                        if updated == 1 { "" } else { "s" }
                    ));
                    model.status_expires_at = Some(Instant::now() + Duration::from_secs(5));
                } else if repaired > 0 {
                    model.status = Status::Success(format!(
                        "Synchronized {repaired} attachment{}",
                        if repaired == 1 { "" } else { "s" }
                    ));
                }
            }
            WorkerResult::PluginInspected(Ok(inspection)) => {
                model.plugin_detail = Some(inspection);
                model.inspection_in_flight = None;
                model.status = Status::Idle;
            }
            WorkerResult::MarketplaceInspected(Ok(detail)) => {
                model.marketplace_detail = Some(detail);
                model.inspection_in_flight = None;
                model.status = Status::Idle;
            }
            WorkerResult::Mutated(Ok((message, data))) => {
                model.refreshed(data);
                model.status = Status::Success(message);
            }
            WorkerResult::TrustRequired {
                plugin,
                detail,
                retry,
            } => {
                model.overlay = Overlay::TrustRequired {
                    plugin,
                    detail,
                    retry,
                };
                model.focus = Focus::Overlay;
                model.status = Status::Idle;
            }
            WorkerResult::ContextAnalyzed(Ok((status, plan))) => {
                model.context_status = Some(status);
                model.context_plan = Some(plan);
                model.status = Status::Idle;
            }
            WorkerResult::ContextApplied(Ok((message, report))) => {
                model.status = Status::Success(message);
                let _ = report;
            }
            WorkerResult::ProfileApplied(Ok((message, results, data))) => {
                model.refreshed(data);
                model.profile_apply_results = results;
                model.status = Status::Success(message);
                model.status_expires_at = Some(Instant::now() + Duration::from_secs(5));
            }
            WorkerResult::ProfilePreviewed(question, result) => {
                model.profile_previewed(question, result);
            }
            WorkerResult::Refreshed(Err(error)) => {
                model.maintenance_in_flight = false;
                model.status = Status::Error(error);
            }
            // A failed inspection stays failed until the selection moves:
            // clearing the in-flight marker here would have the per-frame
            // check retry it forever, error after error.
            WorkerResult::PluginInspected(Err(error))
            | WorkerResult::MarketplaceInspected(Err(error)) => {
                model.status = Status::Error(error);
            }
            WorkerResult::Mutated(Err(error))
            | WorkerResult::ContextAnalyzed(Err(error))
            | WorkerResult::ContextApplied(Err(error))
            | WorkerResult::ProfileApplied(Err(error)) => model.status = Status::Error(error),
        }
    }
}

/// `Applied profile "default" to 3 harnesses · 1 approximation` — the one
/// concise status line the spec asks for; per-harness detail lives in the
/// Harnesses panel's badges (`model.profile_apply_results`), not here.
fn apply_message(id: &str, results: &[ProfileApplyResult]) -> String {
    use uze_application::PreferenceApplyOutcome;
    let approximated = results
        .iter()
        .filter(|result| {
            matches!(
                result.outcome,
                PreferenceApplyOutcome::AppliedWithApproximation { .. }
            )
        })
        .count();
    let failed = results
        .iter()
        .filter(|result| matches!(result.outcome, PreferenceApplyOutcome::Failed { .. }))
        .count();
    let mut message = format!(
        "Applied profile \"{id}\" to {} harness{}",
        results.len(),
        if results.len() == 1 { "" } else { "es" }
    );
    if approximated > 0 {
        message.push_str(&format!(
            " · {approximated} approximation{}",
            if approximated == 1 { "" } else { "s" }
        ));
    }
    if failed > 0 {
        message.push_str(&format!(" · {failed} failed",));
    }
    message
}

fn remove_message(report: RemovePluginReport) -> String {
    match report {
        RemovePluginReport::Removed { plugin, .. } => format!("Removed {plugin}"),
        RemovePluginReport::AlreadyAbsent { plugin } => {
            format!("No UZE state remains for {plugin}")
        }
        RemovePluginReport::Blocked { report, .. } => {
            format!(
                "{} changed outside UZE; managed state was preserved",
                report.package_id
            )
        }
    }
}

fn update_message(report: UpdatePluginReport) -> String {
    match report {
        UpdatePluginReport::Updated { plugin, .. } => format!("Updated {}", plugin.id),
        UpdatePluginReport::Blocked { report, .. } => {
            format!(
                "{} update blocked; managed state was preserved",
                report.package_id
            )
        }
    }
}

/// Hands `url` to whatever this machine opens links with, and answers with
/// the name of the opener that accepted it.
///
/// Ordered by how directly each one speaks for the user: `$BROWSER` is
/// what they set themselves, `xdg-open` is what the desktop answers with,
/// `sensible-browser` is Debian's fallback, and `explorer.exe` answers on
/// WSL — where `xdg-open` is installed as somebody else's dependency,
/// accepts the address and exits 4 without opening a thing.
///
/// `wslview` is deliberately absent: it is the tool that case calls for,
/// but its project (`wslutilities/wslu`) was archived in March 2025, and
/// it only shells out to the Windows interop this list now reaches
/// directly. Anyone still running it names it in `$BROWSER`, which is
/// honoured ahead of everything here.
///
/// Every stream is closed: the alternate screen belongs to ratatui, and a
/// browser's startup chatter written into it lands in the middle of the
/// frame.
fn open_in_browser(url: &str) -> Option<String> {
    // `$BROWSER` is a colon-separated list, and an entry may carry the URL
    // in a `%s` placeholder rather than as a trailing argument.
    let configured = std::env::var("BROWSER").unwrap_or_default();
    let preferred: Vec<&str> = configured.split(':').filter(|e| !e.is_empty()).collect();
    // `open` first, and only on macOS: it is the one opener there, and the
    // three below are all absent — so every link the workspace offered on a
    // Mac died as "no browser to open it with" unless the operator had set
    // `$BROWSER`. Written when Linux was the only reader, and found by
    // asking a second platform.
    let native: &[&str] = if cfg!(target_os = "macos") {
        &["open"]
    } else {
        &["xdg-open", "sensible-browser", "explorer.exe"]
    };
    for opener in preferred.into_iter().chain(native.iter().copied()) {
        let mut words = opener.split_whitespace();
        let Some(program) = words.next() else {
            continue;
        };
        let mut command = Command::new(program);
        let mut placed = false;
        for word in words {
            command.arg(word.replace("%s", url));
            placed |= word.contains("%s");
        }
        if !placed {
            command.arg(url);
        }
        let spawned = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            // A launcher hands the URL over and exits at once; nobody is
            // waiting on it, so reap it off-thread rather than leaving a
            // zombie behind for as long as the TUI runs.
            let parent = tracing::Span::current();
            thread::spawn(move || {
                let _parent = parent.enter();
                let _ = child.wait();
            });
            return Some(program.to_owned());
        }
    }
    None
}

/// Puts a theme in force and records it, so the next frame — and the next
/// `uze` command — are drawn in it. No session, pane or agent is touched:
/// changing what UZE looks like is not an event in the work it is hosting.
/// Records the glyph set and puts the re-resolved stack in force.
///
/// Resolved against whichever theme is active — including none, which
/// resolves the default — because the set is a layer beneath the theme
/// rather than a theme of its own.
fn select_glyph_set(home: &UzeHome, id: &str) -> std::result::Result<(), String> {
    let application = tui_application(home.clone()).map_err(|error| error.to_string())?;
    application
        .themes()
        .select_glyphs(id)
        .map_err(|error| error.to_string())?;
    let theme = application
        .themes()
        .active()
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| uze_theme::builtin_names()[0].to_owned());
    let loaded =
        crate::theme::resolve(&application, home, &theme).map_err(|error| error.to_string())?;
    uze_theme::set_active(loaded.theme);
    Ok(())
}

/// What a theme card shows of a palette: the accent it leads with, then
/// the four states that carry meaning, then the brightest text. Six is what
/// a card has room for, and these six are the ones a theme is actually
/// judged on — a row of surfaces would be six shades of the same near-black.
const SWATCHES: &[uze_theme::Token] = &[
    uze_theme::Token::Accent,
    uze_theme::Token::StateSuccess,
    uze_theme::Token::StateWarning,
    uze_theme::Token::StateDanger,
    uze_theme::Token::StateInfo,
    uze_theme::Token::TextBright,
];

/// Reads both lists the Appearance screen chooses from.
///
/// Cheap enough to read on this thread rather than a worker, for the same
/// reason the theme picker reads its own: a JSON read and a directory
/// listing, which is exactly the work `uze theme list` is budgeted for.
fn load_appearance(home: &UzeHome, model: &mut TuiModel) {
    model.appearance_read = true;
    let Ok(application) = tui_application(home.clone()) else {
        return;
    };
    if let Ok(themes) = application.themes().list(uze_theme::builtin_names()) {
        // Resolved here rather than per frame: each one is a file read, and
        // a screen that re-read the whole themes directory every tick would
        // be paying a directory walk to draw six coloured cells.
        model.appearance_palettes = themes
            .iter()
            .filter_map(|theme| {
                let loaded = crate::theme::resolve(&application, home, &theme.id).ok()?;
                let colours = SWATCHES
                    .iter()
                    .map(|token| loaded.theme.color(*token))
                    .collect();
                Some((theme.id.clone(), colours))
            })
            .collect();
        model.appearance_themes = themes;
    }
    if let Ok(sets) = application.themes().glyph_sets(uze_theme::glyph_sets()) {
        model.appearance_glyph_sets = sets;
    }
    model.settle_appearance_selection();
}

fn select_theme(home: &UzeHome, id: &str) -> std::result::Result<(), String> {
    let application = tui_application(home.clone()).map_err(|error| error.to_string())?;
    let loaded =
        crate::theme::resolve(&application, home, id).map_err(|error| error.to_string())?;
    application
        .themes()
        .select(id)
        .map_err(|error| error.to_string())?;
    uze_theme::set_active(loaded.theme);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::mpsc};

    use uze_application::application::{
        DoctorReport, MaintenanceOutcome, MaintenanceReport, StoreHealth,
    };

    use super::*;

    fn refreshed_with_repair() -> RefreshData {
        RefreshData {
            doctor: Some(DoctorReport {
                uze_home: PathBuf::from("/home/uze"),
                store: StoreHealth::Ready,
                plugins: Vec::new(),
                harnesses: Vec::new(),
                attachments: Vec::new(),
                ledger_error: None,
                integration_state_error: None,
                provisioning_state_error: None,
                maintenance: MaintenanceReport {
                    outcomes: vec![MaintenanceOutcome::Repaired {
                        plugin: "fixture@local".to_owned(),
                        integration: "fixture".to_owned(),
                        receipt: "fixture:skill".to_owned(),
                    }],
                },
            }),
            ..RefreshData::default()
        }
    }

    #[test]
    fn repaired_maintenance_is_a_success_notification_not_a_problem() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(WorkerResult::Refreshed(Ok(refreshed_with_repair())))
            .unwrap();
        let mut model = TuiModel {
            maintenance_in_flight: true,
            ..TuiModel::default()
        };

        drain_worker_results(&mut model, &receiver);

        assert!(!model.maintenance_in_flight);
        assert_eq!(
            model.status,
            Status::Success("Synchronized 1 attachment".to_owned())
        );
    }

    #[test]
    fn refresh_is_coalesced_while_maintenance_is_in_flight() {
        let (sender, receiver) = mpsc::channel();
        let home = UzeHome::at(uze_testkit::temp::scratch("worker-coalesce"));
        let mut model = TuiModel {
            maintenance_in_flight: true,
            ..TuiModel::default()
        };

        dispatch(Intent::Refresh, &home, &sender, &mut model);

        assert!(receiver.try_recv().is_err());
        assert!(model.maintenance_in_flight);
        assert_eq!(model.status, Status::Idle);
    }
}
