//! Thin CLI presentation over `UzeApplication`.

mod shim;

use std::{io::IsTerminal, path::PathBuf};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use uze::{
    Result, UzeApplication, UzeHome,
    application::{
        ContextPlan, ContextReconciliationReport, DoctorReport, HarnessContextDelivery,
        PluginInspection, Portability, ProjectContextStatus, RemovePluginReport, StatusReport,
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
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Install a local plugin package
    Add {
        source: String,
        /// Authorize executable capabilities
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// List installed plugins
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Inspect a plugin's delivery
    Inspect {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Remove a plugin
    Remove {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Update a plugin to latest version
    Update {
        plugin: String,
        /// Authorize new executable capabilities
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Install project environment from agents.lock
    Install {
        path: Option<PathBuf>,
        /// Authorize executable capabilities
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Show project health status
    Status {
        path: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Manage project context (AGENTS.md)
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },
    /// Manage marketplace sources
    Marketplace {
        #[command(subcommand)]
        action: MarketplaceAction,
    },
    /// Manage plugins via marketplace
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Run diagnostics
    Doctor {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Setup harness integrations
    Setup { harness: Option<String> },
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
enum MarketplaceAction {
    /// Add a marketplace discovery source (local path or https://...).
    Add { source: String },
    /// List registered marketplaces (including embedded uze-official).
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Remove a marketplace (blocked while plugins from it are installed).
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
enum PluginAction {
    /// Install a plugin via `name@marketplace`.
    Install {
        plugin: String,
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// List installed plugins (with marketplace column).
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Remove a plugin.
    Remove {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Update a plugin (re-resolve marketplace source).
    Update {
        plugin: String,
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
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

    // Check for --help flag and render custom colored help
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        print_colored_help();
        return;
    }

    // Check for project shorthand `uze <plugin>@<marketplace>` before clap parsing.
    // If the first argument contains `@` and is not a known subcommand, treat it as shorthand.
    if args.len() >= 2 {
        let first_arg = &args[1];
        if first_arg.contains('@') && !first_arg.starts_with('-') {
            // This looks like `uze flow@ai` shorthand.
            // Parse it and convert to a synthetic Cli command.
            if let Err(error) = run_project_shorthand(&args[1..]) {
                eprintln!("uze: {error}");
                std::process::exit(1);
            }
            return;
        }
    }

    if let Err(error) = run(Cli::parse()) {
        eprintln!("uze: {error}");
        std::process::exit(1);
    }
}

fn print_colored_help() {
    let cyan = "\x1b[36m";
    let green = "\x1b[32m";
    let yellow = "\x1b[33m";
    let reset = "\x1b[0m";
    let bold = "\x1b[1m";

    println!("{}UZE{} - Agent Plugin Environment Manager", bold, reset);
    println!();
    println!("{}Usage:{} uze <command> [options]", bold, reset);
    println!();
    println!("{}Package Management:{}", bold, reset);
    println!(
        "  {}add{}        Install a local plugin package",
        cyan, reset
    );
    println!("  {}list{}       List installed plugins", cyan, reset);
    println!("  {}inspect{}    Inspect a plugin's delivery", cyan, reset);
    println!("  {}remove{}     Remove a plugin", cyan, reset);
    println!(
        "  {}update{}     Update a plugin to latest version",
        cyan, reset
    );
    println!();
    println!("{}Project Environment:{}", bold, reset);
    println!(
        "  {}install{}    Install project environment from agents.lock",
        cyan, reset
    );
    println!("  {}status{}     Show project health status", cyan, reset);
    println!(
        "  {}context{}    Manage project context (AGENTS.md)",
        cyan, reset
    );
    println!();
    println!("{}Marketplace:{}", bold, reset);
    println!("  {}marketplace{} Manage marketplace sources", cyan, reset);
    println!(
        "  {}plugin{}     Manage plugins via marketplace",
        cyan, reset
    );
    println!();
    println!("{}Diagnostics:{}", bold, reset);
    println!("  {}doctor{}     Run diagnostics", cyan, reset);
    println!("  {}setup{}      Setup harness integrations", cyan, reset);
    println!();
    println!("{}Options:{}", bold, reset);
    println!("  {}-h, --help{}     Print help", green, reset);
    println!("  {}-V, --version{}  Print version", green, reset);
    println!();
    println!("{}Quick Start:{}", bold, reset);
    println!(
        "  {}uze install{}         Install project environment",
        yellow, reset
    );
    println!(
        "  {}uze doctor{}          Check system health",
        yellow, reset
    );
    println!(
        "  {}uze marketplace add{} Add a marketplace source",
        yellow, reset
    );
}

fn run_project_shorthand(args: &[String]) -> Result<()> {
    // Parse `plugin@marketplace [--trust] [--format text|json]`
    let spec = &args[0];
    let (plugin, marketplace) = uze_core::project_lock::parse_plugin_marketplace_spec(spec)?;

    let mut trust = false;
    let mut format = OutputFormat::Text;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--trust" => trust = true,
            "--format" => {
                i += 1;
                if i < args.len() {
                    format = match args[i].as_str() {
                        "json" => OutputFormat::Json,
                        _ => OutputFormat::Text,
                    };
                }
            }
            _ => {}
        }
        i += 1;
    }

    let home = UzeHome::from_env()?;
    let app = UzeApplication::from_env(home)?;
    let _ = app.ensure_default_plugins();

    let authority = trust_authority(trust);
    let report = app.add_project_plugin(
        &plugin,
        &marketplace,
        &PathBuf::from("."),
        authority.as_ref(),
    )?;

    match format {
        OutputFormat::Text => {
            println!("Added plugin to project: {plugin}@{marketplace}");
            println!("Store path: {}", report.plugin.store_path.display());
            for (harness, plan) in &report.package_plans {
                println!(
                    "Package delivery to {harness}: {:?} ({} components)",
                    plan.route,
                    plan.provided_resource_identities.len()
                );
            }
        }
        OutputFormat::Json => print_json(&report),
    }
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    let home = UzeHome::from_env()?;
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
    // `uze add` and heals its attachment after a binary update. Best-effort:
    // failures here must not block `doctor`/`list` etc.
    let _ = app.ensure_default_plugins();
    match command {
        Command::Add {
            source,
            trust,
            format,
        } => {
            let authority = trust_authority(trust);
            let report = app.add_plugin(parse_source(&source), authority.as_ref())?;
            match format {
                OutputFormat::Text => {
                    println!("Installed plugin: {}", report.plugin.id);
                    println!("Store path: {}", report.plugin.store_path.display());
                    for (harness, plan) in &report.package_plans {
                        println!(
                            "Package delivery to {harness}: {:?} ({} components)",
                            plan.route,
                            plan.provided_resource_identities.len()
                        );
                    }
                    for attachment in &report.attachments {
                        println!(
                            "Attached to {}: {}",
                            attachment.integration,
                            attachment.location.display()
                        );
                    }
                    for publication in &report.publications {
                        if let Some(error) = &publication.error {
                            println!(
                                "Warning: {} could not publish its package view: {error}\n  \
                                 The package is installed. Re-run `uze setup {}` to rebuild it.",
                                publication.integration, publication.integration
                            );
                        }
                    }
                }
                OutputFormat::Json => print_json(&report),
            }
        }
        Command::List { format } => {
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
        Command::Inspect { plugin, format } => {
            let report = app.inspect_plugin(&plugin)?;
            match format {
                OutputFormat::Text => print!("{}", render_inspection(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
        Command::Setup { harness } => {
            println!("Provisioning harnesses through official routes…");
            for result in app.setup(harness.as_deref())? {
                if result.configured {
                    println!(
                        "{}: ready ({}; version {})",
                        result.integration,
                        format!("{:?}", result.provisioning.action).to_lowercase(),
                        result.detection.version.as_deref().unwrap_or("unknown")
                    );
                    if let Some(shim) = &result.runtime_shim {
                        println!(
                            "  EXPERIMENTAL runtime shim: {} (remove this symlink to turn it \
                             back off)",
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
        }
        Command::Remove { plugin, format } => {
            // Try to remove from project first (if agents.lock exists)
            let current_dir = std::env::current_dir().map_err(|e| uze::UzeError::Read {
                path: std::path::PathBuf::from("."),
                source: e,
            })?;
            let project_report = app.remove_project_plugin(&plugin, &current_dir)?;

            match project_report {
                uze::application::RemoveProjectPluginReport::Removed { .. } => {
                    // Successfully removed from project lock
                    match format {
                        OutputFormat::Text => println!("Removed {plugin} from project"),
                        OutputFormat::Json => print_json(&project_report),
                    }
                }
                uze::application::RemoveProjectPluginReport::NoLock
                | uze::application::RemoveProjectPluginReport::NotInLock { .. } => {
                    // No lock or plugin not in lock, fallback to global remove
                    let report = app.remove_plugin(&plugin)?;
                    match format {
                        OutputFormat::Text => print!("{}", render_remove(&report)),
                        OutputFormat::Json => print_json(&report),
                    }
                }
            }
        }
        Command::Update {
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
        Command::Doctor { format } => {
            let report = app.doctor();
            match format {
                OutputFormat::Text => print!("{}", render_doctor(&report)),
                OutputFormat::Json => print_json(&report),
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
        Command::Marketplace { action } => match action {
            MarketplaceAction::Add { source } => {
                app.marketplace_add(&source)?;
                println!("Added marketplace from {source}");
            }
            MarketplaceAction::List { format } => {
                let mps = app.marketplace_list()?;
                match format {
                    OutputFormat::Text => {
                        println!("Marketplaces");
                        for mp in mps {
                            println!("{}  {}  {}", mp.name, mp.source, mp.plugin_count);
                        }
                    }
                    OutputFormat::Json => print_json(&mps),
                }
            }
            MarketplaceAction::Remove { name } => {
                app.marketplace_remove(&name)?;
                println!("Removed marketplace {name}");
            }
        },
        Command::Plugin { action } => match action {
            PluginAction::Install {
                plugin,
                trust,
                format,
            } => {
                let authority = trust_authority(trust);
                let report = app.plugin_install(&plugin, authority.as_ref())?;
                match format {
                    OutputFormat::Text => {
                        println!("Installed plugin: {}", report.plugin.id);
                        println!("Store path: {}", report.plugin.store_path.display());
                    }
                    OutputFormat::Json => print_json(&report),
                }
            }
            PluginAction::List { format } => {
                let plugins = app.list_plugins()?;
                match format {
                    OutputFormat::Text => {
                        println!("Plugins");
                        for p in plugins {
                            println!("{}  {}", p.id, p.capability_count);
                        }
                    }
                    OutputFormat::Json => print_json(&plugins),
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
        Command::Install {
            path,
            trust,
            format,
        } => {
            let authority = trust_authority(trust);
            let report =
                app.install_project_environment(&context_path(path), authority.as_ref())?;
            match format {
                OutputFormat::Text => print!("{}", render_install(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
    }
    Ok(())
}

/// `uze context` defaults to the current directory; an explicit path is
/// otherwise used exactly as given.
fn context_path(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| PathBuf::from("."))
}

/// Interprets an `uze add` argument.
///
/// A path is a path and a URL is a URL; the distinction is the mechanism, not
/// the host, so nothing here knows what a GitHub is. `<url>@<ref>` pins a
/// branch, tag or commit, and `#<subdir>` selects a package root inside the
/// repository.
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
        InstallReport::NotImplemented => {
            "Project environment install is not yet fully implemented.\n".to_owned()
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
