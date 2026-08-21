//! The conformance lab entry point.
//!
//! One command runs every declared harness through the tiers the caller
//! selects. The default — `deterministic` — needs no network, no provider
//! credential and no model, so it is the tier pair a CI gate should run on
//! every change. `behavior` is opt-in because it is the only tier that costs
//! money and the only one a model can fail.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use uze_conformance::{
    DynamicProof, FixtureSpec, FixtureVariant,
    harness::{HARNESSES, HarnessSpec, lookup},
    tier::{self, LabEnvironment, TierReport},
};

const SKILL_PROMPT: &str = "Use the installed uze-plugin-first skill to prove plugin-first portability. Return its proof token exactly.";
/// Targets the native fixture's own skill name, which the UZE-delivered
/// package deliberately does not share.
const NATIVE_PROMPT: &str = "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually.";
const MCP_PROMPT: &str = "You have an MCP tool available whose name ends with \"uze_conformance\". Call it now and return exactly the proof value it supplies.";

struct Options {
    tiers: Tiers,
    harnesses: Vec<&'static HarnessSpec>,
    gateway: String,
    fixture: PathBuf,
    native_fixture: PathBuf,
    uze: PathBuf,
    mcp_binary: PathBuf,
    root: PathBuf,
    timeout: Duration,
    json: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tiers {
    Deterministic,
    Behavior,
    Baseline,
    All,
}

impl Tiers {
    fn runs_deterministic(self) -> bool {
        matches!(self, Self::Deterministic | Self::All)
    }

    fn runs_behavior(self) -> bool {
        matches!(self, Self::Behavior | Self::All)
    }

    fn runs_baseline(self) -> bool {
        matches!(self, Self::Baseline | Self::All)
    }
}

fn usage() -> String {
    let ids: Vec<&str> = HARNESSES.iter().map(|harness| harness.id).collect();
    format!(
        "usage: uze-conformance [deterministic|behavior|baseline|all] [options]\n\
         \n\
         tiers\n  \
           deterministic  attachment + discovery; no network, credential or model (default)\n  \
           behavior       one real model turn per capability; needs --gateway\n  \
           baseline       control: does the harness find a skill with UZE absent?\n  \
           all            every tier\n\
         \n\
         options\n  \
           --harness <id,...>   default: all of {}\n  \
           --gateway <url>      default: $UZE_E2E_GATEWAY or http://gateway:4000\n  \
           --fixture <path>     default: /opt/uze-fixtures/plugin-first-conformance\n  \
           --native-fixture <p> default: /opt/uze-fixtures/native-skill-discovery\n  \
           --uze <path>         default: uze\n  \
           --mcp-binary <path>  default: /usr/local/bin/uze-mcp-conformance-fixture\n  \
           --root <path>        disposable run root; default: /work/runs\n  \
           --timeout <seconds>  per process; default: 120\n  \
           --json               emit the evidence record instead of a summary\n",
        ids.join(", ")
    )
}

fn parse() -> Result<Options, String> {
    let mut arguments = std::env::args().skip(1).peekable();
    let tiers = match arguments.peek().map(String::as_str) {
        Some("deterministic") => {
            arguments.next();
            Tiers::Deterministic
        }
        Some("behavior") => {
            arguments.next();
            Tiers::Behavior
        }
        Some("baseline") => {
            arguments.next();
            Tiers::Baseline
        }
        Some("all") => {
            arguments.next();
            Tiers::All
        }
        _ => Tiers::Deterministic,
    };
    let mut options = Options {
        tiers,
        harnesses: HARNESSES.iter().collect(),
        gateway: std::env::var("UZE_E2E_GATEWAY")
            .unwrap_or_else(|_| "http://gateway:4000".to_owned()),
        fixture: PathBuf::from("/opt/uze-fixtures/plugin-first-conformance"),
        native_fixture: PathBuf::from("/opt/uze-fixtures/native-skill-discovery"),
        uze: PathBuf::from("uze"),
        mcp_binary: PathBuf::from("/usr/local/bin/uze-mcp-conformance-fixture"),
        root: PathBuf::from("/work/runs"),
        timeout: Duration::from_secs(120),
        json: false,
    };
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--json" => options.json = true,
            "--help" | "-h" => return Err(usage()),
            "--harness" => {
                let raw = value()?;
                let mut selected = Vec::new();
                for id in raw.split(',').map(str::trim).filter(|id| !id.is_empty()) {
                    selected.push(
                        lookup(id).ok_or_else(|| format!("unknown harness {id}\n\n{}", usage()))?,
                    );
                }
                options.harnesses = selected;
            }
            "--gateway" => options.gateway = value()?,
            "--fixture" => options.fixture = PathBuf::from(value()?),
            "--native-fixture" => options.native_fixture = PathBuf::from(value()?),
            "--uze" => options.uze = PathBuf::from(value()?),
            "--mcp-binary" => options.mcp_binary = PathBuf::from(value()?),
            "--root" => options.root = PathBuf::from(value()?),
            "--timeout" => {
                options.timeout = Duration::from_secs(
                    value()?
                        .parse()
                        .map_err(|error| format!("--timeout: {error}"))?,
                )
            }
            other => return Err(format!("unrecognized argument {other}\n\n{}", usage())),
        }
    }
    if options.harnesses.is_empty() {
        return Err("no harness selected".to_owned());
    }
    Ok(options)
}

