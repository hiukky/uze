//! CLI composition root. See ADR-005 for the no-launcher, peer-integration boundary.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
mod integrations;
use uze::{
    Result, UzeEngine, UzeHome, UzeStore, build_report, importer,
    integration::{IntegrationPort, IntegrationStatus},
    report::render_text,
};

use integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, opencode::OpenCodeIntegration,
};

#[derive(Debug, Parser)]
#[command(
    name = "uze",
    version,
    about = "Resolve a standards-first agent project environment"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Resolve a project-owned portable core and report optional enhancements.
    Inspect {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Install one local Agent Plugins 1.0 package into the UZE-owned store.
    Add {
        package: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Explicitly import a declarative plugin bundle as a compatibility fallback.
    ImportBundle {
        bundle: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Machine-level, idempotent integration setup. Omit `harness` to set up
    /// every detected harness; pass `claude` or `codex` to set up one.
    Setup { harness: Option<String> },
    /// Read-only report of UZE_HOME/Store readiness and per-harness
    /// integration state. Prints no credential material.
    Doctor,
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
    match cli.command {
        Command::Inspect { project, format } => {
            let home = UzeHome::from_env()?;
            let store = UzeStore::new(home.clone());
            let environment = UzeEngine::new(store).compose_project(project)?;
            let claude = ClaudeIntegration::from_env(home.clone())?;
            let codex = CodexIntegration::from_env(home)?;
            let opencode = OpenCodeIntegration;
            let integrations: [&dyn IntegrationPort; 3] = [&claude, &codex, &opencode];
            let report = build_report(&environment, &integrations);
            match format {
                OutputFormat::Text => print!("{}", render_text(&report)),
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .expect("report serialization is infallible")
                ),
            }
        }
        Command::Add { package, format } => {
            let home = UzeHome::from_env()?;
            let store = UzeStore::new(home.clone());
            let installed = store.install_agent_plugin(package)?;
            let environment = UzeEngine::new(store).compose(std::slice::from_ref(&installed.id))?;

            let claude = ClaudeIntegration::from_env(home.clone())?;
            let codex = CodexIntegration::from_env(home)?;
            let integrations: [(&str, &dyn IntegrationPort); 2] =
                [(claude.id(), &claude), (codex.id(), &codex)];
            let mut attached = Vec::new();
            for resource in &environment.resources {
                for (label, integration) in integrations {
                    if let Some(path) = integration.attach(resource)? {
                        attached.push((label.to_owned(), path));
                    }
                }
            }

            match format {
                OutputFormat::Text => {
                    println!("Installed Agent Plugin package: {}", installed.id.as_str());
                    println!("Store path: {}", installed.root.display());
                    for (harness, path) in &attached {
                        println!("Attached to {harness}: {}", path.display());
                    }
                }
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "package_id": installed.id.as_str(),
                        "store_path": installed.root,
                        "attached": attached.iter().map(|(harness, path)| serde_json::json!({
                            "harness": harness,
                            "path": path,
                        })).collect::<Vec<_>>(),
                    }))
                    .expect("installation serialization is infallible")
                ),
            }
        }
        Command::ImportBundle { bundle, format } => {
            let imported = importer::import_bundle(bundle)?;
            match format {
                OutputFormat::Text => {
                    println!("Compatibility fallback import: {}", imported.root.display());
                    println!("Manifest: {}", imported.manifest.display());
                    println!("Standard items: {}", imported.standard_items.len());
                    println!(
                        "Optional enhancements: {}",
                        imported.optional_enhancements.len()
                    );
                }
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&imported)
                        .expect("bundle serialization is infallible")
                ),
            }
        }
        Command::Setup { harness } => {
            let home = UzeHome::from_env()?;
            home.ensure_layout()?;
            let claude = ClaudeIntegration::from_env(home.clone())?;
            let codex = CodexIntegration::from_env(home.clone())?;
            let selected: Vec<(&str, &dyn IntegrationPort)> = match harness.as_deref() {
                Some("claude") => vec![(claude.id(), &claude)],
                Some("codex") => vec![(codex.id(), &codex)],
                Some(other) => {
                    eprintln!("uze: unknown harness `{other}` (expected `claude` or `codex`)");
                    return Ok(());
                }
                None => vec![(claude.id(), &claude), (codex.id(), &codex)],
            };
            for (label, integration) in selected {
                let detection = integration.detect();
                if !detection.present {
                    println!("{label}: not detected, skipping setup");
                    continue;
                }
                integration.install(&home)?;
                println!(
                    "{label}: ready (version {})",
                    detection.version.as_deref().unwrap_or("unknown")
                );
            }
        }
        Command::Doctor => {
            let home = UzeHome::from_env()?;
            println!("UZE_HOME       {}", home.root().display());
            println!(
                "Store          {}",
                if home.registry_path().is_file() {
                    "ready"
                } else {
                    "empty"
                }
            );
            println!();
            println!("Integrations");
            let claude = ClaudeIntegration::from_env(home.clone())?;
            let codex = CodexIntegration::from_env(home.clone())?;
            let integrations: [(&str, &dyn IntegrationPort); 2] =
                [(claude.id(), &claude), (codex.id(), &codex)];
            for (label, integration) in integrations {
                let status = match integration.status(&home) {
                    IntegrationStatus::NotConfigured => "not configured",
                    IntegrationStatus::InstalledUnverified => "installed / unverified",
                    IntegrationStatus::InstalledVerified => "installed / verified",
                };
                println!("{label:<14} {status}");
            }
        }
    }
    Ok(())
}
