//! TUI — background worker threads: every product operation runs here
//! against a short-lived application facade, then reports back over the
//! channel so the render loop never blocks.

use std::{path::PathBuf, sync::mpsc::Sender, thread};

use crate::{
    Result, UzeApplication, UzeError, UzeHome,
    application::{
        ContextPlan, ContextReconciliationReport, ProjectContextStatus, RemovePluginReport,
        UpdatePluginReport,
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
    ContextAnalyze(PathBuf),
    ContextApply(PathBuf),
}

pub(crate) enum WorkerResult {
    Refreshed(std::result::Result<RefreshData, String>),
    PluginInspected(std::result::Result<crate::application::PluginInspection, String>),
    MarketplaceInspected(std::result::Result<crate::application::MarketplacePluginDetail, String>),
    Mutated(std::result::Result<(String, RefreshData), String>),
    TrustRequired {
        plugin: String,
        detail: String,
        retry: TrustedRetry,
    },
    ContextAnalyzed(std::result::Result<(ProjectContextStatus, ContextPlan), String>),
    ContextApplied(std::result::Result<(String, ContextReconciliationReport), String>),
}

pub(crate) fn dispatch(
    intent: Intent,
    home: &UzeHome,
    sender: &Sender<WorkerResult>,
    model: &mut TuiModel,
) {
    match intent {
        Intent::None | Intent::Quit => {}
        Intent::Refresh => {
            model.status = Status::Working("Refreshing environment…".to_owned());
            spawn_refresh(home.clone(), sender.clone(), model.context_root.clone());
        }
        Intent::InspectPlugin(id) => {
            model.status = Status::Working(format!("Inspecting {id}…"));
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home)
                    .and_then(|app| app.inspect_plugin(&id))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::PluginInspected(result));
            });
        }
        Intent::InspectMarketplacePlugin { name, marketplace } => {
            model.status = Status::Working(format!("Inspecting {name}…"));
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home)
                    .and_then(|app| app.inspect_marketplace_plugin(&marketplace, &name))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::MarketplaceInspected(result));
            });
        }
        Intent::Remove(id) => {
            model.status = Status::Working(format!("Removing {id}…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| app.remove_plugin(&id).map(remove_message),
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
                move |app, authority| app.update_plugin(&id, authority).map(update_message),
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
                name.clone(),
                move |app, authority| {
                    app.plugin_install(&spec, authority)
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
                    app.marketplace_add(&source).map(|added| {
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
            thread::spawn(move || {
                let result = tui_application(home).and_then(|app| {
                    let status = app.context_inspect(&root)?;
                    let plan = app.context_plan(&root)?;
                    Ok((status, plan))
                });
                let _ = sender.send(WorkerResult::ContextAnalyzed(
                    result.map_err(|error| error.to_string()),
                ));
            });
        }
        Intent::ContextApply(root) => {
            model.status = Status::Working("Applying context reconciliation…".to_owned());
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home)
                    .and_then(|app| app.context_reconcile(&root))
                    .map(|report| ("Context reconciled".to_owned(), report))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::ContextApplied(result));
            });
        }
    }
}

fn spawn_refresh(home: UzeHome, sender: Sender<WorkerResult>, context_root: PathBuf) {
    thread::spawn(move || {
        let result = load_refresh_data(home, &context_root).map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(result));
    });
}

/// The one-time startup path, run in the background so the terminal takes
/// over instantly instead of sitting blank while default plugins are
/// seeded. Before this moved here, `main` ran `ensure_default_plugins`
/// synchronously — several harness-detection subprocess spawns — *before*
/// the alternate screen was even entered, so the terminal appeared frozen
/// for that whole stretch. Every subsequent refresh (`Intent::Refresh`)
/// goes through the plain `spawn_refresh` above; seeding defaults only
/// needs to happen once, at launch, not on every manual refresh.
pub(crate) fn spawn_startup(home: UzeHome, sender: Sender<WorkerResult>, context_root: PathBuf) {
    thread::spawn(move || {
        if let Ok(app) = tui_application(home.clone()) {
            let _ = app.ensure_default_plugins();
        }
        let result = load_refresh_data(home, &context_root).map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(result));
    });
}

fn load_refresh_data(home: UzeHome, context_root: &std::path::Path) -> Result<RefreshData> {
    let app = tui_application(home)?;
    let plugins = app.list_plugins()?;
    let doctor = app.doctor();
    let marketplace_count = app.marketplace_list()?.len();
    let marketplace_plugins = app.list_marketplace_plugins()?;
    let context_status = app.context_inspect(context_root).ok();
    Ok(RefreshData {
        plugins,
        doctor: Some(doctor),
        marketplace_plugins,
        marketplace_count,
        context_status,
    })
}

fn spawn_mutation(
    home: UzeHome,
    sender: Sender<WorkerResult>,
    context_root: PathBuf,
    operation: impl FnOnce(&UzeApplication) -> Result<String> + Send + 'static,
) {
    thread::spawn(move || {
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
    operation: impl FnOnce(&UzeApplication, &dyn crate::trust::TrustAuthority) -> Result<String>
    + Send
    + 'static,
    retry: TrustedRetry,
) {
    thread::spawn(move || {
        let outcome = tui_application(home.clone()).and_then(|app| {
            let result = match grant {
                TrustGrant::Ask => operation(&app, &crate::trust::NoTrustAuthority),
                TrustGrant::Granted => operation(&app, &crate::trust::AlwaysTrust),
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
            WorkerResult::Refreshed(Ok(data)) => model.refreshed(data),
            WorkerResult::PluginInspected(Ok(inspection)) => {
                model.plugin_detail = Some(inspection);
                model.status = Status::Idle;
            }
            WorkerResult::MarketplaceInspected(Ok(detail)) => {
                model.marketplace_detail = Some(detail);
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
            WorkerResult::Refreshed(Err(error))
            | WorkerResult::PluginInspected(Err(error))
            | WorkerResult::MarketplaceInspected(Err(error))
            | WorkerResult::Mutated(Err(error))
            | WorkerResult::ContextAnalyzed(Err(error))
            | WorkerResult::ContextApplied(Err(error)) => model.status = Status::Error(error),
        }
    }
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