/// One disposable installation. Each is a fresh HOME, UZE_HOME, workspace and
/// fixture copy, so no probe can observe state another probe created.
struct Run {
    environment: LabEnvironment,
    package_id: String,
    proof: DynamicProof,
}

fn nonce() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is available")
        .as_nanos();
    format!("{}_{stamp}", std::process::id())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Creates the disposable directories, the declared environment and any
/// provider config a Tier 3 route needs. Shared by every tier so all of them
/// observe the same isolation: a fresh HOME, UZE_HOME and workspace, and no
/// value inherited from the operator's shell.
fn prepare_environment(
    options: &Options,
    harness: &HarnessSpec,
    label: &str,
    with_provider: bool,
) -> Result<LabEnvironment, String> {
    let root = options.root.join(format!(
        "{}-{}-{}",
        harness.id,
        label.replace(':', "-"),
        nonce()
    ));
    let home = root.join("home");
    let uze_home = root.join("uze-home");
    let workspace = root.join("project");
    for path in [&home, &uze_home, &workspace] {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }

    // The runner clears the ambient environment, so PATH is declared rather
    // than inherited: a harness resolved from an unexplained PATH is not
    // reproducible evidence.
    let mut environment = BTreeMap::new();
    environment.insert(
        "PATH".to_owned(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_owned()),
    );
    environment.insert("TERM".to_owned(), "dumb".to_owned());

    if with_provider {
        if let Some(config) = harness.behavior.and_then(|spec| spec.provider_config) {
            for (from, to) in config.seed {
                copy_tree(Path::new(from), &home.join(to))?;
            }
            let path = home.join(config.relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(&path, config.contents.replace("{gateway}", &options.gateway))
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(LabEnvironment {
        uze: options.uze.clone(),
        mcp_binary: options.mcp_binary.clone(),
        home,
        uze_home,
        workspace,
        timeout: options.timeout,
        environment,
    })
}

/// The control for Tier 3. Places the native fixture directly in the caller's
/// workspace and never runs `uze` at all, so a pass means the harness finds
/// the skill on its own. Only a baseline *failure* plus a behavior *pass*
/// attributes the capability to UZE's delivery.
fn prepare_baseline(
    options: &Options,
    harness: &HarnessSpec,
) -> Result<(LabEnvironment, DynamicProof), String> {
    let lab = prepare_environment(options, harness, "baseline", true)?;
    let proof = DynamicProof::from_nonce(&nonce());
    uze_conformance::materialize_fixture(
        &options.native_fixture,
        &lab.workspace.join("native"),
        &FixtureSpec {
            variant: FixtureVariant::SkillOnly,
            mcp_binary: options.mcp_binary.clone(),
            proof: proof.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    // `materialize_fixture` refuses an existing destination, so the tree is
    // built beside the workspace root and then moved into place.
    let staged = lab.workspace.join("native/.agents");
    fs::rename(&staged, lab.workspace.join(".agents")).map_err(|error| error.to_string())?;
    fs::remove_dir_all(lab.workspace.join("native")).map_err(|error| error.to_string())?;
    Ok((lab, proof))
}

fn prepare(
    options: &Options,
    harness: &HarnessSpec,
    variant: FixtureVariant,
    label: &str,
    with_provider: bool,
) -> Result<Run, String> {
    let lab = prepare_environment(options, harness, label, with_provider)?;
    let source = lab
        .workspace
        .parent()
        .ok_or("run root has no parent")?
        .join("package");

    let proof = DynamicProof::from_nonce(&nonce());
    uze_conformance::materialize_fixture(
        &options.fixture,
        &source,
        &FixtureSpec {
            variant,
            mcp_binary: options.mcp_binary.clone(),
            proof: proof.clone(),
        },
    )
    .map_err(|error| error.to_string())?;

    let package_id = {
        let manifest = fs::read_to_string(source.join("plugin.json"))
            .map_err(|error| format!("fixture plugin.json: {error}"))?;
        let parsed: serde_json::Value =
            serde_json::from_str(&manifest).map_err(|error| error.to_string())?;
        parsed
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or("fixture plugin.json has no name")?
            .to_owned()
    };

    for arguments in [
        vec!["setup".to_owned(), harness.uze_name.to_owned()],
        vec!["add".to_owned(), source.to_string_lossy().into_owned()],
    ] {
        let label = arguments.join(" ");
        let result = uze_conformance::run(&uze_conformance::HarnessRunSpec {
            executable: lab.uze.clone(),
            arguments,
            environment: lab.environment.clone(),
            home: lab.home.clone(),
            uze_home: lab.uze_home.clone(),
            working_directory: lab.workspace.clone(),
            stdin: None,
            timeout: lab.timeout,
        })
        .map_err(|error| error.to_string())?;
        if result.exit_code != Some(0) {
            return Err(format!(
                "uze {label} exited with {:?}: {}{}",
                result.exit_code,
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
    }

    Ok(Run {
        environment: lab,
        package_id,
        proof,
    })
}

fn blocked(harness: &HarnessSpec, tier: &'static str, detail: String) -> TierReport {
    TierReport {
        harness: harness.id.to_owned(),
        tier,
        state: uze_conformance::EvidenceState::BlockedByEnvironment,
        attachments: Vec::new(),
        probes: Vec::new(),
        detail: Some(detail),
    }
}

fn main() -> ExitCode {
    let options = match parse() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(64);
        }
    };
    let mut reports: Vec<TierReport> = Vec::new();

    for harness in &options.harnesses {
        if options.tiers.runs_deterministic() {
            // Both capabilities are installed together here. There is no
            // model to confuse, so the realistic shape is also the safe one.
            match prepare(&options, harness, FixtureVariant::Full, "deterministic", false) {
                Err(detail) => reports.push(blocked(harness, "attachment", detail)),
                Ok(run) => {
                    let attachment =
                        tier::attachment(&run.environment, harness, &run.package_id);
                    let attachments = attachment.attachments.clone();
                    let verified = attachment.passed();
                    reports.push(attachment);
                    if verified {
                        reports.push(tier::discovery(&run.environment, harness, &attachments));
                    }
                }
            }
        }
        if options.tiers.runs_behavior() {
            // Two isolated installations, one capability each. See
            // `FixtureVariant`: sharing one installation lets the model
            // answer the skill prompt with the MCP tool's value, which is
            // what the retired per-harness prompt patches were fighting.
            for (variant, tier_name, prompt, pick) in [
                (
                    FixtureVariant::SkillOnly,
                    "behavior:skill",
                    SKILL_PROMPT,
                    (|proof: &DynamicProof| proof.skill.clone()) as fn(&DynamicProof) -> String,
                ),
                (
                    FixtureVariant::McpOnly,
                    "behavior:mcp",
                    MCP_PROMPT,
                    (|proof: &DynamicProof| proof.mcp.clone()) as fn(&DynamicProof) -> String,
                ),
            ] {
                match prepare(&options, harness, variant, tier_name, true) {
                    Err(detail) => {
                        let mut report = blocked(harness, "behavior", detail);
                        report.tier = tier_name;
                        reports.push(report);
                    }
                    Ok(run) => {
                        let mut report = tier::behavior(
                            &run.environment,
                            harness,
                            &options.gateway,
                            prompt,
                            &pick(&run.proof),
                        );
                        report.tier = tier_name;
                        reports.push(report);
                    }
                }
            }
        }
        if options.tiers.runs_baseline() {
            match prepare_baseline(&options, harness) {
                Err(detail) => {
                    let mut report = blocked(harness, "baseline", detail);
                    report.tier = "baseline:native";
                    reports.push(report);
                }
                Ok((environment, proof)) => {
                    let mut report = tier::behavior(
                        &environment,
                        harness,
                        &options.gateway,
                        NATIVE_PROMPT,
                        &proof.skill,
                    );
                    report.tier = "baseline:native";
                    reports.push(report);
                }
            }
        }
    }

    // A baseline result is never a pass/fail verdict on UZE, so it does not
    // gate the exit code. It measures a different surface than the behavior
    // tier: the baseline puts a skill in the project's own `.agents/skills/`,
    // while UZE attaches at user scope out of the Store and leaves the
    // project untouched. So a baseline pass does not weaken a behavior pass —
    // the behavior workspace contains no skill file at all, so only the
    // user-scope delivery can have supplied it. What the baseline gives is
    // context for reading a behavior *failure*, and evidence of how much
    // UZE's route is doing: a harness that cannot discover a project-local
    // skill has no route to that capability except the one UZE provides.
    let failed = reports
        .iter()
        .any(|report| !report.tier.starts_with("baseline") && !report.passed());
    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports).expect("evidence serialization is infallible")
        );
    } else {
        for report in &reports {
            println!(
                "{:<10} {:<16} {:?}{}",
                report.harness,
                report.tier,
                report.state,
                report
                    .detail
                    .as_deref()
                    .map(|detail| format!("\n           {detail}"))
                    .unwrap_or_default()
            );
        }
        let (native, no_native): (Vec<_>, Vec<_>) = reports
            .iter()
            .filter(|report| report.tier.starts_with("baseline"))
            .partition(|report| report.passed());
        if !native.is_empty() || !no_native.is_empty() {
            println!("\nbaseline — project-local `.agents/skills/` discovery, UZE absent:");
            for report in &native {
                println!(
                    "  {:<10} finds a project-local skill on its own",
                    report.harness
                );
            }
            for report in &no_native {
                println!(
                    "  {:<10} does NOT — UZE's user-scope route is its only path to that capability",
                    report.harness
                );
            }
            println!(
                "  This measures a different surface than the behavior tier, whose workspace"
            );
            println!("  holds no skill file at all. It does not weaken a behavior pass.");
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_selection_controls_which_tiers_run() {
        assert!(Tiers::Deterministic.runs_deterministic());
        assert!(!Tiers::Deterministic.runs_behavior());
        assert!(Tiers::Behavior.runs_behavior());
        assert!(!Tiers::Behavior.runs_deterministic());
        assert!(Tiers::All.runs_deterministic() && Tiers::All.runs_behavior());
    }

    #[test]
    fn usage_lists_every_declared_harness() {
        let text = usage();
        for harness in HARNESSES {
            assert!(text.contains(harness.id), "usage omits {}", harness.id);
        }
    }
}
