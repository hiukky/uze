use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use uze::{Result, build_report, import_bundle, project::resolve_project, report::render_text};

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
            let report = build_report(&resolve_project(project)?);
            match format {
                OutputFormat::Text => print!("{}", render_text(&report)),
                OutputFormat::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .expect("report serialization is infallible")
                ),
            }
        }
        Command::ImportBundle { bundle, format } => {
            let imported = import_bundle(bundle)?;
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
