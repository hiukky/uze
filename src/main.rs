//! Thin CLI presentation over `UzeApplication`.

use std::{io::IsTerminal, path::PathBuf};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use uze::{
    Result, UzeApplication, UzeHome,
    application::{DoctorReport, PluginInspection, RemovePluginReport},
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
    /// Install one local Agent Plugins package and expose it where safe.
    Add {
        source: String,
        /// Authorize any executable capability the package declares without
        /// prompting. Named for what it grants rather than as a generic
        /// `--yes`, because it answers one specific security question.
        #[arg(long)]
        trust: bool,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// List locally installed plugins.
    List {
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Inspect one installed plugin and its delivery facts.
    Inspect {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Prepare detected harness integrations.
    Setup { harness: Option<String> },
    /// Safely detach a plugin only when its receipts still match.
    Remove {
        plugin: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Deterministic Store, harness, and attachment diagnostics.
    Doctor {
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
    if let Err(error) = run(Cli::parse()) {
        eprintln!("uze: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let home = UzeHome::from_env()?;
    let Some(command) = cli.command else {
        if std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
            return uze::tui::run(home);
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
            for result in app.setup(harness.as_deref())? {
                if result.configured {
                    println!(
                        "{}: ready (version {})",
                        result.integration,
                        result.detection.version.as_deref().unwrap_or("unknown")
                    );
                } else {
                    println!("{}: not detected, skipping setup", result.integration);
                }
            }
        }
        Command::Remove { plugin, format } => {
            let report = app.remove_plugin(&plugin)?;
            match format {
                OutputFormat::Text => print!("{}", render_remove(&report)),
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
    }
    Ok(())
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
