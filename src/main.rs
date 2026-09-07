//! Thin CLI presentation over `UzeApplication`.

// Test-only tooling (see command_performance.rs's module doc): a registry
// plus exhaustiveness tests, never wired into runtime command dispatch.
#[cfg(test)]
mod command_performance;
mod progress;
use crate::progress::Colorize;
mod prompt;
mod shim;

use std::{collections::BTreeMap, io::IsTerminal, path::Path, path::PathBuf};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use uze_application::{HookEffect, HookEvent, PlannedAction, Result, UzeHome};
use uze_application::{
    UzeApplication,
    application::{
        AddPluginReport, ContextPlan, ContextReconciliationReport, DoctorReport,
        HarnessContextDelivery, HarnessHealth, MarketplaceSummary, PluginInspection, Portability,
        ProjectContextStatus, RemovePluginReport, RemoveProjectPluginReport, StatusReport,
    },
};

#[derive(Debug, Parser)]
#[command(
    name = "uze",
    version,
    about = "Manage one local agent plugin environment",
    styles = progress::clap_styles()
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
    /// Resolve agents.yaml into agents.lock, then install what it records
    #[command(visible_alias = "i")]
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
    /// Choose what UZE looks like (machine-level)
    Theme {
        #[command(subcommand)]
        action: ThemeAction,
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
    /// Run diagnostics
    Doctor {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Provision harness integrations, or inspect their readiness
    Setup {
        /// Harness ids to provision. Omit to provision every registered harness.
        #[arg(value_name = "HARNESS", num_args = 0..)]
        arguments: Vec<String>,
    },
    /// Experimental persistent local terminal workspace
    Terminal {
        #[command(subcommand)]
        action: TerminalAction,
    },
    /// The agent's own surface: what an agent UZE launched calls to take
    /// part in the workflow it is inside. Hidden from this help on
    /// purpose — its audience reads the instruction text UZE projects into
    /// the project, not `uze --help`.
    #[command(hide = true)]
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Internal runtime dispatch: runs a package's hook commands for one
    /// hook event (ADR-033). Harness integrations emit invocations of this
    /// exact form into managed hook configuration; it is not for
    /// interactive use and is hidden from help.
    #[command(hide = true)]
    HookExec {
        /// Hook adapter id, as registered by the integration registry
        #[arg(long)]
        adapter: String,
        /// ABI event name: pre_tool_use | post_tool_use | stop
        #[arg(long)]
        event: String,
        /// Declared group effect: observe | allow | ask | deny | transform
        #[arg(long)]
        effect: String,
        /// Canonical package root the handlers run in
        #[arg(long)]
        plugin_root: PathBuf,
        /// Authored handler command, repeatable for sequential handlers
        #[arg(long = "command", required = true)]
        commands: Vec<String>,
    },
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
enum AgentAction {
    /// The task this agent is working in
    Task {
        #[command(subcommand)]
        action: AgentTaskAction,
    },
}

#[derive(Debug, Subcommand)]
enum AgentTaskAction {
    /// Name the work being done, as `<type>/<subject>`
    Name {
        /// The proposed name, judged against the project's vocabulary
        name: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum TerminalAction {
    /// Attach the workspace client, starting its local server when needed
    Attach,
    /// Stop this workspace's persistent terminal session
    Stop,
    /// Local server entry point; started only by the workspace runtime
    #[command(hide = true)]
    Serve {
        #[arg(long)]
        root: PathBuf,
    },
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
enum ThemeAction {
    /// List the themes this machine can draw with, marking the active one.
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Draw in this theme, from now on, in both the CLI and the TUI.
    Use { id: String },
    /// Show a theme's resolved colours and glyphs, and anything its file
    /// got wrong. The active one by default.
    Show {
        id: Option<String>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum PluginAction {
    /// Install a plugin on this machine from a marketplace that must have
    /// been added first (`uze market add <market>`), as `name@marketplace`.
    /// A direct path or Git URL is never accepted — never touches the
    /// current project's `agents.lock` (use `uze <plugin>@<market>` for
    /// that).
    Install {
        plugin: String,
        #[arg(long)]
        trust: bool,
        /// If `plugin`'s bare name is already active from a different
        /// marketplace, install this one under `NAME` instead, so both stay
        /// active side by side. Conflicts with `--replace`.
        #[arg(long, conflicts_with = "replace")]
        alias: Option<String>,
        /// If `plugin`'s bare name is already active from a different
        /// marketplace, remove that one first (once safe to) and let this
        /// install claim the name. Conflicts with `--alias`.
        #[arg(long)]
        replace: bool,
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
    // invoked as `claude`/`codex`/`opencode` (via the symlink
    // `ensure_runtime_shim` creates at `~/.uze/shims/<name>`) never reaches
    // the ordinary `uze` subcommand grammar at all. `shim::run` diverges —
    // it always either `exec`s the real binary or exits.
    if let Some(name) = shim::detect() {
        shim::run(&name);
    }

    // Help is presentation-only, but every public command routes through the
    // same renderer before Clap can emit its unstyled generated help.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().skip(1).any(|argument| argument == "-help") {
        Cli::command()
            .error(
                ErrorKind::UnknownArgument,
                "`-help` is not supported; use `help`, `--help`, or `-h`",
            )
            .exit();
    }
    if let Some(topic) = help_topic(&args[1..]) {
        print_help(topic);
        return;
    }

    if let Err(error) = run(Cli::parse()) {
        eprintln!("uze: {error}");
        std::process::exit(1);
    }
}

#[derive(Clone, Copy)]
enum HelpTopic {
    Root,
    Install,
    Remove,
    Status,
    Context,
    Market,
    Plugin,
    Setup,
    Doctor,
}

fn help_topic(arguments: &[String]) -> Option<HelpTopic> {
    let (path, requested) = match arguments {
        [command] if command == "help" || command == "--help" || command == "-h" => (&[][..], true),
        [command, path @ ..] if command == "help" && path.len() <= 1 => (path, true),
        [path @ .., command] if command == "help" || command == "--help" || command == "-h" => {
            (path, true)
        }
        _ => (&[][..], false),
    };
    if !requested {
        return None;
    }
    match path {
        [] => Some(HelpTopic::Root),
        [command, ..] if command == "install" => Some(HelpTopic::Install),
        [command, ..] if command == "remove" => Some(HelpTopic::Remove),
        [command, ..] if command == "status" => Some(HelpTopic::Status),
        [command, ..] if command == "context" => Some(HelpTopic::Context),
        [command, ..] if command == "market" => Some(HelpTopic::Market),
        [command, ..] if command == "plugin" => Some(HelpTopic::Plugin),
        [command, ..] if command == "setup" => Some(HelpTopic::Setup),
        [command, ..] if command == "doctor" => Some(HelpTopic::Doctor),
        _ => None,
    }
}

fn print_help(topic: HelpTopic) {
    match topic {
        HelpTopic::Root => print_root_help(),
        HelpTopic::Install => print_command_help(
            "UZE install",
            "Resolve agents.yaml into agents.lock, then install what it records.",
            "uze install [path] [--trust]",
            &[],
        ),
        HelpTopic::Remove => print_command_help(
            "UZE remove",
            "Remove a plugin from this project without touching the machine store.",
            "uze remove <plugin>",
            &[],
        ),
        HelpTopic::Status => print_command_help(
            "UZE status",
            "Show this project's environment readiness.",
            "uze status [path]",
            &[],
        ),
        HelpTopic::Context => print_command_help(
            "UZE context",
            "Inspect and reconcile this project's AGENTS.md context.",
            "uze context <command>",
            &[
                ("inspect", "Read the current context without writing"),
                ("plan", "Preview reconciliation without writing"),
                ("reconcile", "Apply the project context plan"),
            ],
        ),
        HelpTopic::Market => print_command_help(
            "UZE market",
            "Manage marketplace sources installed on this machine.",
            "uze market <command>",
            &[
                ("add <source>", "Register a marketplace source"),
                ("list", "List registered marketplaces"),
                ("inspect <name>", "Show a marketplace's details"),
                ("remove <name>", "Remove a marketplace source"),
            ],
        ),
        HelpTopic::Plugin => print_command_help(
            "UZE plugin",
            "Manage plugins installed on this machine.",
            "uze plugin <command>",
            &[
                ("install <name@market>", "Install a plugin"),
                ("list", "List installed plugins"),
                ("inspect <name>", "Show plugin delivery details"),
                ("update <name>", "Update an installed plugin"),
                ("remove <name>", "Remove a plugin from this machine"),
            ],
        ),
        HelpTopic::Setup => print_setup_help(),
        HelpTopic::Doctor => print_command_help(
            "UZE doctor",
            "Run machine diagnostics for the UZE store and integrations.",
            "uze doctor",
            &[],
        ),
    }
}

fn print_root_help() {
    let version = env!("CARGO_PKG_VERSION");
    let desc = "Agent environment manager";
    // Center within the commands block width (indent 2 + cmd 12 + gap 2 + longest desc ~44 = 60)
    const CW: usize = 60;
    let center = |s: &str| {
        let len = s.chars().count();
        if len >= CW {
            s.to_string()
        } else {
            let left = (CW - len) / 2;
            format!("{}{}", " ".repeat(left), s)
        }
    };
    println!("{}", progress::title(center("UZE")));
    println!("{}", progress::label(center(&format!("v{version}"))));
    println!("{}", progress::label(center(desc)));
    println!();
    println!("{}", progress::section("Usage"));
    println!("  uze <plugin>@<market>");
    println!("  uze <command> [options]");
    println!();
    // One shared table across both groups: they're both plain command
    // lists, so they must land in the same gutter even though they're
    // printed under separate headings.
    let [project_rows, machine_rows] = progress::aligned_groups(vec![
        vec![
            vec![
                progress::accent("install"),
                "Install this project's environment from agents.yaml".to_owned(),
            ],
            vec![
                progress::accent("remove"),
                "Remove a plugin from this project".to_owned(),
            ],
            vec![
                progress::accent("status"),
                "Show this project's environment status".to_owned(),
            ],
            vec![
                progress::accent("context"),
                "Manage this project's AGENTS.md context".to_owned(),
            ],
        ],
        vec![
            vec![
                progress::accent("market"),
                "Manage marketplace sources".to_owned(),
            ],
            vec![
                progress::accent("plugin"),
                "Manage plugins installed on this machine".to_owned(),
            ],
            vec![
                progress::accent("setup"),
                "Provision or inspect harness integrations".to_owned(),
            ],
            vec![progress::accent("doctor"), "Run diagnostics".to_owned()],
        ],
    ])
    .try_into()
    .expect("aligned_groups preserves the number of groups passed in");
    println!("{}", progress::section("Project:"));
    println!("{project_rows}");
    println!();
    println!("{}", progress::section("Machine:"));
    println!("{machine_rows}");
    println!();
    println!("{}", progress::section("Options"));
    println!(
        "{}",
        progress::aligned_rows(vec![
            vec![
                progress::success_text("-h, --help"),
                "Print help".to_owned(),
            ],
            vec![
                progress::success_text("-V, --version"),
                "Print version".to_owned(),
            ],
        ])
    );
}

fn print_command_help(title: &str, description: &str, usage: &str, commands: &[(&str, &str)]) {
    println!("{}", progress::title(title));
    println!("{}", progress::label(description));
    println!();
    println!("{}", progress::section("Usage"));
    println!("  {usage}");
    if !commands.is_empty() {
        println!();
        println!("{}", progress::section("Commands"));
        println!(
            "{}",
            progress::aligned_rows(
                commands
                    .iter()
                    .map(|(command, description)| vec![
                        progress::accent(command),
                        description.to_string()
                    ])
                    .collect()
            )
        );
    }
    println!();
    println!("{}", progress::section("Options"));
    println!(
        "{}",
        progress::aligned_rows(vec![
            vec![
                progress::success_text("help, --help, -h"),
                "Show this help".to_owned(),
            ],
            vec![
                progress::success_text("--verbose"),
                "Show delivery evidence".to_owned(),
            ],
        ])
    );
}

fn run(cli: Cli) -> Result<()> {
    let home = UzeHome::from_env()?;
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // The TUI owns the terminal, so its trace goes to a file; everything
    // else may write to stderr like any other diagnostic.
    let opens_the_tui = cli.command.is_none()
        && std::env::var_os("UZE_PANE").is_none()
        && std::io::stdout().is_terminal()
        && std::io::stdin().is_terminal();
    let sink = if opens_the_tui {
        uze::telemetry::Sink::File(home.state_dir().join("logs").join("uze.log"))
    } else {
        uze::telemetry::Sink::Stderr
    };
    let _telemetry = uze::telemetry::init(sink);
    let span = uze::telemetry::command_span(&leaf_command_of(&argv), &argv);
    // A `uze` started by a harness the shim launched — a hook it fired, or
    // an agent running `uze` itself — continues the launch's trace.
    uze::telemetry::adopt_parent_from_env(&span);
    let _entered = span.enter();
    let result = dispatch(cli, home);
    if let Err(error) = &result {
        tracing::error!(error = %error, "command failed");
    }
    result
}

/// The leaf command path `argv` names, spelled the way a person types it
/// (`context inspect`); the first argument when it names no subcommand
/// (a `plugin@marketplace` shorthand), and `tui` when there is none.
/// Parsed again from the grammar rather than derived from `Cli`'s
/// variants, so a renamed subcommand renames its span with it.
fn leaf_command_of(argv: &[String]) -> String {
    let parsed = Cli::command()
        .try_get_matches_from(std::iter::once("uze".to_owned()).chain(argv.iter().cloned()))
        .ok();
    let mut path = Vec::new();
    let mut current = parsed.as_ref();
    while let Some((name, matches)) = current.and_then(clap::ArgMatches::subcommand) {
        path.push(name.to_owned());
        current = Some(matches);
    }
    if !path.is_empty() {
        return path.join(" ");
    }
    argv.iter()
        .find(|argument| !argument.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| "tui".to_owned())
}

fn dispatch(cli: Cli, home: UzeHome) -> Result<()> {
    // Before anything is drawn or printed, so the CLI's first line and the
    // TUI's first frame are already in the operator's theme. A theme that
    // will not load reports itself here and is otherwise ignored — see
    // `theme::install`.
    for problem in uze::theme::install(&home) {
        eprintln!("uze: {problem}");
    }
    let verbose = cli.verbose;
    let Some(command) = cli.command else {
        // Started inside one of the running client's own panes: a client
        // inside a client is never what that means. Open a space for this
        // directory in the uze already running, and leave.
        if std::env::var_os("UZE_PANE").is_some() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let root = uze_application::space_root(&cwd);
            let label = uze_terminal::open_space(&root)
                .map_err(|error| uze_application::UzeError::TerminalRuntime(error.to_string()))?;
            println!(
                "opened space `{label}` at {} in the running uze",
                root.display()
            );
            return Ok(());
        }
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
            .map_err(|source| uze_application::UzeError::Write {
                path: PathBuf::from("stdout"),
                source,
            })?;
        println!();
        return Ok(());
    };
    if let Command::Terminal { action } = command {
        let root = std::env::current_dir().map_err(|source| uze_application::UzeError::Read {
            path: PathBuf::from("."),
            source,
        })?;
        return match action {
            TerminalAction::Attach => uze::ui::run(home),
            // Resolved the same way `ui::orchestrator` resolves it before
            // attaching — `stop` must target the server that `attach`
            // actually started, not one keyed on the raw cwd.
            TerminalAction::Stop => {
                uze_terminal::stop(&uze_application::workspace_root_or_self(&root))
                    .map_err(|error| uze_application::UzeError::TerminalRuntime(error.to_string()))
            }
            TerminalAction::Serve { root } => uze_terminal::serve(root)
                .map_err(|error| uze_application::UzeError::TerminalRuntime(error.to_string())),
        };
    }
    let app = UzeApplication::from_env(home.clone())?;
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
            match app
                .project()
                .install(&context_path(path), authority.as_ref())
            {
                Ok(report) => {
                    let message = match &report {
                        uze_application::application::InstallReport::NoChanges => {
                            "Already up to date"
                        }
                        uze_application::application::InstallReport::Installed { .. } => {
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
                    spinner.finish_and_clear();
                    progress::error(&format!("Failed to install environment: {e}"));
                    return Err(e);
                }
            }
        }
        Command::Remove { plugin, format } => {
            let current_dir =
                std::env::current_dir().map_err(|source| uze_application::UzeError::Read {
                    path: PathBuf::from("."),
                    source,
                })?;
            let spinner = progress::spinner(&format!("Removing {plugin} from this project..."));
            let report = match app.project().remove(&plugin, &current_dir) {
                Ok(report) => report,
                Err(e) => {
                    spinner.finish_and_clear();
                    progress::error(&format!("Failed to remove {plugin}: {e}"));
                    return Err(e);
                }
            };
            match report {
                RemoveProjectPluginReport::Removed { .. } => {
                    spinner.finish_and_clear();
                    match format {
                        OutputFormat::Text => {
                            progress::success(&format!("Removed {plugin} from project"));
                        }
                        OutputFormat::Json => {
                            print_json(&RemoveProjectPluginReport::Removed { plugin })
                        }
                    }
                }
                // Strictly project-scoped, by design (ADR-019): neither of
                // these falls through to machine-level removal. `?` below
                // surfaces `uze_application::UzeError::{NoProjectEnvironment,
                // PluginNotUsedByProject}` through the same `uze: {error}`
                // path every other failure in this program uses.
                RemoveProjectPluginReport::NoLock => {
                    spinner.finish_and_clear();
                    return Err(uze_application::UzeError::NoProjectEnvironment { plugin });
                }
                RemoveProjectPluginReport::NotInLock { .. } => {
                    spinner.finish_and_clear();
                    return Err(uze_application::UzeError::PluginNotUsedByProject { plugin });
                }
            }
        }
        Command::Status { path, format } => {
            let report = app.health().status(&context_path(path))?;
            match format {
                OutputFormat::Text => print!("{}", render_status(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
        Command::Agent { action } => run_agent(&app, action)?,
        Command::Context { action } => run_context(&app, action)?,
        Command::Theme { action } => run_theme(&app, &home, action)?,
        Command::Market { action } => run_market(&app, action)?,
        Command::Plugin { action } => run_plugin(&app, action, verbose)?,
        Command::Doctor { format } => {
            let spinner = progress::spinner("Running diagnostics...");
            let report = app.health().report();
            spinner.finish_with_message("Diagnostics complete");
            match format {
                OutputFormat::Text => print!("{}", render_doctor(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
        Command::Setup { arguments } => run_setup_command(&app, &home, &arguments, verbose)?,
        Command::External(args) => run_shorthand(&app, args, verbose)?,
        Command::HookExec {
            adapter,
            event,
            effect,
            plugin_root,
            commands,
        } => {
            let code = run_hook_exec(&home, &adapter, &event, &effect, &plugin_root, commands)?;
            // The exit code is part of the ABI: a denied outcome must read
            // as a denial to targets that key off exit codes, and an error
            // must not print a second `uze:` line into the harness's stderr.
            std::process::exit(code);
        }
        Command::Terminal { .. } => {
            unreachable!("terminal commands return before application setup")
        }
    }
    Ok(())
}

/// The `hook-exec` runtime wrapper (ADR-033): reads the harness's native
/// hook payload from stdin, normalizes it through the adapter, runs the
/// authored handlers sequentially against the portable ABI, and renders the
/// aggregated decision back into the harness's own native contract — JSON
/// stdout where the harness parses it, the reason on stderr where that is
/// the fed-back channel, and the harness's own blocking exit code (2 on
/// the command-hook harnesses) for a deny. Internal canonical exit codes
/// (the handler-level deny exit `3`) never leak outward: on Claude/Codex
/// any other non-zero exit is a *non-blocking* error ("logged and ignored,
/// execution continues") — leaking it would turn a deny into a tool that
/// still runs.
fn run_hook_exec(
    home: &UzeHome,
    adapter_id: &str,
    event_name: &str,
    effect_name: &str,
    plugin_root: &Path,
    commands: Vec<String>,
) -> Result<i32> {
    use std::io::{Read, Write};
    use uze_application::UzeError;

    let event = HookEvent::parse_abi(event_name)
        .ok_or_else(|| UzeError::HookDispatch(format!("unknown hook event `{event_name}`")))?;
    let effect = HookEffect::parse_abi(effect_name)
        .ok_or_else(|| UzeError::HookDispatch(format!("unknown hook effect `{effect_name}`")))?;
    let mut native = String::new();
    std::io::stdin()
        .read_to_string(&mut native)
        .map_err(|source| {
            UzeError::HookDispatch(format!("cannot read the native payload: {source}"))
        })?;
    let native: serde_json::Value = serde_json::from_str(&native).map_err(|source| {
        UzeError::HookDispatch(format!("the native hook payload is not JSON: {source}"))
    })?;

    let rendered = UzeApplication::from_env(home.clone())?.hooks().dispatch(
        adapter_id,
        event,
        effect,
        plugin_root,
        commands,
        &native,
    )?;
    if let Some(bytes) = rendered.stdout {
        std::io::stdout().write_all(&bytes).map_err(|source| {
            UzeError::HookDispatch(format!("cannot render hook output: {source}"))
        })?;
        let _ = std::io::stdout().flush();
    }
    if let Some(reason) = rendered.stderr {
        eprintln!("{reason}");
    }
    Ok(rendered.exit_code)
}

fn run_setup_command(
    app: &UzeApplication,
    home: &UzeHome,
    arguments: &[String],
    verbose: bool,
) -> Result<()> {
    match arguments {
        [] => run_setup(app, home, arguments, verbose),
        [command] if command == "list" => {
            print!("{}", render_harness_list(&app.health().harnesses()));
            Ok(())
        }
        [command, name] if command == "inspect" => {
            print!("{}", render_harness_detail(&app.health().harness(name)?));
            Ok(())
        }
        [command] if command == "inspect" => {
            setup_usage_error("`uze setup inspect` requires a harness name")
        }
        [command] if command == "help" => {
            print_setup_help();
            Ok(())
        }
        [command, ..] if command == "list" || command == "inspect" || command == "help" => {
            setup_usage_error(
                "use `uze setup list`, `uze setup inspect <harness>`, or `uze setup <harness>...`",
            )
        }
        harnesses => run_setup(app, home, harnesses, verbose),
    }
}

fn setup_usage_error(message: &str) -> ! {
    Cli::command()
        .error(ErrorKind::InvalidValue, message)
        .exit()
}

fn print_setup_help() {
    println!("{}", progress::title("UZE setup"));
    println!(
        "{}",
        progress::label("Provision and inspect machine harness integrations.")
    );
    println!();
    println!("{}", progress::section("Usage"));
    println!(
        "{}",
        progress::aligned_rows(vec![
            vec![
                "uze setup".to_owned(),
                "Choose harnesses interactively".to_owned(),
            ],
            vec![
                "uze setup <harness>...".to_owned(),
                "Provision exactly these".to_owned(),
            ],
            vec![
                "uze setup list".to_owned(),
                "Show every harness and its integration health".to_owned(),
            ],
            vec![
                "uze setup inspect <harness>".to_owned(),
                "Show one harness's detection and delivery".to_owned(),
            ],
        ])
    );
    println!();
    println!("{}", progress::section("Options"));
    println!(
        "{}",
        progress::aligned_rows(vec![
            vec![
                progress::success_text("help, --help, -h"),
                "Show this help".to_owned(),
            ],
            vec![
                progress::success_text("--verbose"),
                "Show delivery evidence".to_owned(),
            ],
        ])
    );
}

/// `uze setup` is the single machine-level harness surface. With no
/// arguments in a terminal it asks which harnesses to provision (see
/// `choose_harnesses`) and provisions every registered one anywhere a
/// question cannot be answered; with one or more ids it provisions exactly
/// those ids. `list` and `inspect` remain read-only views under the same
/// verb, so users do not need to learn a redundant namespace.
///
/// Progress contract: `setup` runs harnesses **sequentially
/// in registration order**, one opaque container per harness. The vendor
/// installer's output is buffered to `$UZE_HOME/state/logs/setup-<harness>.log`
/// instead of interleaving on the terminal, so the terminal shows only
/// ordered step headers and the per-harness final status.
/// `uze context …` — the project-scoped half of the grammar: what this
/// project declares, what reconciling it would write, and writing it. See
/// ADR-019 for why none of these ever touch machine state.
fn run_context(app: &UzeApplication, action: ContextAction) -> Result<()> {
    match action {
        ContextAction::Inspect { path, format } => {
            let status = app.context().inspect(&context_path(path))?;
            match format {
                OutputFormat::Text => print!("{}", render_context_status(&status)),
                OutputFormat::Json => print_json(&status),
            }
        }
        ContextAction::Plan { path, format } => {
            let plan = app.context().plan(&context_path(path))?;
            match format {
                OutputFormat::Text => print!("{}", render_context_plan(&plan, app)),
                OutputFormat::Json => print_json(&plan),
            }
        }
        ContextAction::Reconcile { path, format } => {
            let report = app.context().reconcile(&context_path(path))?;
            match format {
                OutputFormat::Text => {
                    print!("{}", render_context_reconciliation(&report, app));
                }
                OutputFormat::Json => print_json(&report),
            }
        }
    }
    Ok(())
}

/// `uze market …` — the marketplaces a machine knows about.
fn run_theme(app: &UzeApplication, home: &UzeHome, action: ThemeAction) -> Result<()> {
    match action {
        ThemeAction::List { format } => {
            let themes = app.themes().list(uze_theme::builtin_names())?;
            match format {
                OutputFormat::Text => print!("{}", render_theme_list(&themes)),
                OutputFormat::Json => print_json(&themes),
            }
        }
        ThemeAction::Use { id } => {
            // Load it before recording the choice: a theme that will not
            // resolve should be refused here, where the operator is looking,
            // rather than accepted and complained about on every later run.
            let resolved = uze::theme::resolve(app, home, &id)?;
            for warning in &resolved.warnings {
                progress::warn(&warning.to_string());
            }
            app.themes().select(&id)?;
            uze_theme::set_active(resolved.theme);
            progress::success(&format!("Drawing in {id}"));
        }
        ThemeAction::Show { id, format } => {
            let id = match id {
                Some(id) => id,
                None => app
                    .themes()
                    .active()?
                    .unwrap_or_else(|| uze_theme::builtin_names()[0].to_owned()),
            };
            let (resolved, layers) = uze::theme::resolve_with_layers(app, home, &id)?;
            match format {
                OutputFormat::Text => print!("{}", render_theme(&id, layers.clone(), &resolved)),
                OutputFormat::Json => print_json(&theme_report(&id, layers, &resolved)),
            }
        }
    }
    Ok(())
}

fn render_theme_list(themes: &[uze_application::application::ThemeSummary]) -> String {
    let mut out = String::new();
    out.push_str(&progress::section("Themes\n"));
    let rows: Vec<Vec<String>> = themes
        .iter()
        .map(|theme| {
            let mark = if theme.active {
                progress::success_icon()
            } else {
                " ".to_owned()
            };
            let source = match &theme.path {
                Some(path) => progress::label(path.display().to_string()),
                None => progress::label("built in"),
            };
            vec![mark, progress::title(&theme.id), source]
        })
        .collect();
    out.push_str(&progress::aligned_rows(rows));
    out.push('\n');
    out
}

/// What `theme show` reports, in the shape `--format json` prints.
#[derive(serde::Serialize)]
struct ThemeReport {
    id: String,
    name: String,
    description: String,
    /// The layers this resolved from, bottom-up. With `extends` and the
    /// operator's own overrides in play, "where did this colour come from"
    /// is the first thing an author asks.
    layers: Vec<String>,
    syntax_theme: String,
    colors: std::collections::BTreeMap<String, String>,
    symbols: std::collections::BTreeMap<String, Vec<String>>,
    warnings: Vec<String>,
}

fn theme_report(id: &str, layers: Vec<String>, loaded: &uze_theme::Loaded) -> ThemeReport {
    let theme = &loaded.theme;
    ThemeReport {
        id: id.to_owned(),
        name: theme.name().to_owned(),
        description: theme.description().to_owned(),
        layers,
        syntax_theme: theme.syntax_theme().to_owned(),
        colors: uze_theme::Token::ALL
            .iter()
            .map(|token| (token.to_string(), theme.color(*token).to_string()))
            .collect(),
        symbols: uze_theme::Symbol::ALL
            .iter()
            .map(|symbol| (symbol.to_string(), theme.symbol(*symbol).frames().to_vec()))
            .collect(),
        warnings: loaded
            .warnings
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
    }
}

fn render_theme(id: &str, layers: Vec<String>, loaded: &uze_theme::Loaded) -> String {
    let report = theme_report(id, layers, loaded);
    let mut out = String::new();
    out.push_str(&progress::title(format!("{} ({id})\n", report.name)));
    if !report.description.is_empty() {
        out.push_str(&progress::label(format!("{}\n", report.description)));
    }
    out.push_str(&progress::label(format!(
        "resolved from {}\n",
        report.layers.join(" → ")
    )));
    out.push('\n');
    out.push_str(&progress::section("Colours\n"));
    out.push_str(&progress::aligned_rows(
        report
            .colors
            .iter()
            .map(|(token, value)| vec![progress::label(token), progress::title(value)])
            .collect(),
    ));
    out.push_str("\n\n");
    out.push_str(&progress::section("Symbols\n"));
    out.push_str(&progress::aligned_rows(
        report
            .symbols
            .iter()
            .map(|(symbol, frames)| vec![progress::label(symbol), frames.join(" ")])
            .collect(),
    ));
    out.push('\n');
    if !report.warnings.is_empty() {
        out.push('\n');
        out.push_str(&progress::section("Warnings\n"));
        for warning in &report.warnings {
            out.push_str(&format!("  {}\n", progress::warning_text(warning)));
        }
    }
    out
}

fn run_market(app: &UzeApplication, action: MarketAction) -> Result<()> {
    match action {
        MarketAction::Add { source } => {
            let spinner = progress::spinner("Adding marketplace...");
            match app.marketplace().add(&source) {
                Ok(true) => {
                    spinner.finish_and_clear();
                    progress::success(&format!("Added marketplace from {source}"));
                }
                Ok(false) => {
                    spinner.finish_and_clear();
                    progress::success(&format!("Marketplace from {source} is already added"));
                }
                Err(e) => {
                    spinner.finish_and_clear();
                    progress::error(&format!("Failed to add marketplace: {e}"));
                    return Err(e);
                }
            }
        }
        MarketAction::List { format } => {
            let mps = app.marketplace().list()?;
            match format {
                OutputFormat::Text => print!("{}", render_market_list(&mps)),
                OutputFormat::Json => print_json(&mps),
            }
        }
        MarketAction::Remove { name } => {
            app.marketplace().remove(&name)?;
            println!("Removed marketplace {name}");
        }
        MarketAction::Inspect { name, format } => {
            let detail = app.marketplace().inspect(&name)?;
            match format {
                OutputFormat::Text => print!("{}", render_market_detail(&detail)),
                OutputFormat::Json => print_json(&detail),
            }
        }
    }
    Ok(())
}

/// `uze plugin …` — the machine-scoped package lifecycle.
fn run_plugin(app: &UzeApplication, action: PluginAction, verbose: bool) -> Result<()> {
    match action {
        PluginAction::Install {
            plugin,
            trust,
            alias,
            replace,
            format,
        } => {
            let authority = trust_authority(trust);
            let name_authority = name_collision_authority(alias, replace);
            let spinner = progress::spinner(&format!("Installing plugin {plugin}..."));
            // Every install goes through a marketplace that must have
            // been added first: `uze market add <market>` then
            // `uze plugin install <name>@<market>`. A direct source
            // (path or Git URL) is never accepted — the marketplace is
            // the product's provenance contract (see ADR-019).
            let installed = if plugin.contains('@') {
                app.marketplace().install_plugin_resolving(
                    &plugin,
                    authority.as_ref(),
                    name_authority.as_ref(),
                )
            } else {
                return Err(uze_application::UzeError::UnknownPackage(format!(
                    "`{plugin}` is not a `name@marketplace` spec; add its marketplace with \
                     `uze market add <market>` first, then install with \
                     `uze plugin install {plugin}@<market>`"
                )));
            };
            match installed {
                Ok(report) => {
                    spinner.finish_and_clear();
                    match format {
                        OutputFormat::Text => {
                            println!(
                                "{}",
                                progress::report_title("Plugin installed", Some(&report.plugin.id))
                            );
                            println!(
                                "{}",
                                progress::key_value(
                                    "Store path",
                                    report.plugin.store_path.display().to_string()
                                )
                            );
                            print!("{}", render_add_report(&report, verbose, app));
                            for publication in &report.publications {
                                if let Some(error) = &publication.error {
                                    progress::warn(&format!(
                                        "{} could not publish: {error}",
                                        app.health().integration_label(&publication.integration)
                                    ));
                                }
                            }
                        }
                        OutputFormat::Json => print_json(&report),
                    }
                }
                Err(e) => {
                    spinner.finish_and_clear();
                    progress::error(&format!("Failed to install plugin: {e}"));
                    return Err(e);
                }
            }
        }
        PluginAction::List { format } => {
            let plugins = app.plugins().list()?;
            match format {
                OutputFormat::Text => {
                    println!(
                        "{}",
                        progress::report_title("Plugins", Some("Installed on this machine"))
                    );
                    if plugins.is_empty() {
                        println!("  No plugins installed");
                    } else {
                        println!(
                            "{}",
                            progress::aligned_rows(
                                plugins
                                    .iter()
                                    .map(|plugin| {
                                        let origin = if plugin.active_name == plugin.id {
                                            String::new()
                                        } else {
                                            progress::label(format!("origin: {}", plugin.id))
                                        };
                                        vec![
                                            progress::title(&plugin.active_name),
                                            origin,
                                            format!("{} capabilities", plugin.capability_count),
                                        ]
                                    })
                                    .collect()
                            )
                        );
                    }
                }
                OutputFormat::Json => print_json(&plugins),
            }
        }
        PluginAction::Inspect { plugin, format } => {
            let report = app.plugins().inspect(&plugin)?;
            match format {
                OutputFormat::Text => print!("{}", render_inspection(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
        PluginAction::Remove { plugin, format } => {
            let report = app.plugins().remove(&plugin)?;
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
            let report = app.plugins().update(&plugin, authority.as_ref())?;
            match format {
                OutputFormat::Text => print!("{}", render_update(&report)),
                OutputFormat::Json => print_json(&report),
            }
        }
    }
    Ok(())
}

/// Answers "which harnesses?" when `uze setup` was given none.
///
/// A terminal is asked, because provisioning every harness the catalog
/// knows about is a decision about someone's machine that nobody made —
/// installing three vendors' CLIs to use one. Anything else (a pipe, an
/// image build, a CI job) still gets the whole catalog: a caller that
/// cannot answer must not be blocked on the question, and a script that
/// wrote `uze setup` meant all of them.
///
/// The detected harnesses come pre-checked, so the common answer —
/// "provision what I already use" — is one keystroke, while a fresh
/// machine has to say what it wants. `None` is the run being called off,
/// which is never the same as an empty selection.
fn choose_harnesses(app: &UzeApplication) -> Option<Vec<String>> {
    let catalog = app.health().harnesses();
    if catalog.is_empty() || !prompt::interactive() {
        return Some(catalog.into_iter().map(|h| h.integration).collect());
    }
    let choices: Vec<prompt::Choice> = catalog
        .iter()
        .map(|harness| prompt::Choice {
            label: harness.display_name.clone(),
            hint: harness_hint(harness),
            selected: harness.detection.present,
        })
        .collect();
    match prompt::multi_select("Harnesses to provision", &choices) {
        // No second line: the question's own answer line already reads
        // `Harnesses to provision: nothing`, and saying it twice is how a
        // report teaches people to skip it.
        prompt::Answer::Chosen(picked) if picked.is_empty() => None,
        prompt::Answer::Chosen(picked) => Some(
            picked
                .into_iter()
                .map(|index| catalog[index].integration.clone())
                .collect(),
        ),
        prompt::Answer::Declined => {
            println!("{}", progress::label("Setup cancelled"));
            None
        }
    }
}

/// What a person needs in order to choose one row: whether the harness is
/// already on this machine, and which version answered.
fn harness_hint(harness: &HarnessHealth) -> String {
    if !harness.detection.present {
        return "not installed".to_owned();
    }
    match &harness.detection.version {
        Some(version) => format!("installed {version}"),
        None => "installed".to_owned(),
    }
}

fn run_setup(
    app: &UzeApplication,
    home: &UzeHome,
    harnesses: &[String],
    verbose: bool,
) -> Result<()> {
    let targets: Vec<String> = if harnesses.is_empty() {
        match choose_harnesses(app) {
            Some(chosen) => chosen,
            None => return Ok(()),
        }
    } else {
        harnesses.to_vec()
    };
    if targets.is_empty() {
        println!("No harnesses registered");
        return Ok(());
    }
    let total = targets.len();
    let is_tty = std::io::stderr().is_terminal();
    if is_tty {
        println!(
            "{} Provisioning {} harness(es) through official routes…",
            "▸".cyan().bold(),
            total.to_string().cyan().bold()
        );
    } else {
        println!(
            "Provisioning {} harness(es) through official routes…",
            total
        );
    }
    if !verbose {
        let msg = "(installer output is buffered per harness — see $UZE_HOME/state/logs/setup-<harness>.log; use --verbose to stream)";
        if is_tty {
            println!("{}", msg.dim());
        } else {
            println!("{}", msg);
        }
    }
    let logs_dir = home.state_dir().join("logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let mut had_warning = false;
    let mut failed_harnesses: Vec<String> = Vec::new();
    let mut shell_path_hints = Vec::new();
    let mut shell_path_shim_names = Vec::new();

    for (idx, id) in targets.iter().enumerate() {
        let step = idx + 1;
        let header = if is_tty {
            crate::progress::step_header(step, total, id)
        } else {
            format!("[{}/{}] {} — provisioning…", step, total, id)
        };
        let spinner = if is_tty {
            Some(progress::spinner(&header))
        } else {
            println!("{}", header);
            None
        };
        let log_path = logs_dir.join(format!("setup-{}.log", id));
        let _ = std::fs::write(
            &log_path,
            format!("=== uze setup {} — {} ===\n", id, chrono_stamp()),
        );
        let runner = CapturingRunner::new(log_path.clone(), verbose);
        let per_app = UzeApplication::from_env_with_runner(home.clone(), Box::new(runner))?;
        let results = match per_app.setup(Some(id)) {
            Ok(r) => r,
            Err(e) => {
                if let Some(pb) = &spinner {
                    pb.finish_and_clear();
                }
                progress::error(&format!("[{}/{}] {} failed: {}", step, total, id, e));
                if !verbose {
                    eprintln!("  → log: {}", log_path.display());
                    if let Ok(tail) = read_tail(&log_path, 20) {
                        eprintln!("  ── tail ──\n{}\n  ──", tail);
                    }
                }
                return Err(e);
            }
        };
        for result in &results {
            if result.configured {
                let summary = if is_tty {
                    format!(
                        "{} [{}/{}] {}: {} ({}; version {})",
                        crate::progress::success_icon(),
                        step,
                        total,
                        result.integration.cyan().bold(),
                        "ready".green().bold(),
                        format!("{:?}", result.provisioning.action)
                            .to_lowercase()
                            .dim(),
                        result
                            .detection
                            .version
                            .as_deref()
                            .unwrap_or("unknown")
                            .cyan()
                    )
                } else {
                    format!(
                        "[{}/{}] {}: ready ({}; version {})",
                        step,
                        total,
                        result.integration,
                        format!("{:?}", result.provisioning.action).to_lowercase(),
                        result.detection.version.as_deref().unwrap_or("unknown")
                    )
                };
                if let Some(pb) = &spinner {
                    pb.finish_and_clear();
                }
                println!("{}", summary);
                if let Some(shim) = &result.runtime_shim {
                    println!("  ↳ shim: {}", shim.shim_path.display().to_string().dim());
                    if let Some(rc) = &shim.rc_file_updated {
                        println!("    added to PATH in {}", rc.display().to_string().cyan());
                    }
                    if let Some(hint) = &shim.path_hint {
                        shell_path_hints.push(hint.clone());
                        if let Some(name) = shim.shim_path.file_name().and_then(|n| n.to_str()) {
                            let name = name.to_owned();
                            if !shell_path_shim_names.contains(&name) {
                                shell_path_shim_names.push(name);
                            }
                        }
                    }
                }
                if let Some(err) = &result.attach_error {
                    had_warning = true;
                    if is_tty {
                        eprintln!(
                            "  {} {}: {}",
                            crate::progress::warning_icon(),
                            id.cyan().bold(),
                            err.yellow()
                        );
                        eprintln!(
                            "    {} run `uze doctor` for details; fix and re-run `uze setup {}`",
                            "→".dim(),
                            id.cyan()
                        );
                        eprintln!(
                            "    {} log: {}",
                            "→".dim(),
                            log_path.display().to_string().dim()
                        );
                    } else {
                        eprintln!("  warning {}: {}", id, err);
                        eprintln!("    log: {}", log_path.display());
                    }
                    if verbose && let Ok(tail) = std::fs::read_to_string(&log_path) {
                        print_log_block(&log_path, &tail);
                    }
                }
                if let Some(err) = &result.shim_error {
                    had_warning = true;
                    if is_tty {
                        eprintln!(
                            "  {} shim {}: {}",
                            crate::progress::warning_icon(),
                            id.cyan().bold(),
                            err.yellow()
                        );
                    } else {
                        eprintln!("  shim warning {}: {}", id, err);
                    }
                    eprintln!("    log: {}", log_path.display().to_string().dim());
                }
                if verbose
                    && result.attach_error.is_none()
                    && result.shim_error.is_none()
                    && let Ok(content) = std::fs::read_to_string(&log_path)
                    && !content.trim().is_empty()
                    && content.lines().count() > 1
                {
                    print_log_block(&log_path, &content);
                }
            } else {
                if let Some(pb) = &spinner {
                    pb.finish_and_clear();
                }
                let summary = if is_tty {
                    format!(
                        "{} [{}/{}] {}: {} {:?}: {}",
                        crate::progress::error_icon(),
                        step,
                        total,
                        result.integration.cyan().bold(),
                        "setup".red().bold(),
                        result.provisioning.status,
                        result
                            .provisioning
                            .reason
                            .as_deref()
                            .unwrap_or("executable was not verified")
                            .dim()
                    )
                } else {
                    format!(
                        "[{}/{}] {}: setup {:?}: {}",
                        step,
                        total,
                        result.integration,
                        result.provisioning.status,
                        result
                            .provisioning
                            .reason
                            .as_deref()
                            .unwrap_or("executable was not verified")
                    )
                };
                println!("{}", summary);
                failed_harnesses.push(result.integration.clone());
                if let Some(err) = &result.attach_error {
                    eprintln!("  {} {}", crate::progress::warning_icon(), err.yellow());
                }
                if verbose {
                    if let Ok(content) = std::fs::read_to_string(&log_path) {
                        print_log_block(&log_path, &content);
                    }
                } else {
                    eprintln!("  → log: {}", log_path.display().to_string().dim());
                }
            }
        }
    }
    if let Some(command) = shell_path_reload_command(&shell_path_hints) {
        println!("\nShell PATH was updated. Run this in the current terminal:");
        println!("  {}", command.cyan().bold());
        println!("Then verify:");
        for name in &shell_path_shim_names {
            println!("  {}", format!("which {}", name).cyan().bold());
        }
    }
    // A harness that was not provisioned is a failed setup, not a warning:
    // a caller that scripts `uze setup` (an image build, a bootstrap) must
    // never read "all ready" over a missing binary.
    if !failed_harnesses.is_empty() {
        return Err(uze_application::UzeError::ProvisioningIncomplete(format!(
            "{} of {} harness(es) ready; not provisioned: {} (see `uze doctor`)",
            total - failed_harnesses.len(),
            total,
            failed_harnesses.join(", ")
        )));
    }
    if had_warning {
        if is_tty {
            eprintln!(
                "\n{} Setup completed with warnings — some harnesses need manual cleanup. See `{}`.",
                crate::progress::warning_icon(),
                "uze doctor".cyan()
            );
        } else {
            eprintln!(
                "\nSetup completed with warnings — some harnesses need manual cleanup. See `uze doctor`."
            );
        }
    } else if is_tty {
        println!(
            "\n{} Setup completed — all {} harness(es) ready.",
            crate::progress::success_icon(),
            total.to_string().green().bold()
        );
    } else {
        println!("\nSetup completed — all {} harness(es) ready.", total);
    }
    Ok(())
}

fn shell_path_reload_command(hints: &[String]) -> Option<&str> {
    let hint = hints.first()?;
    Some(
        hint.strip_prefix("open a new terminal, or run: ")
            .unwrap_or(hint),
    )
}

#[cfg(test)]
mod setup_output_tests {
    use super::shell_path_reload_command;

    #[test]
    fn shell_reload_command_is_deduplicated_for_many_harnesses() {
        let hints = vec![
            "open a new terminal, or run: source ~/.zshrc".to_owned(),
            "open a new terminal, or run: source ~/.zshrc".to_owned(),
            "open a new terminal, or run: source ~/.zshrc".to_owned(),
        ];
        assert_eq!(shell_path_reload_command(&hints), Some("source ~/.zshrc"));
    }
}

fn chrono_stamp() -> String {
    format!("{:?}", std::time::SystemTime::now())
}

fn print_log_block(path: &std::path::Path, content: &str) {
    println!("  ── log {} ──", path.display().to_string().dim());
    let lines: Vec<&str> = content.lines().collect();
    let to_show = if lines.len() > 80 { 80 } else { lines.len() };
    for line in lines.iter().take(to_show) {
        println!("  {} {}", crate::progress::log_prefix(), line.dim());
    }
    if lines.len() > to_show {
        println!(
            "  {} … ({} more lines, see {})",
            crate::progress::log_prefix(),
            lines.len() - to_show,
            path.display().to_string().dim()
        );
    }
    println!("  ── end log ──");
}

fn read_tail(path: &std::path::Path, n: usize) -> std::io::Result<String> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].join("\n"))
}

/// `ProcessRunner` that buffers an installer's inherited output to a per-harness
/// log file instead of streaming it interleaved on the terminal. `Quiet`
/// probes stay quiet; `Inherit` (the installer) is redirected to the file so
/// each harness's log is an opaque container until the step finishes.
struct CapturingRunner {
    log_path: std::path::PathBuf,
    verbose: bool,
}

impl CapturingRunner {
    fn new(log_path: std::path::PathBuf, verbose: bool) -> Self {
        Self { log_path, verbose }
    }
}

impl uze_application::ProcessRunner for CapturingRunner {
    fn run(
        &self,
        spec: &uze_application::ProcessSpec,
    ) -> uze_application::Result<uze_application::ProcessResult> {
        use std::fs::OpenOptions;
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        let mut command = Command::new(&spec.program);
        command.args(&spec.arguments).stdin(Stdio::null());
        match spec.output {
            uze_application::ProcessOutput::Quiet => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
            uze_application::ProcessOutput::Inherit => {
                if let Ok(mut f) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log_path)
                {
                    use std::io::Write;
                    let _ = writeln!(
                        f,
                        "\n--- run: {} {} (timeout {:?}) ---",
                        spec.program,
                        spec.arguments.join(" "),
                        spec.timeout
                    );
                }
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log_path)
                    .map_err(|source| uze_application::UzeError::Write {
                        path: self.log_path.clone(),
                        source,
                    })?;
                let stdout =
                    file.try_clone()
                        .map_err(|source| uze_application::UzeError::Write {
                            path: self.log_path.clone(),
                            source,
                        })?;
                let stderr = file;
                command
                    .stdout(Stdio::from(stdout))
                    .stderr(Stdio::from(stderr));
                if self.verbose {
                    eprintln!("  │ run: {} {}", spec.program, spec.arguments.join(" "));
                }
            }
        }
        let mut child = command
            .spawn()
            .map_err(|source| uze_application::UzeError::Process {
                program: spec.program.clone(),
                source,
            })?;
        let started = Instant::now();
        loop {
            if let Some(status) =
                child
                    .try_wait()
                    .map_err(|source| uze_application::UzeError::Process {
                        program: spec.program.clone(),
                        source,
                    })?
            {
                return Ok(uze_application::ProcessResult {
                    success: status.success(),
                    timed_out: false,
                });
            }
            if started.elapsed() >= spec.timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(uze_application::ProcessResult {
                    success: false,
                    timed_out: true,
                });
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
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
    let (plugin, marketplace) = match uze_application::parse_plugin_marketplace_spec(&first) {
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

    let current_dir =
        std::env::current_dir().map_err(|source| uze_application::UzeError::Read {
            path: PathBuf::from("."),
            source,
        })?;
    let authority = trust_authority(shorthand.trust);
    let report = app
        .project()
        .add(&plugin, &marketplace, &current_dir, authority.as_ref())?;

    match shorthand.format {
        OutputFormat::Text => {
            println!(
                "{}",
                progress::report_title(
                    "Added to project",
                    Some(&format!("{plugin}@{marketplace}"))
                )
            );
            println!(
                "{}",
                progress::key_value("Store path", report.plugin.store_path.display().to_string())
            );
            print!(
                "{}",
                render_add_report(&report, verbose || shorthand.verbose, app)
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

/// Chooses who answers a trust question.
///
/// Without `--trust`, an interactive terminal prompts and anything else
/// refuses to answer — a pipeline gets `TRUST_REQUIRED` rather than a silent
/// yes.
fn trust_authority(trusted: bool) -> Box<dyn uze_application::TrustAuthority> {
    if trusted {
        return Box::new(uze_application::AlwaysTrust);
    }
    if prompt::interactive() {
        return Box::new(PromptingAuthority);
    }
    Box::new(uze_application::NoTrustAuthority)
}

struct PromptingAuthority;

impl uze_application::TrustAuthority for PromptingAuthority {
    fn authorize(&self, request: &uze_application::TrustRequest) -> uze_application::TrustOutcome {
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
        match prompt::confirm("Trust and install?", false) {
            Some(true) => uze_application::TrustOutcome::Granted,
            Some(false) => uze_application::TrustOutcome::Denied,
            // Withdrawn, or a terminal that would not carry the question.
            // Neither is a person declining, and the caller has its own
            // ending for that.
            None => uze_application::TrustOutcome::Unavailable,
        }
    }
}

/// Chooses who answers a plugin-name-collision question (ADR-038).
///
/// `--alias`/`--replace` answer it out of band. Without either, an
/// interactive terminal prompts and anything else refuses to answer — a
/// pipeline gets the structured `PluginNameCollision` error rather than a
/// silent shadowing of the plugin already active under that name.
fn name_collision_authority(
    alias: Option<String>,
    replace: bool,
) -> Box<dyn uze_application::NameCollisionAuthority> {
    if let Some(alias) = alias {
        return Box::new(uze_application::FixedResolution(
            uze_application::NameCollisionResolution::Alias(alias),
        ));
    }
    if replace {
        return Box::new(uze_application::FixedResolution(
            uze_application::NameCollisionResolution::Replace,
        ));
    }
    if prompt::interactive() {
        return Box::new(PromptingCollisionAuthority);
    }
    Box::new(uze_application::NoNameCollisionAuthority)
}

struct PromptingCollisionAuthority;

impl uze_application::NameCollisionAuthority for PromptingCollisionAuthority {
    fn resolve(
        &self,
        request: &uze_application::NameCollisionRequest,
    ) -> uze_application::NameCollisionResolution {
        println!();
        println!(
            "`{}` is already active as `{}` — installing `{}` under the same name would silently \
             shadow it in every harness.",
            request.name, request.existing, request.requested
        );
        let resolutions = [
            prompt::Choice {
                label: "Keep existing".to_owned(),
                hint: format!("`{}` stays the one under this name", request.existing),
                selected: true,
            },
            prompt::Choice {
                label: "Replace it".to_owned(),
                hint: format!("`{}` takes the name over", request.requested),
                selected: false,
            },
            prompt::Choice {
                label: "Alias this install".to_owned(),
                hint: "both stay, under names of their own".to_owned(),
                selected: false,
            },
        ];
        match prompt::select("How should this be resolved?", &resolutions) {
            Some(1) => uze_application::NameCollisionResolution::Replace,
            Some(2) => match prompt::ask("New local name") {
                // An alias nobody typed is not a name to install under.
                Some(alias) if !alias.is_empty() => {
                    uze_application::NameCollisionResolution::Alias(alias)
                }
                _ => uze_application::NameCollisionResolution::Abort,
            },
            _ => uze_application::NameCollisionResolution::Abort,
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
    let mut text = progress::report_title(&report.plugin.id, Some("Plugin inspection"));
    text.push('\n');
    text.push_str(&progress::report_section("Source"));
    text.push_str(&format!("  {}\n\n", report.plugin.source));
    text.push_str(&progress::report_section("Capabilities"));
    for capability in &report.capabilities {
        text.push_str(&format!("  {:?}  {}\n", capability.kind, capability.name));
    }
    text.push('\n');
    text.push_str(&progress::report_section("Delivery"));
    for delivery in &report.deliveries {
        let package = delivery
            .package_plan
            .as_ref()
            .map_or("decomposed".to_owned(), |plan| format!("{:?}", plan.route));
        text.push_str(&format!(
            "\n{}\n  Package  {package}\n",
            delivery.display_name
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
    text.push('\n');
    text.push_str(&progress::report_section("Managed state"));
    text.push_str(&format!(
        "  {} matched\n  {} missing\n  {} drifted\n  {} conflicts\n  {} blocked\n",
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
/// `plugin inspect` state the same facts read-only. Harness rows carry the
/// human label (`app.integration_label`) — the report's own keys stay the
/// stable ids, which is what `--format json` emits.
fn render_add_report(report: &AddPluginReport, verbose: bool, app: &UzeApplication) -> String {
    let mut out = format!("\n{}", progress::report_section("Delivery"));
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
        out.push_str(&format!(
            "  {}: {route}{attached}\n",
            app.health().integration_label(harness)
        ));
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
                app.health().integration_label(&attachment.integration),
                attachment.location.display()
            ));
        }
    }
    out
}

fn render_update(report: &uze_application::application::UpdatePluginReport) -> String {
    use uze_application::application::UpdatePluginReport;
    match report {
        UpdatePluginReport::Updated { plugin, .. } => format!(
            "{} Updated {}\n",
            progress::success_icon(),
            progress::title(&plugin.id)
        ),
        UpdatePluginReport::Blocked { report, plan } => {
            let mut text = progress::report_title("Update blocked", Some(&report.package_id));
            text.push_str(&format!(
                "{}\n\n",
                progress::warning_text(format!("Plan: {plan:?}"))
            ));
            text.push_str(&progress::report_section("Managed state"));
            text.push_str(&format!(
                "{}\n",
                render_managed_state(
                    &report
                        .receipts
                        .iter()
                        .map(|receipt| receipt.inspection.state)
                        .collect::<Vec<_>>(),
                )
            ));
            text
        }
    }
}

fn render_remove(report: &RemovePluginReport) -> String {
    match report {
        RemovePluginReport::AlreadyAbsent { plugin } => {
            format!(
                "{} No UZE state remains for {plugin}\n",
                progress::success_icon()
            )
        }
        RemovePluginReport::Removed { plugin, .. } => {
            format!(
                "{} Removed {}\n",
                progress::success_icon(),
                progress::title(plugin)
            )
        }
        RemovePluginReport::Blocked { report, plan } => {
            let mut text = progress::report_title("Removal blocked", Some(&report.package_id));
            text.push_str(&format!(
                "{}\n\n",
                progress::warning_text(format!("Plan: {plan:?}"))
            ));
            text.push_str(&progress::report_section("Managed state"));
            text.push_str(&format!(
                "{}\n",
                render_managed_state(
                    &report
                        .receipts
                        .iter()
                        .map(|receipt| receipt.inspection.state)
                        .collect::<Vec<_>>(),
                )
            ));
            text
        }
    }
}

fn render_install(report: &uze_application::application::InstallReport) -> String {
    use uze_application::application::InstallReport;
    match report {
        InstallReport::NoChanges => format!(
            "{} Project environment is already up to date.\n",
            progress::success_icon()
        ),
        InstallReport::Installed {
            plugins,
            removed,
            reconciled,
        } => {
            let mut text = progress::report_title("Installed environment", None);
            text.push_str(&format!(
                "{} plugin(s) are ready\n\n",
                progress::success_text(plugins.len().to_string())
            ));
            if !plugins.is_empty() {
                text.push_str(&progress::report_section("Packages"));
                for plugin in plugins {
                    text.push_str(&format!("  {plugin}\n"));
                }
            }
            if !removed.is_empty() {
                text.push_str(&progress::report_section("Removed"));
                for plugin in removed {
                    text.push_str(&format!("  {plugin}\n"));
                }
            }
            if *reconciled {
                text.push_str(&progress::report_section("Context"));
                text.push_str("  AGENTS.md and the harness bridges are reconciled\n");
            }
            text
        }
    }
}

/// The agent's own surface. Every answer here is written for a model: a
/// refusal names what this project would have accepted, because that
/// sentence is the only feedback channel a denied agent has.
fn run_agent(app: &UzeApplication, action: AgentAction) -> Result<()> {
    let AgentAction::Task { action } = action;
    match action {
        AgentTaskAction::Name { name, format } => {
            let cwd =
                std::env::current_dir().map_err(|source| uze_application::UzeError::Read {
                    path: PathBuf::from("."),
                    source,
                })?;
            let named = app.workspace().name_task(&cwd, &name)?;
            match format {
                OutputFormat::Text => println!(
                    "{} named `{}` on branch `{}`",
                    progress::success_icon(),
                    named.label,
                    named.branch
                ),
                OutputFormat::Json => print_json(&NamedTaskReport::from(&named)),
            }
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct NamedTaskReport<'a> {
    task: &'a str,
    branch: &'a str,
    label: &'a str,
}

impl<'a> From<&'a uze_application::NamedTask> for NamedTaskReport<'a> {
    fn from(named: &'a uze_application::NamedTask) -> Self {
        Self {
            task: &named.task,
            branch: &named.branch,
            label: &named.label,
        }
    }
}

fn render_doctor(report: &DoctorReport) -> String {
    let mut text =
        progress::report_title("Environment diagnostics", Some("Read-only health report"));
    text.push('\n');
    text.push_str(&progress::report_section("UZE Home"));
    text.push_str(&format!("  {}\n\n", report.uze_home.display()));
    text.push_str(&progress::report_section("Store"));
    text.push_str(&format!("  {:?}\n\n", report.store));
    text.push_str(&progress::report_section("Plugins"));
    text.push_str(&format!("  {} installed\n\n", report.plugins.len()));
    text.push_str(&progress::report_section("Harnesses"));
    text.push_str(&progress::aligned_rows(
        report
            .harnesses
            .iter()
            .map(|harness| {
                let detected = if harness.detection.present {
                    progress::success_text("detected")
                } else {
                    progress::label("not detected")
                };
                vec![
                    progress::title(&harness.display_name),
                    detected,
                    progress::label(format!("setup: {}", harness.setup)),
                ]
            })
            .collect(),
    ));
    text.push('\n');
    for harness in &report.harnesses {
        if let Some(provisioning) = &harness.provisioning {
            text.push_str(&format!(
                "    provisioning: {:?} via {} ({:?})\n",
                provisioning.status, provisioning.method, provisioning.action
            ));
        }
        if let uze_application::PublicationStatus::Unpublished(reason) = &harness.publication {
            text.push_str(&format!("    package view not published: {reason}\n"));
        }
    }
    text.push('\n');
    text.push_str(&progress::report_section("Attachments"));
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
        for hook in &attachment.hooks {
            let verdict = format!("{:?}", hook.route).to_lowercase();
            let attached = match (&hook.artifact, &hook.state) {
                (Some(artifact), Some(state)) => {
                    format!(" | {:?} at {}", state, artifact.display())
                }
                _ => String::new(),
            };
            let weakened = hook
                .weakened
                .as_deref()
                .map(|loss| format!(" | weakened: {loss}"))
                .unwrap_or_default();
            let delivery = hook
                .delivery
                .as_deref()
                .map(|note| format!(" | delivery: {note}"))
                .unwrap_or_default();
            // Hook rows key on the stable id; render the label doctor
            // already carries for the harness.
            text.push_str(&format!(
                "    hook {} [{}] on {}: {verdict}{attached}{weakened}{delivery}\n",
                hook.hook,
                hook.event,
                harness_label_of(report, &hook.harness)
            ));
        }
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
    if !report.maintenance.outcomes.is_empty() {
        text.push_str("\nMaintenance\n");
        for outcome in &report.maintenance.outcomes {
            text.push_str(&format!("  {:?}\n", outcome));
        }
    }
    text
}

/// The label `DoctorReport.harnesses` carries for a hook row's stable
/// integration id — the id falls back to itself for any future id the
/// report doesn't describe.
fn harness_label_of(report: &DoctorReport, id: &str) -> String {
    report
        .harnesses
        .iter()
        .find(|harness| harness.integration == id)
        .map(|harness| harness.display_name.clone())
        .unwrap_or_else(|| id.to_owned())
}

fn render_market_list(marketplaces: &[MarketplaceSummary]) -> String {
    let mut text = progress::report_title("Marketplaces", Some("Machine-scoped sources"));
    text.push('\n');
    if marketplaces.is_empty() {
        text.push_str("  No marketplaces registered\n");
        return text;
    }
    text.push_str(&progress::aligned_rows(
        marketplaces
            .iter()
            .map(|market| {
                vec![
                    progress::title(&market.name),
                    progress::label(&market.source),
                    format!("{} plugins", market.plugin_count),
                ]
            })
            .collect(),
    ));
    text.push('\n');
    text
}

fn render_market_detail(detail: &MarketplaceSummary) -> String {
    let mut text = progress::report_title(&detail.name, Some("Marketplace"));
    text.push('\n');
    text.push_str(&progress::report_section("Source"));
    text.push_str(&format!("  {}\n\n", detail.source));
    text.push_str(&progress::report_section("Plugins"));
    text.push_str(&format!("  {}\n", detail.plugin_count));
    text
}

fn render_harness_list(harnesses: &[HarnessHealth]) -> String {
    let mut text = progress::report_title("Harnesses", Some("Machine integration health"));
    text.push('\n');
    if harnesses.is_empty() {
        text.push_str("  No harnesses registered\n");
        return text;
    }
    text.push_str(&progress::aligned_rows(
        harnesses
            .iter()
            .map(|harness| {
                let detected = if harness.detection.present {
                    progress::success_text("detected")
                } else {
                    progress::label("not detected")
                };
                vec![
                    progress::title(&harness.display_name),
                    detected,
                    progress::label(format!("setup: {}", harness.setup)),
                ]
            })
            .collect(),
    ));
    text
}

fn render_harness_detail(harness: &HarnessHealth) -> String {
    let mut text = progress::report_title(&harness.display_name, Some("Harness integration"));
    text.push('\n');
    text.push_str(&progress::report_section("Detection"));
    text.push_str(&format!(
        "{}\n{}\n\n",
        progress::key_value("present", harness.detection.present.to_string()),
        progress::key_value(
            "version",
            harness.detection.version.as_deref().unwrap_or("unknown")
        )
    ));
    text.push_str(&progress::report_section("Setup"));
    text.push_str(&format!("  {}\n", harness.setup));
    if let Some(provisioning) = &harness.provisioning {
        text.push_str(&format!(
            "\nProvisioning\n  {:?} via {} ({:?})\n",
            provisioning.status, provisioning.method, provisioning.action
        ));
    }
    text
}

fn render_status(report: &StatusReport) -> String {
    let (headline, detail) = status_headline(report);
    let mut text =
        progress::report_title("Project status", Some(&report.root.display().to_string()));
    text.push('\n');
    text.push_str(&format!("{} {}\n", status_icon(report), headline));
    text.push_str(&format!("  {}\n", progress::label(detail)));

    text.push('\n');
    text.push_str(&progress::report_section("Context coverage"));
    for harness in &report.harnesses {
        text.push_str(&render_status_harness(harness));
    }

    text.push('\n');
    text.push_str(&progress::report_section("Project environment"));
    text.push_str(&format!(
        "{}\n{}\n",
        progress::key_value(
            "Installed",
            format!("{} on this machine", report.packages_installed)
        ),
        progress::key_value(
            "In this project",
            format!("{} contributing", report.packages_contributing_here)
        )
    ));
    text.push_str(&render_project_lock_status(&report.project_lock));
    text.push_str(&render_drift(&report.drift));

    let next_step = status_next_step(report);
    if !report.issues.is_empty() || next_step.is_some() {
        text.push('\n');
        text.push_str(&progress::report_section("Next step"));
        if let Some(next_step) = next_step {
            text.push_str(&format!("  {}\n", progress::accent(next_step)));
        }
        for issue in &report.issues {
            text.push_str(&format!("  {} {}\n", progress::warning_icon(), issue));
        }
    } else {
        text.push('\n');
        text.push_str(&progress::report_section("Health"));
        text.push_str(&format!("  {} no issues\n", progress::success_icon()));
    }
    text
}

fn status_headline(report: &StatusReport) -> (String, &'static str) {
    if !report.issues.is_empty() {
        return (
            progress::warning_heading("Needs attention"),
            "Some project context needs reconciliation.",
        );
    }
    if locked_plugin_count(&report.project_lock) > 0 {
        return (
            progress::warning_heading("Environment not installed"),
            "The project lock lists packages that are missing locally.",
        );
    }
    match &report.portability {
        Portability::Portable => (
            progress::success_heading("Ready"),
            "Project context is available to every detected harness.",
        ),
        _ => (
            progress::warning_heading("Needs attention"),
            "Review the project context before starting work.",
        ),
    }
}

fn status_icon(report: &StatusReport) -> String {
    if report.issues.is_empty() && locked_plugin_count(&report.project_lock) == 0 {
        progress::success_icon()
    } else {
        progress::warning_icon()
    }
}

/// What `agents.yaml` asks for that the rest of the chain has not caught
/// up to. Silent when there is nothing owed — a clean project says nothing
/// rather than saying "no drift", which is a sentence nobody needs.
fn render_drift(drift: &uze_application::application::EnvironmentDrift) -> String {
    if drift.is_clear() {
        return String::new();
    }
    let mut text = String::from("\n");
    text.push_str(&progress::report_section("Declared, not yet applied"));
    if !drift.unresolved.is_empty() {
        text.push_str(&format!(
            "  {} declared in agents.yaml, not resolved: {}\n",
            progress::warning_icon(),
            drift.unresolved.join(", ")
        ));
    }
    if !drift.surplus.is_empty() {
        text.push_str(&format!(
            "  {} in agents.lock, no longer declared: {}\n",
            progress::warning_icon(),
            drift.surplus.join(", ")
        ));
    }
    if !drift.missing.is_empty() {
        text.push_str(&format!(
            "  {} locked, absent from this machine: {}\n",
            progress::warning_icon(),
            drift.missing.join(", ")
        ));
    }
    if drift.stale_projection {
        text.push_str(&format!(
            "  {} AGENTS.md is behind the declared worktree policy\n",
            progress::warning_icon()
        ));
    }
    text.push_str(&format!(
        "  {}\n",
        progress::label("Run `uze install` to apply")
    ));
    text
}

fn render_status_harness(harness: &uze_application::application::HarnessContextStatus) -> String {
    let state = match &harness.delivery {
        HarnessContextDelivery::Native => progress::success_text("Native"),
        HarnessContextDelivery::NotDetected => progress::label("Not installed"),
        HarnessContextDelivery::Bridge {
            state: uze_application::AttachmentState::Matched,
            ..
        } => progress::success_text("Bridged"),
        HarnessContextDelivery::Bridge { needed: false, .. } => progress::label("Not needed"),
        HarnessContextDelivery::Bridge { .. } => progress::warning_text("Needs reconciliation"),
    };
    format!("  {:<16} {state}\n", harness.display_name)
}

fn locked_plugin_count(status: &uze_application::application::ProjectLockStatus) -> usize {
    match status {
        uze_application::application::ProjectLockStatus::Present { plugins } => {
            plugins.iter().filter(|plugin| !plugin.installed).count()
        }
        _ => 0,
    }
}

fn status_next_step(report: &StatusReport) -> Option<&'static str> {
    if locked_plugin_count(&report.project_lock) > 0 {
        Some("Run `uze install` to install the locked project packages.")
    } else if !report.issues.is_empty() || !matches!(report.portability, Portability::Portable) {
        Some("Run `uze context reconcile` to repair project context.")
    } else {
        None
    }
}

fn render_project_lock_status(status: &uze_application::application::ProjectLockStatus) -> String {
    use uze_application::application::ProjectLockStatus;
    match status {
        ProjectLockStatus::Absent => {
            let mut text = format!("\n{}", progress::report_section("Project lock"));
            text.push_str(&format!(
                "  {}\n",
                progress::label("No agents.lock in this project.")
            ));
            text
        }
        ProjectLockStatus::Malformed { reason } => {
            // The reason already names the file and what is wrong with it;
            // a prefix here only says it twice, and says "malformed" over
            // errors that are not.
            format!(
                "\n{}  {} {reason}\n",
                progress::report_section("Project lock"),
                progress::warning_icon()
            )
        }
        ProjectLockStatus::Present { plugins } => {
            let mut text = format!("\n{}", progress::report_section("Project lock"));
            if plugins.is_empty() {
                text.push_str(&format!(
                    "  {}\n",
                    progress::label("agents.lock has no plugins.")
                ));
            }
            for plugin in plugins {
                let state = if plugin.installed {
                    progress::success_text("installed")
                } else {
                    progress::warning_text("missing (run `uze install`)")
                };
                text.push_str(&format!("  {:<16} {state}\n", plugin.plugin));
            }
            text
        }
    }
}

fn render_context_status(status: &ProjectContextStatus) -> String {
    let mut text = progress::report_title(
        "Project context",
        Some(&status.canonical.display().to_string()),
    );
    text.push('\n');
    text.push_str(&progress::report_section("Sources"));
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
        text.push('\n');
        text.push_str(&progress::report_section("Contributions"));
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
    text.push('\n');
    text.push_str(&progress::report_section("Harnesses"));
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
        text.push_str(&format!("  {}  {delivery}\n", harness.display_name));
    }
    if let Some(worktrees) = &status.worktrees {
        text.push('\n');
        text.push_str(&progress::report_section("Worktree policy"));
        text.push_str(&format!(
            "  {}  region {:?}\n",
            worktrees.directory.display(),
            worktrees.state
        ));
        text.push_str(&format!(
            "  completion: {}\n",
            worktrees.completion.abi_name()
        ));
        for identity in &worktrees.superseded_regions {
            text.push_str(&format!("  {identity}  SUPERSEDED (a previous policy)\n"));
        }
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

/// Bridge rows show the human label (`app.integration_label`); the plan's
/// own keys stay the stable ids — which is what `--format json` emits.
fn render_context_plan(plan: &ContextPlan, app: &UzeApplication) -> String {
    let mut text =
        progress::report_title("Context plan", Some(&plan.agents_md.display().to_string()));
    text.push('\n');
    text.push_str(&progress::report_section("Changes"));
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
                app.health().integration_label(&bridge.integration),
                bridge.file.display(),
                render_action(&bridge.action)
            ));
        }
    }
    if let Some(region) = &plan.worktree_region {
        text.push_str("\nWorktree policy\n");
        text.push_str(&format!(
            "  {}  {}\n",
            region.file.display(),
            render_action(&region.action)
        ));
        for identity in &region.superseded {
            text.push_str(&format!("  {identity}  REMOVE (superseded)\n"));
        }
    }
    if plan.has_changes() {
        text.push_str("\nRun `uze context reconcile` to apply.\n");
    } else {
        text.push_str("\nNo changes: context is already reconciled.\n");
    }
    text
}

fn render_context_reconciliation(
    report: &ContextReconciliationReport,
    app: &UzeApplication,
) -> String {
    let mut text = progress::report_title(
        "Context reconciled",
        Some(&report.agents_md.display().to_string()),
    );
    text.push('\n');
    text.push_str(&progress::report_section("Packages"));
    for package in &report.packages {
        text.push_str(&format!("  {}  {:?}\n", package.package_id, package.state));
    }
    for orphan in &report.removed_orphans {
        text.push_str(&format!("  {orphan}  REMOVED (orphaned)\n"));
    }
    for (orphan, reason) in &report.blocked_orphans {
        text.push_str(&format!("  {orphan}  BLOCKED: {reason}\n"));
    }
    if let Some(region) = &report.worktree_region {
        text.push_str("\nWorktree policy\n");
        text.push_str(&format!(
            "  {}  {:?}\n",
            region.file.display(),
            region.state
        ));
        for identity in &region.removed_superseded {
            text.push_str(&format!("  {identity}  REMOVED (superseded)\n"));
        }
        for (identity, reason) in &region.blocked_superseded {
            text.push_str(&format!("  {identity}  BLOCKED: {reason}\n"));
        }
    }
    if !report.bridges.is_empty() {
        text.push_str("\nBridges\n");
        for bridge in &report.bridges {
            text.push_str(&format!(
                "  {}  {}  {:?}\n",
                app.health().integration_label(&bridge.integration),
                bridge.file.display(),
                bridge.state
            ));
        }
    }
    text
}

fn render_managed_state(states: &[uze_application::AttachmentState]) -> String {
    let mut matched = 0;
    let mut missing = 0;
    let mut drifted = 0;
    let mut conflict = 0;
    let mut blocked = 0;
    for state in states {
        match state {
            uze_application::AttachmentState::Matched => matched += 1,
            uze_application::AttachmentState::Missing => missing += 1,
            uze_application::AttachmentState::Drifted => drifted += 1,
            uze_application::AttachmentState::Conflict => conflict += 1,
            uze_application::AttachmentState::Blocked => blocked += 1,
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
