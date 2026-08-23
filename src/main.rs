//! Thin CLI presentation over `UzeApplication`.

// Test-only tooling (see command_performance.rs's module doc): a registry
// plus exhaustiveness tests, never wired into runtime command dispatch.
#[cfg(test)]
mod command_performance;
mod progress;
mod shim;

use std::{collections::BTreeMap, io::IsTerminal, path::PathBuf};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use uze::{
    Result, UzeApplication, UzeHome,
    application::{
        AddPluginReport, ContextPlan, ContextReconciliationReport, DoctorReport,
        HarnessContextDelivery, HarnessHealth, MarketplaceSummary, PluginInspection, Portability,
        ProjectContextStatus, RemovePluginReport, RemoveProjectPluginReport, StatusReport,
    },
    context::PlannedAction,
};

#[derive(Debug, Parser)]
#[command(
    name = "uze",
    version,
    about = "Manage one local agent plugin environment"
)]
struct Cli {
    /// Show delivery evidence and full attachment details
    #[arg(long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Install this project's environment from agents.lock
    Install {
        path: Option<PathBuf>,
        /// Authorize executable capabilities
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Remove a plugin from this project (never touches the machine Store —
    /// see `uze plugin remove` for that)
    Remove {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Show this project's environment status
    Status {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Manage this project's context (AGENTS.md)
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },
    /// Manage marketplace sources (machine-level)
    Market {
        #[command(subcommand)]
        action: MarketAction,
    },
    /// Manage plugins installed on this machine
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Manage agent harness integrations (machine-level)
    Harness {
        #[command(subcommand)]
        action: HarnessAction,
    },
    /// Run diagnostics
    Doctor {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Set up harness integrations
    Setup { harness: Option<String> },
    /// Reached only when the first argument matches none of the built-ins
    /// above — `clap`'s own generated matcher tries every named variant
    /// first, so this is the *sole* place `<plugin>@<market>` project
    /// shorthand is recognized, with one formal precedence rule (see
    /// `docs/adr/019-explicit-project-machine-boundary-in-cli-command-grammar.md`):
    /// no built-in name anywhere in this tree contains `@`, and the
    /// shorthand grammar requires it, so a first argument's
    /// shorthand-or-not classification is a single lexical fact, not a
    /// hand-maintained priority list.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum ContextAction {
    /// Read-only: what does this project's context currently look like?
    /// Never writes anything, in any state.
    Inspect {
        /// Project directory. Defaults to the current directory.
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Read-only: exactly what would `reconcile` change here? Never writes
    /// anything.
    Plan {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Writes: composes every installed package's contribution into this
    /// project's AGENTS.md, and reconciles the harness bridges it implies.
    Reconcile {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum MarketAction {
    /// Add a marketplace discovery source (local path or https://...).
    Add { source: String },
    /// List registered marketplaces (including embedded uze-official).
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Remove a marketplace (blocked while plugins from it are installed).
    Remove { name: String },
    /// Inspect one marketplace's own source and plugin count — distinct
    /// from inspecting one plugin within a marketplace (`plugin inspect`).
    Inspect {
        name: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum PluginAction {
    /// Install a plugin on this machine, from `name@marketplace` or a
    /// direct local path/Git URL — never touches the current project's
    /// `agents.lock` (use `uze <plugin>@<market>` for that).
    Install {
        plugin: String,
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// List plugins installed on this machine.
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Inspect one installed plugin's delivery.
    Inspect {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Remove a plugin from this machine (subject to ADR-009 lifecycle/
    /// drift safety) — never implied by `uze remove`.
    Remove {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Update a plugin to its latest version (re-resolves its source).
    Update {
        plugin: String,
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum HarnessAction {
    /// List detected harnesses and their setup state.
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Inspect one harness's detection/setup/provisioning detail.
    Inspect {
        name: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Provision this harness — identical to root `uze setup <name>`; this
    /// is the namespaced spelling, not a second implementation.
    Setup { name: Option<String> },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

/// `uze <plugin>@<market>` — the sole consumer of `Command::External`. A
/// real `clap::Parser`, not a hand-rolled loop: an unrecognized flag here
/// is rejected by `clap` itself (`try_parse_from` returns an `Err` this
/// binary's caller turns into `clap`'s own formatted error + exit), not
/// silently ignored, and `uze <plugin>@<market> --help` works for free.
#[derive(Debug, Parser)]
#[command(
    name = "uze",
    about = "Add a plugin to this project (project shorthand)"
)]
struct ShorthandArgs {
    /// `<plugin>@<market>`
    #[arg(value_name = "PLUGIN@MARKET")]
    spec: String,
    /// Authorize executable capabilities
    #[arg(long)]
    trust: bool,
    /// Show delivery evidence and full attachment details
    #[arg(long)]
    verbose: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

fn main() {
    // Checked before any `clap` parsing, on `argv[0]` alone: a process
    // invoked as `claude`/`codex`/`opencode`/`gemini` (via the symlink
    // `ensure_runtime_shim` creates at `~/.uze/shims/<name>`) never reaches
    // the ordinary `uze` subcommand grammar at all. `shim::run` diverges —
    // it always either `exec`s the real binary or exits.
    if let Some(name) = shim::detect() {
        shim::run(&name);
    }

    // Check for --help flag and render custom colored help. This is the
    // one thing still special-cased ahead of `clap` — it only changes how
    // help is *rendered*, not what argument dispatches where, so it does
    // not reintroduce the parallel-grammar problem the project shorthand
    // used to have (see `docs/adr/019-...md`).
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        print_colored_help();
        return;
    }

    if let Err(error) = run(Cli::parse()) {
        eprintln!("uze: {error}");
        std::process::exit(1);
    }
}

fn print_colored_help() {
    let cyan = "\x1b[36m";
    let green = "\x1b[32m";
    let reset = "\x1b[0m";
    let bold = "\x1b[1m";

    println!("{}UZE{} — Agent environment manager", bold, reset);
    println!();
    println!("{}Usage:{}", bold, reset);
    println!("  uze <plugin>@<market>");
    println!("  uze <command> [options]");
    println!();
    println!("{}Project:{}", bold, reset);
    println!(
        "  {}<plugin>@<market>{}   Add a plugin to this project",
        cyan, reset
    );
    println!(
        "  {}install{}             Install this project's environment from agents.lock",
        cyan, reset
    );
    println!(
        "  {}remove{}              Remove a plugin from this project",
        cyan, reset
    );
    println!(
        "  {}status{}              Show this project's environment status",
        cyan, reset
    );
    println!(
        "  {}context{}             Manage this project's AGENTS.md context",
        cyan, reset
    );
    println!();
    println!("{}Machine:{}", bold, reset);
    println!(
        "  {}market{}              Manage marketplace sources",
        cyan, reset
    );
    println!(
        "  {}plugin{}              Manage plugins installed on this machine",
        cyan, reset
    );
    println!(
        "  {}harness{}             Manage agent harness integrations",
        cyan, reset
    );
    println!();
    println!("{}Diagnostics:{}", bold, reset);
    println!("  {}doctor{}              Run diagnostics", cyan, reset);
    println!(
        "  {}setup{}               Set up harness integrations",
        cyan, reset
    );
    println!();
    println!("{}Options:{}", bold, reset);
    println!("  {}-h, --help{}     Print help", green, reset);
    println!("  {}-V, --version{}  Print version", green, reset);
    println!();
    println!("{}Examples:{}", bold, reset);
    println!("  uze flow@ai");
    println!("  uze install");
    println!("  uze market add hiukky/ai");
    println!("  uze plugin install flow@ai");
}

fn run(cli: Cli) -> Result<()> {
    let home = UzeHome::from_env()?;
    let verbose = cli.verbose;
    let Some(command) = cli.command else {
        if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
            // Seeding default marketplace plugins now happens inside the
            // TUI's own startup worker (see `ui::spawn_startup`), off the
            // terminal-takeover path — running it here, synchronously,
            // before the alternate screen is even entered, left the
            // terminal looking frozen for however long harness detection
            // took.
            return uze::ui::run(home);
        }
        Cli::command()
            .print_help()
            .map_err(|source| uze::UzeError::Write {
                path: PathBuf::from("stdout"),
                source,
            })?;
        println!();
        return Ok(());
    };
    let app = UzeApplication::from_env(home)?;
    // Seed the default marketplace plugins (`plugins/uze`) on every CLI
    // invocation. This makes the Skill globally available without a manual
    // `uze plugin install` and heals its attachment after a binary update.
    // Best-effort: failures here must not block `doctor`/`list` etc.
    let _ = app.ensure_default_plugins();
    match command {
        Command::Install {
            path,
            trust,
            format,
        } => {
            let authority = trust_authority(trust);
            let spinner = progress::spinner("Installing project environment...");
            match app.install_project_environment(&context_path(path), authority.as_ref()) {
                Ok(report) => {
                    let message = match &report {
                        uze::application::InstallReport::NoChanges => "Already up to date",
                        uze::application::InstallReport::Installed { .. } => {
                            "Environment installed"
                        }
                    };
                    spinner.finish_with_message(message);
                    match format {
                        OutputFormat::Text => print!("{}", render_install(&report)),
                        OutputFormat::Json => print_json(&report),
                    }
                }
                Err(e) => {
                    spinner.finish_with_message("Failed");
                    progress::error(&format!("Failed to install environment: {e}"));
                    return Err(e);
                }
            }
        }
        Command::Remove { plugin, format } => {
            let current_dir = std::env::current_dir().map_err(|source| uze::UzeError::Read {
                path: PathBuf::from("."),
                source,
            })?;
            let spinner = progress::spinner(&format!("Removing {plugin} from this project..."));
            let report = match app.remove_project_plugin(&plugin, &current_dir) {
                Ok(report) => report,
                Err(e) => {
                    spinner.finish_with_message("Failed");
                    progress::error(&format!("Failed to remove {plugin}: {e}"));
                    return Err(e);
                }
            };
            match report {
                RemoveProjectPluginReport::Removed { .. } => {
                    spinner.finish_with_message("Removed from project");
                    match format {
                        OutputFormat::Text => {
                            progress::success(&format!("Removed {plugin} from project"));
                        }
                        OutputFormat::Json => print_json(&RemoveProjectPluginReport::Removed {
                            plugin: plugin.clone(),
                        }),
                    }
                }
                // Strictly project-scoped, by design (ADR-019): neither of
                // these falls through to machine-level removal. `?` below
                // surfaces `uze::UzeError::{NoProjectEnvironment,
                // PluginNotUsedByProject}` through the same `uze: {error}`
                // path every other failure in this program uses.
                RemoveProjectPluginReport::NoLock => {
                    spinner.finish_with_message("Failed");
                    return Err(uze::UzeError::NoProjectEnvironment { plugin });
                }
                RemoveProjectPluginReport::NotInLock { .. } => {
                    spinner.finish_with_message("Failed");
                    return Err(uze::UzeError::PluginNotUsedByProject { plugin });
                }
            }
        }
        Command::Status { path, format } => {
            let report = app.status(&context_path(path))?;
            match format {
                OutputFormat::Text => print!("{}", render_status(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
        Command::Context { action } => match action {
            ContextAction::Inspect { path, format } => {
                let status = app.context_inspect(&context_path(path))?;
                match format {
                    OutputFormat::Text => print!("{}", render_context_status(&status)),
                    OutputFormat::Json => print_json(&status),
                }
            }
            ContextAction::Plan { path, format } => {
                let plan = app.context_plan(&context_path(path))?;
                match format {
                    OutputFormat::Text => print!("{}", render_context_plan(&plan)),
                    OutputFormat::Json => print_json(&plan),
                }
            }
            ContextAction::Reconcile { path, format } => {
                let report = app.context_reconcile(&context_path(path))?;
                match format {
                    OutputFormat::Text => print!("{}", render_context_reconciliation(&report)),
                    OutputFormat::Json => print_json(&report),
                }
            }
        },
        Command::Market { action } => match action {
            MarketAction::Add { source } => {
                let spinner = progress::spinner("Adding marketplace...");
                match app.marketplace_add(&source) {
                    Ok(true) => {
                        spinner.finish_with_message("Marketplace added");
                        progress::success(&format!("Added marketplace from {source}"));
                    }
                    Ok(false) => {
                        spinner.finish_with_message("Already added");
                        progress::success(&format!("Marketplace from {source} is already added"));
                    }
                    Err(e) => {
                        spinner.finish_with_message("Failed");
                        progress::error(&format!("Failed to add marketplace: {e}"));
                        return Err(e);
                    }
                }
            }
            MarketAction::List { format } => {
                let mps = app.marketplace_list()?;
                match format {
                    OutputFormat::Text => print!("{}", render_market_list(&mps)),
                    OutputFormat::Json => print_json(&mps),
                }
            }
            MarketAction::Remove { name } => {
                app.marketplace_remove(&name)?;
                println!("Removed marketplace {name}");
            }
            MarketAction::Inspect { name, format } => {
                let detail = app.market_inspect(&name)?;
                match format {
                    OutputFormat::Text => print!("{}", render_market_detail(&detail)),
                    OutputFormat::Json => print_json(&detail),
                }
            }
        },
        Command::Plugin { action } => match action {
            PluginAction::Install {
                plugin,
                trust,
                format,
            } => {
                let authority = trust_authority(trust);
                let spinner = progress::spinner(&format!("Installing plugin {plugin}..."));
                // `name@marketplace` resolves through the marketplace
                // registry; anything else (a local path or a Git URL) is a
                // direct source, exactly as root `uze add` used to accept
                // — that capability now lives here, not at the root (see
                // ADR-019's migration table).
                let installed = if plugin.contains('@') {
                    app.plugin_install(&plugin, authority.as_ref())
                } else {
                    app.add_plugin(parse_source(&plugin), authority.as_ref())
                };
                match installed {
                    Ok(report) => {
                        spinner.finish_with_message("Plugin installed");
                        match format {
                            OutputFormat::Text => {
                                progress::success(&format!(
                                    "Installed plugin: {}",
                                    report.plugin.id
                                ));
                                println!("  Store path: {}", report.plugin.store_path.display());
                                print!("{}", render_add_report(&report, verbose));
                                for publication in &report.publications {
                                    if let Some(error) = &publication.error {
                                        progress::warn(&format!(
                                            "{} could not publish: {error}",
                                            publication.integration
                                        ));
                                    }
                                }
                            }
                            OutputFormat::Json => print_json(&report),
                        }
                    }
                    Err(e) => {
                        spinner.finish_with_message("Failed");
                        progress::error(&format!("Failed to install plugin: {e}"));
                        return Err(e);
                    }
                }
            }
            PluginAction::List { format } => {
                let plugins = app.list_plugins()?;
                match format {
                    OutputFormat::Text => {
                        println!("Plugins");
                        for plugin in plugins {
                            println!("{}  {} capabilities", plugin.id, plugin.capability_count);
                        }
                    }
                    OutputFormat::Json => print_json(&plugins),
                }
            }
            PluginAction::Inspect { plugin, format } => {
                let report = app.inspect_plugin(&plugin)?;
                match format {
                    OutputFormat::Text => print!("{}", render_inspection(&report)),
                    OutputFormat::Json => print_json(&report),
                }
            }
            PluginAction::Remove { plugin, format } => {
                let report = app.remove_plugin(&plugin)?;
                match format {
                    OutputFormat::Text => print!("{}", render_remove(&report)),
                    OutputFormat::Json => print_json(&report),
                }
            }
            PluginAction::Update {
                plugin,
                trust,
                format,
            } => {
                let authority = trust_authority(trust);
                let report = app.update_plugin(&plugin, authority.as_ref())?;
                match format {
                    OutputFormat::Text => print!("{}", render_update(&report)),
                    OutputFormat::Json => print_json(&report),
                }
            }
        },
        Command::Harness { action } => match action {
            HarnessAction::List { format } => {
                let harnesses = app.harness_list();
                match format {
                    OutputFormat::Text => print!("{}", render_harness_list(&harnesses)),
                    OutputFormat::Json => print_json(&harnesses),
                }
            }
            HarnessAction::Inspect { name, format } => {
                let harness = app.harness_inspect(&name)?;
                match format {
                    OutputFormat::Text => print!("{}", render_harness_detail(&harness)),
                    OutputFormat::Json => print_json(&harness),
                }
            }
            HarnessAction::Setup { name } => run_setup(&app, name.as_deref())?,
        },
        Command::Doctor { format } => {
            let spinner = progress::spinner("Running diagnostics...");
            let report = app.doctor();
            spinner.finish_with_message("Diagnostics complete");
            match format {
                OutputFormat::Text => print!("{}", render_doctor(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
        Command::Setup { harness } => run_setup(&app, harness.as_deref())?,
        Command::External(args) => run_shorthand(&app, args, verbose)?,
    }
    Ok(())
}

/// `uze setup [harness]` and `uze harness setup [name]` are the same
/// operation under two spellings — see
/// `specs/harness-namespace/spec.md`'s "Namespaced and root setup are
/// equivalent" scenario. One function, two call sites, so they cannot
/// silently drift apart.
fn run_setup(app: &UzeApplication, harness: Option<&str>) -> Result<()> {
    println!("Provisioning harnesses through official routes…");
    for result in app.setup(harness)? {
        if result.configured {
            println!(
                "{}: ready ({}; version {})",
                result.integration,
                format!("{:?}", result.provisioning.action).to_lowercase(),
                result.detection.version.as_deref().unwrap_or("unknown")
            );
            if let Some(shim) = &result.runtime_shim {
                println!(
                    "  EXPERIMENTAL runtime shim: {} (remove this symlink to turn it back off)",
                    shim.shim_path.display()
                );
                if let Some(rc_file) = &shim.rc_file_updated {
                    println!("  Added it to PATH in {}", rc_file.display());
                }
                if let Some(hint) = &shim.path_hint {
                    println!("  Not yet on PATH in this session — {hint}");
                }
            }
        } else {
            println!(
                "{}: setup {:?}: {}",
                result.integration,
                result.provisioning.status,
                result
                    .provisioning
                    .reason
                    .as_deref()
                    .unwrap_or("executable was not verified")
            );
        }
    }
    Ok(())
}

/// `uze <plugin>@<market>` — the project shorthand. Reached only from
/// `Command::External`; see that variant's doc comment for the precedence
/// argument. Semantically equivalent to `add_project_plugin`: this
/// function does no acquisition/reconciliation logic of its own, only
/// argument classification and rendering — the Application layer owns
/// desired-project-state → `agents.lock` → reconciliation → machine
/// delivery (see `docs/adr/019-...md`).
fn run_shorthand(app: &UzeApplication, args: Vec<String>, verbose: bool) -> Result<()> {
    // `external_subcommand` only ever fires with at least one token: the
    // unrecognized first argument that caused the fallback.
    let first = args[0].clone();
    let (plugin, marketplace) = match uze_core::project_lock::parse_plugin_marketplace_spec(&first)
    {
        Ok(parsed) => parsed,
        // No `@` at all: this was never shorthand — it's an unrecognized
        // command. Reuse `clap`'s own error type/formatting/exit code
        // rather than a hand-rolled message, so this reads exactly like
        // any other "unrecognized subcommand" `clap` produces natively.
        Err(_) if !first.contains('@') => {
            Cli::command()
                .error(
                    ErrorKind::InvalidSubcommand,
                    format!(
                        "unrecognized subcommand '{first}'\n\n  no marketplace given — did you \
                         mean `{first}@<market>`?\n\nFor more information, try '--help'.",
                    ),
                )
                .exit();
        }
        // Contains `@` but fails the shorthand's own validation (e.g. a
        // path segment, an empty half) — a real shorthand-input error, not
        // a missing command, so it flows through the ordinary `uze:
        // {error}` path like every other application error.
        Err(error) => return Err(error),
    };

    // Parses the *rest* of argv through `clap` — not a hand-rolled loop —
    // so an unrecognized flag is rejected with `clap`'s own error and exit
    // code instead of being silently ignored (the exact bug this replaces;
    // see docs/adr/019-...md).
    let shorthand = ShorthandArgs::try_parse_from(std::iter::once("uze".to_owned()).chain(args))
        .unwrap_or_else(|error| error.exit());

    let current_dir = std::env::current_dir().map_err(|source| uze::UzeError::Read {
        path: PathBuf::from("."),
        source,
    })?;
    let authority = trust_authority(shorthand.trust);
    let report = app.add_project_plugin(&plugin, &marketplace, &current_dir, authority.as_ref())?;

    match shorthand.format {
        OutputFormat::Text => {
            println!("Added plugin to project: {plugin}@{marketplace}");
            println!("Store path: {}", report.plugin.store_path.display());
            print!(
                "{}",
                render_add_report(&report, verbose || shorthand.verbose)
            );
        }
        OutputFormat::Json => print_json(&report),
    }
    Ok(())
}

/// `uze context` defaults to the current directory; an explicit path is
/// otherwise used exactly as given.
fn context_path(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| PathBuf::from("."))
}

/// Interprets a direct plugin source: a local path, or a Git URL —
/// `<url>@<ref>` pins a branch, tag or commit, and `#<subdir>` selects a
/// package root inside the repository. Used by `uze plugin install` when
/// its argument is not a `name@marketplace` spec.
fn parse_source(spec: &str) -> uze::PackageSource {
    let looks_remote = spec.starts_with("https://")
        || spec.starts_with("http://")
        || spec.starts_with("git://")
        || spec.starts_with("ssh://")
        || spec.starts_with("file://");
    if !looks_remote {
        return uze::PackageSource::local(spec);
    }
    let (locator, subdirectory) = match spec.split_once('#') {
        Some((locator, subdirectory)) => (locator, Some(PathBuf::from(subdirectory))),
        None => (spec, None),
    };
    // Split on the last `@` only when it follows the authority, so a URL
    // whose path legitimately contains `@` is not mistaken for a pin.
    let scheme_end = locator.find("://").map(|at| at + 3).unwrap_or(0);
    let (url, reference) = match locator[scheme_end..].rfind('@') {
        Some(at) => {
            let at = scheme_end + at;
            (&locator[..at], Some(locator[at + 1..].to_owned()))
        }
        None => (locator, None),
    };
    uze::PackageSource::Git {
        url: url.to_owned(),
        reference,
        subdirectory,
    }
}

/// Chooses who answers a trust question.
///
/// Without `--trust`, an interactive terminal prompts and anything else
/// refuses to answer — a pipeline gets `TRUST_REQUIRED` rather than a silent
/// yes.
fn trust_authority(trusted: bool) -> Box<dyn uze::trust::TrustAuthority> {
    if trusted {
        return Box::new(uze::trust::AlwaysTrust);
    }
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return Box::new(PromptingAuthority);
    }
    Box::new(uze::trust::NoTrustAuthority)
}

struct PromptingAuthority;

impl uze::trust::TrustAuthority for PromptingAuthority {
    fn authorize(&self, request: &uze::trust::TrustRequest) -> uze::trust::TrustOutcome {
        use std::io::Write;

        println!();
        if request.previously_trusted {
            println!(
                "This update introduces an executable capability the installed package did not have"
            );
        } else {
            println!("This package requests an executable capability");
        }
        println!("\nSource\n  {}", request.requested_source);
        if request.resolved_source != request.requested_source {
            println!("\nResolved\n  {}", request.resolved_source);
        }
        println!("\nPackage\n  {}", request.package_id);
        println!("\nMCP");
        for capability in &request.executable {
            println!(
                "  {} → {} {}",
                capability.name,
                capability.command,
                capability.arguments.join(" ")
            );
        }
        print!("\nTrust and install? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            return uze::trust::TrustOutcome::Unavailable;
        }
        match answer.trim() {
            "y" | "Y" | "yes" => uze::trust::TrustOutcome::Granted,
            _ => uze::trust::TrustOutcome::Denied,
        }
    }
}

fn print_json(value: &impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("application report serializable")
    );
}

fn render_inspection(report: &PluginInspection) -> String {
    let mut text = format!(
        "{}\n\nSource\n  {}\n\nCapabilities\n",
        report.plugin.id, report.plugin.source
    );
    for capability in &report.capabilities {
        text.push_str(&format!("  {:?}  {}\n", capability.kind, capability.name));
    }
    text.push_str("\nDelivery\n");
    for delivery in &report.deliveries {
        let package = delivery
            .package_plan
            .as_ref()
            .map_or("decomposed".to_owned(), |plan| format!("{:?}", plan.route));
        text.push_str(&format!(
            "\n{}\n  Package  {package}\n",
            delivery.integration
        ));
        for capability in &delivery.capabilities {
            let status = if capability.provided_by_package {
                "provided by package".to_owned()
            } else {
                capability
                    .plan
                    .as_ref()
                    .map_or("not exposed".to_owned(), |plan| format!("{:?}", plan.route))
            };
            text.push_str(&format!("  {:?}  {status}\n", capability.kind));
        }
    }
    let state = &report.managed_state;
    text.push_str(&format!(
        "\nManaged state\n  {} matched\n  {} missing\n  {} drifted\n  {} conflicts\n  {} blocked\n",
        state.matched, state.missing, state.drifted, state.conflicts, state.blocked
    ));
    if let Some(error) = &state.ledger_error {
        text.push_str(&format!("  ledger blocked: {error}\n"));
    }
    text
}

/// Compact per-harness report for an install/add: one line per harness with
/// its route and — when an attachment was recorded — where. Evidence
/// sentences and full attachment details are `--verbose`-only; `doctor`/
/// `plugin inspect` state the same facts read-only.
fn render_add_report(report: &AddPluginReport, verbose: bool) -> String {
    let mut out = String::new();
    let attachments: BTreeMap<&str, &PathBuf> = report
        .attachments
        .iter()
        .map(|attachment| (attachment.integration.as_str(), &attachment.location))
        .collect();
    for (harness, plan) in &report.package_plans {
        let route = format!("{:?}", plan.route).to_lowercase();
        let attached = attachments
            .get(harness.as_str())
            .map(|location| format!(" ({})", location.display()))
            .unwrap_or_default();
        out.push_str(&format!("  {harness}: {route}{attached}\n"));
        if verbose {
            out.push_str(&format!("    {}\n", plan.evidence));
        }
    }
    // Attachments that are not package delivery (e.g. an Agent Skill
    // symlink in the shared skills root) still belong in the summary.
    for attachment in &report.attachments {
        if !report
            .package_plans
            .iter()
            .any(|(harness, _)| harness == &attachment.integration)
        {
            out.push_str(&format!(
                "  {}: attached at {}\n",
                attachment.integration,
                attachment.location.display()
            ));
        }
    }
    out
}

fn render_update(report: &uze::application::UpdatePluginReport) -> String {
    use uze::application::UpdatePluginReport;
    match report {
        UpdatePluginReport::Updated { plugin, .. } => format!("Updated {}\n", plugin.id),
        UpdatePluginReport::Blocked { report, plan } => format!(
            "Update blocked for {}: {:?}\n{}\n",
            report.package_id,
            plan,
            render_managed_state(
                &report
                    .receipts
                    .iter()
                    .map(|receipt| receipt.inspection.state)
                    .collect::<Vec<_>>(),
            )
        ),
    }
}

fn render_remove(report: &RemovePluginReport) -> String {
    match report {
        RemovePluginReport::AlreadyAbsent { plugin } => {
            format!("No UZE state remains for {plugin}\n")
        }
        RemovePluginReport::Removed { plugin, .. } => format!("Removed {plugin}\n"),
        RemovePluginReport::Blocked { report, plan } => format!(
            "Removal blocked for {}: {:?}\n{}\n",
            report.package_id,
            plan,
            render_managed_state(
                &report
                    .receipts
                    .iter()
                    .map(|receipt| receipt.inspection.state)
                    .collect::<Vec<_>>(),
            )
        ),
    }
}

fn render_install(report: &uze::application::InstallReport) -> String {
    use uze::application::InstallReport;
    match report {
        InstallReport::NoChanges => "Project environment is already up to date.\n".to_owned(),
        InstallReport::Installed { plugins } => {
            let mut text = format!("Installed {} plugin(s):\n", plugins.len());
            for plugin in plugins {
                text.push_str(&format!("  {plugin}\n"));
            }
            text
        }
    }
}

fn render_doctor(report: &DoctorReport) -> String {
    let mut text = format!(
        "UZE Home\n  {}\n\nStore\n  {:?}\n\nPlugins\n  {} installed\n\nHarnesses\n",
        report.uze_home.display(),
        report.store,
        report.plugins.len()
    );
    for harness in &report.harnesses {
        text.push_str(&format!(
            "  {}  detected: {}  setup: {}\n",
            harness.integration, harness.detection.present, harness.setup
        ));
        if let Some(provisioning) = &harness.provisioning {
            text.push_str(&format!(
                "    provisioning: {:?} via {} ({:?})\n",
                provisioning.status, provisioning.method, provisioning.action
            ));
        }
        if let uze::integration::PublicationStatus::Unpublished(reason) = &harness.publication {
            text.push_str(&format!("    package view not published: {reason}\n"));
        }
    }
    text.push_str("\nAttachments\n");
    for attachment in &report.attachments {
        let state = &attachment.state;
        text.push_str(&format!(
            "  {}  {} matched, {} missing, {} drifted, {} conflicts, {} blocked\n",
            attachment.plugin,
            state.matched,
            state.missing,
            state.drifted,
            state.conflicts,
            state.blocked
        ));
    }
    if let Some(error) = &report.ledger_error {
        text.push_str(&format!("\nLedger\n  blocked: {error}\n"));
    }
    if let Some(error) = &report.integration_state_error {
        text.push_str(&format!("\nIntegration state\n  blocked: {error}\n"));
    }
    if let Some(error) = &report.provisioning_state_error {
        text.push_str(&format!("\nProvisioning state\n  blocked: {error}\n"));
    }
    text
}

fn render_market_list(marketplaces: &[MarketplaceSummary]) -> String {
    let mut text = "Marketplaces\n".to_owned();
    for market in marketplaces {
        text.push_str(&format!(
            "{}  {}  {}\n",
            market.name, market.source, market.plugin_count
        ));
    }
    text
}

fn render_market_detail(detail: &MarketplaceSummary) -> String {
    format!(
        "{}\n\nSource\n  {}\n\nPlugins\n  {}\n",
        detail.name, detail.source, detail.plugin_count
    )
}

fn render_harness_list(harnesses: &[HarnessHealth]) -> String {
    let mut text = "Harnesses\n".to_owned();
    for harness in harnesses {
        text.push_str(&format!(
            "  {}  detected: {}  setup: {}\n",
            harness.integration, harness.detection.present, harness.setup
        ));
    }
    text
}

fn render_harness_detail(harness: &HarnessHealth) -> String {
    let mut text = format!(
        "{}\n\nDetection\n  present: {}\n  version: {}\n\nSetup\n  {}\n",
        harness.display_name,
        harness.detection.present,
        harness.detection.version.as_deref().unwrap_or("unknown"),
        harness.setup
    );
    if let Some(provisioning) = &harness.provisioning {
        text.push_str(&format!(
            "\nProvisioning\n  {:?} via {} ({:?})\n",
            provisioning.status, provisioning.method, provisioning.action
        ));
    }
    text
}

fn render_status(report: &StatusReport) -> String {
    let mut text = format!(
        "Project\n  Context       {}\n\nHarnesses\n",
        render_portability(&report.portability)
    );
    for harness in &report.harnesses {
        let delivery = match &harness.delivery {
            HarnessContextDelivery::Native => "native".to_owned(),
            HarnessContextDelivery::NotDetected => "not installed".to_owned(),
            HarnessContextDelivery::Bridge { state, .. } => format!("bridged ({state:?})"),
        };
        text.push_str(&format!("  {}  {delivery}\n", harness.integration));
    }
    text.push_str(&format!(
        "\nPackages\n  {} installed\n  {} contributing here\n",
        report.packages_installed, report.packages_contributing_here
    ));
    text.push_str(&render_project_lock_status(&report.project_lock));
    if report.issues.is_empty() {
        text.push_str("\nHealth\n  no issues\n");
    } else {
        text.push_str("\nHealth\n");
        for issue in &report.issues {
            text.push_str(&format!("  {issue}\n"));
        }
    }
    text
}

fn render_project_lock_status(status: &uze::application::ProjectLockStatus) -> String {
    use uze::application::ProjectLockStatus;
    match status {
        ProjectLockStatus::Absent => String::new(),
        ProjectLockStatus::Malformed { reason } => {
            format!("\nProject lock\n  agents.lock is malformed: {reason}\n")
        }
        ProjectLockStatus::Present { plugins } => {
            let mut text = "\nProject lock\n".to_owned();
            if plugins.is_empty() {
                text.push_str("  agents.lock has no plugins\n");
            }
            for plugin in plugins {
                let state = if plugin.installed {
                    "installed"
                } else {
                    "missing (run `uze install`)"
                };
                text.push_str(&format!("  {}  {state}\n", plugin.plugin));
            }
            text
        }
    }
}

fn render_context_status(status: &ProjectContextStatus) -> String {
    let mut text = format!("Context for {}\n\nSources\n", status.canonical.display());
    for source in &status.sources {
        if !source.exists {
            text.push_str(&format!("  {}  absent\n", source.file_name));
            continue;
        }
        text.push_str(&format!(
            "  {}  {} managed region(s), user content: {}\n",
            source.file_name,
            source.managed_region_identities.len(),
            if source.has_user_content { "yes" } else { "no" }
        ));
    }
    if !status.contributions.is_empty()
        || !status.orphaned_regions.is_empty()
        || !status.malformed_regions.is_empty()
    {
        text.push_str("\nContributions\n");
        for contribution in &status.contributions {
            text.push_str(&format!(
                "  {}  {:?}\n",
                contribution.package_id, contribution.state
            ));
        }
        for orphan in &status.orphaned_regions {
            text.push_str(&format!(
                "  {orphan}  ORPHANED (no installed package claims it)\n"
            ));
        }
        for malformed in &status.malformed_regions {
            text.push_str(&format!(
                "  {malformed}  MALFORMED (markers cannot be trusted)\n"
            ));
        }
    }
    text.push_str("\nHarnesses\n");
    for harness in &status.harnesses {
        let delivery = match &harness.delivery {
            HarnessContextDelivery::Native => "native".to_owned(),
            HarnessContextDelivery::NotDetected => "not detected".to_owned(),
            HarnessContextDelivery::Bridge { needed, state } => {
                format!(
                    "bridge {:?}{}",
                    state,
                    if *needed {
                        ""
                    } else {
                        " (not currently needed)"
                    }
                )
            }
        };
        text.push_str(&format!("  {}  {delivery}\n", harness.integration));
    }
    text.push_str(&format!(
        "\nPortability: {}\n",
        render_portability(&status.portability)
    ));
    if !status.warnings.is_empty() {
        text.push_str("\nWarnings\n");
        for warning in &status.warnings {
            text.push_str(&format!("  {warning}\n"));
        }
    }
    text
}

fn render_portability(portability: &Portability) -> String {
    match portability {
        Portability::NoContext => "NO_CONTEXT (no recognized instructions file exists)".to_owned(),
        Portability::Portable => "PORTABLE".to_owned(),
        Portability::PartiallyPortable { gaps } => {
            format!("PARTIALLY_PORTABLE ({})", gaps.join("; "))
        }
        Portability::VendorLocked { files } => format!(
            "VENDOR_LOCKED ({})",
            files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_action(action: &PlannedAction) -> String {
    match action {
        PlannedAction::Attach => "ATTACH".to_owned(),
        PlannedAction::NoChange => "NO_CHANGE".to_owned(),
        PlannedAction::Remove => "REMOVE".to_owned(),
        PlannedAction::Blocked(reason) => format!("BLOCKED ({reason})"),
    }
}

fn render_context_plan(plan: &ContextPlan) -> String {
    let mut text = format!("Plan for {}\n", plan.agents_md.display());
    for contribution in &plan.agents_md_plan.contributions {
        text.push_str(&format!(
            "  {}  {}\n",
            contribution.package_id.as_str(),
            render_action(&contribution.action)
        ));
    }
    for orphan in &plan.agents_md_plan.orphans {
        text.push_str(&format!(
            "  {}  {}\n",
            orphan.region_identity,
            render_action(&orphan.action)
        ));
    }
    if !plan.bridges.is_empty() {
        text.push_str("\nBridges\n");
        for bridge in &plan.bridges {
            text.push_str(&format!(
                "  {}  {}  {}\n",
                bridge.integration,
                bridge.file.display(),
                render_action(&bridge.action)
            ));
        }
    }
    if plan.has_changes() {
        text.push_str("\nRun `uze context reconcile` to apply.\n");
    } else {
        text.push_str("\nNo changes: context is already reconciled.\n");
    }
    text
}

fn render_context_reconciliation(report: &ContextReconciliationReport) -> String {
    let mut text = format!("Reconciled {}\n\n", report.agents_md.display());
    for package in &report.packages {
        text.push_str(&format!("  {}  {:?}\n", package.package_id, package.state));
    }
    for orphan in &report.removed_orphans {
        text.push_str(&format!("  {orphan}  REMOVED (orphaned)\n"));
    }
    for (orphan, reason) in &report.blocked_orphans {
        text.push_str(&format!("  {orphan}  BLOCKED: {reason}\n"));
    }
    if !report.bridges.is_empty() {
        text.push_str("\nBridges\n");
        for bridge in &report.bridges {
            text.push_str(&format!(
                "  {}  {}  {:?}\n",
                bridge.integration,
                bridge.file.display(),
                bridge.state
            ));
        }
    }
    text
}

fn render_managed_state(states: &[uze::integration::AttachmentState]) -> String {
    let mut matched = 0;
    let mut missing = 0;
    let mut drifted = 0;
    let mut conflict = 0;
    let mut blocked = 0;
    for state in states {
        match state {
            uze::integration::AttachmentState::Matched => matched += 1,
            uze::integration::AttachmentState::Missing => missing += 1,
            uze::integration::AttachmentState::Drifted => drifted += 1,
            uze::integration::AttachmentState::Conflict => conflict += 1,
            uze::integration::AttachmentState::Blocked => blocked += 1,
        }
    }
    format!(
        "{matched} matched, {missing} missing, {drifted} drifted, {conflict} conflicts, {blocked} blocked"
    )
}

/// The machine-checkable half of ADR-019's soundness argument for
/// `<plugin>@<market>` shorthand dispatch (`docs/adr/019-explicit-project-
/// machine-boundary-in-cli-command-grammar.md`): a first argument is
/// classified as shorthand iff it contains `@`, which is sound only as
/// long as no built-in command name — at any nesting level — ever
/// contains `@` itself. This walks `clap`'s own generated command tree
/// (not a hand-maintained list) so the guarantee cannot silently rot as
/// commands are added.
#[cfg(test)]
mod grammar_tests {
    use clap::CommandFactory;

    use super::Cli;

    fn leaf_and_group_names(command: &clap::Command, out: &mut Vec<String>) {
        for sub in command.get_subcommands() {
            out.push(sub.get_name().to_owned());
            leaf_and_group_names(sub, out);
        }
    }

    #[test]
    fn no_command_or_subcommand_name_anywhere_in_the_tree_contains_at() {
        let mut names = Vec::new();
        leaf_and_group_names(&Cli::command(), &mut names);
        assert!(
            !names.is_empty(),
            "sanity: the command tree must not be empty"
        );
        let offenders: Vec<&String> = names.iter().filter(|name| name.contains('@')).collect();
        assert!(
            offenders.is_empty(),
            "a built-in command name contains '@', which would make it \
             ambiguous with <plugin>@<market> shorthand: {offenders:?}"
        );
    }

    /// `command_performance::current_leaf_commands` already proves this
    /// indirectly (its own exhaustiveness test would fail if `external`
    /// leaked in as a real leaf), but this asserts it directly: the
    /// shorthand's `External` variant must never surface as a named,
    /// discoverable subcommand — it exists only as `clap`'s fallback.
    #[test]
    fn external_subcommand_is_not_a_named_leaf() {
        let mut names = Vec::new();
        leaf_and_group_names(&Cli::command(), &mut names);
        assert!(
            !names.iter().any(|name| name == "external"),
            "the shorthand fallback must never appear as a discoverable command name"
        );
    }
}
