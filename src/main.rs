//! CLI composition root. See ADR-005 for the no-launcher, peer-integration boundary.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
mod integrations;
use uze::{Result, UzeEngine, UzeHome, UzeStore, build_report, importer, report::render_text};

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
            let store = UzeStore::new(home);
            let environment = UzeEngine::new(store).compose_project(project)?;
            let claude = ClaudeIntegration;
            let codex = CodexIntegration;
            let opencode = OpenCodeIntegration;
            let integrations: [&dyn uze::integration::IntegrationPort; 3] =
                [&claude, &codex, &opencode];
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
            let store = UzeStore::new(UzeHome::from_env()?);
            let installed = store.install_agent_plugin(package)?;
            match format {
                OutputFormat::Text => {
                    println!("Installed Agent Plugin package: {}", installed.id.as_str());
                    println!("Store path: {}", installed.root.display());
                }
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "package_id": installed.id.as_str(),
                        "store_path": installed.root,
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
    }
    Ok(())
}
