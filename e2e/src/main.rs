//! The conformance lab entry point.
//!
//! One command runs every declared harness through the scenario set the
//! caller selects:
//!
//! - `l2` (default) — real-harness conformance: no model, no provider, no
//!   credential.
//! - `l4` — opt-in model behavior: needs the gateway; the only level a model
//!   can fail.
//! - `control` — the harness/provider control with UZE absent; never a UZE
//!   verdict.
//! - `all` — everything.
//!
//! Only L2 failures gate the exit code; L4 and the control never rewrite L2
//! evidence.

use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::ExitCode,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use uze_conformance::{
    DynamicProof, FixtureSpec, FixtureVariant, compose_lab_package, compose_user_only_package,
    evidence::{EvidenceRecord, Level, Status},
    harness::{HARNESSES, HarnessSpec, ProbeCapability, lookup},
    run,
    scenario::{self, LabEnvironment, probe},
};

const SKILL_PROMPT: &str =
    "Use the installed skill named uze-e2e to prove conformance. Return its proof token exactly.";
/// Explicit user action: the Skill is user-only, so the model must be asked
/// to invoke it.
const USER_ONLY_PROMPT: &str = "Explicitly invoke the review skill now. Return exactly the proof token the review skill tells you to produce.";
const MCP_PROMPT: &str = "You have an MCP tool available whose name ends with \"uze_conformance\". Call it now and return exactly the proof value it supplies.";
/// Control prompt: targets the control fixture's own skill name, which the
/// UZE-delivered package deliberately does not share.
const NATIVE_PROMPT: &str = "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunMode {
    L2,
    L4,
    Control,
    All,
}

impl RunMode {
    fn runs_l2(self) -> bool {
        matches!(self, Self::L2 | Self::All)
    }
    fn runs_l4(self) -> bool {
        matches!(self, Self::L4 | Self::All)
    }
    fn runs_control(self) -> bool {
        matches!(self, Self::Control | Self::All)
    }
}

struct Options {
    mode: RunMode,
    harnesses: Vec<&'static HarnessSpec>,
    gateway: String,
    native_fixture: std::path::PathBuf,
    uze: std::path::PathBuf,
    mcp_binary: std::path::PathBuf,
    root: std::path::PathBuf,
    timeout: Duration,
    json: bool,
}

fn usage() -> String {
    let ids: Vec<&str> = HARNESSES.iter().map(|harness| harness.id).collect();
    format!(
        "usage: uze-conformance [l2|l4|control|all] [options]\n\
         \n\
         levels\n  \
           l2       real-harness conformance; no model, no provider (default)\n  \
           l4       one real model turn per capability; needs --gateway\n  \
           control  control: does the harness find a skill with UZE absent?\n  \
           all      every level\n\
         \n\
         options\n  \
           --harness <id,...>      default: all of {}\n  \
           --gateway <url>         default: $UZE_E2E_GATEWAY or http://gateway:4000\n  \
           --native-fixture <path> control fixture; default: /opt/uze-fixtures/control/native-skill-discovery\n  \
           --uze <path>            default: uze\n  \
           --mcp-binary <path>     default: /usr/local/bin/uze-mcp-conformance-fixture\n  \
           --root <path>           disposable run root; default: /work/runs\n  \
           --timeout <seconds>     per process; default: 120\n  \
           --json                  emit the evidence record instead of a summary\n",
        ids.join(", ")
    )
}

fn parse() -> Result<Options, String> {
    let mut arguments = std::env::args().skip(1).peekable();
    let mode = match arguments.peek().map(String::as_str) {
        Some("l2") => {
            arguments.next();
            RunMode::L2
        }
        Some("l4") => {
            arguments.next();
            RunMode::L4
        }
        Some("control") => {
            arguments.next();
            RunMode::Control
        }
        Some("all") => {
            arguments.next();
            RunMode::All
        }
        _ => RunMode::L2,
    };
    let mut options = Options {
        mode,
        harnesses: HARNESSES.iter().collect(),
        gateway: std::env::var("UZE_E2E_GATEWAY")
            .unwrap_or_else(|_| "http://gateway:4000".to_owned()),
        native_fixture: std::path::PathBuf::from(
            "/opt/uze-fixtures/control/native-skill-discovery",
        ),
        uze: std::path::PathBuf::from("uze"),
        mcp_binary: std::path::PathBuf::from("/usr/local/bin/uze-mcp-conformance-fixture"),
        root: std::path::PathBuf::from("/work/runs"),
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
            "--native-fixture" => options.native_fixture = std::path::PathBuf::from(value()?),
            "--uze" => options.uze = std::path::PathBuf::from(value()?),
            "--mcp-binary" => options.mcp_binary = std::path::PathBuf::from(value()?),
            "--root" => options.root = std::path::PathBuf::from(value()?),
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

/// One disposable installation: fresh HOME, UZE_HOME, workspace and fixture
/// copy, so no probe can observe state another probe created.
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

/// Creates the disposable directories and the declared environment. The
/// runner clears the ambient environment, so PATH is declared rather than
/// inherited: a harness resolved from an unexplained PATH is not
/// reproducible evidence. Provider config is only written for L4 routes.
fn prepare_environment(
    options: &Options,
    harness: &HarnessSpec,
    label: &str,
    with_provider: bool,
) -> Result<LabEnvironment, String> {
    let root = scenario::safe_root(&options.root, &format!("{}-{label}", harness.id))?;
    let home = root.join("home");
    let uze_home = root.join("uze-home");
    let workspace = root.join("project");
    for path in [&home, &uze_home, &workspace] {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }

    let mut environment = BTreeMap::new();
    environment.insert(
        "PATH".to_owned(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_owned()),
    );
    environment.insert("TERM".to_owned(), "dumb".to_owned());
    // Deterministic only: no probe may depend on a network catalog refresh.
    environment.insert("OPENCODE_DISABLE_MODELS_FETCH".to_owned(), "1".to_owned());

    if with_provider && let Some(config) = harness.l4.and_then(|spec| spec.provider_config) {
        for (from, to) in config.seed {
            copy_tree(Path::new(from), &home.join(to))?;
        }
        let path = home.join(config.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(
            &path,
            config.contents.replace("{gateway}", &options.gateway),
        )
        .map_err(|error| error.to_string())?;
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

fn run_uze(environment: &LabEnvironment, arguments: &[&str]) -> Result<String, String> {
    let result = run(&environment.spec(
        &environment.uze.to_string_lossy(),
        arguments.iter().map(|value| value.to_string()).collect(),
    ))
    .map_err(|error| error.to_string())?;
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if result.exit_code != Some(0) {
        return Err(format!(
            "uze {arguments:?} exited with {:?}: {}",
            result.exit_code, output
        ));
    }
    Ok(output)
}

/// Composes the canonical lab package from the single fixture source,
/// materializes per-run proofs, and installs it through the real `uze`
/// binary (`setup` + `plugin install`).
fn prepare_full(options: &Options, harness: &HarnessSpec, label: &str) -> Result<Run, String> {
    let environment = prepare_environment(options, harness, label, false)?;
    let stage = environment
        .workspace
        .parent()
        .ok_or("run root has no parent")?
        .join("package");
    compose_lab_package(&stage).map_err(|error| error.to_string())?;
    let payload = environment
        .workspace
        .parent()
        .ok_or("run root has no parent")?
        .join("payload");
    let proof = DynamicProof::from_nonce(&nonce());
    uze_conformance::materialize_fixture(
        &stage,
        &payload,
        &FixtureSpec {
            variant: FixtureVariant::Full,
            mcp_binary: options.mcp_binary.clone(),
            proof: proof.clone(),
        },
    )
    .map_err(|error| error.to_string())?;

    run_uze(&environment, &["setup", harness.uze_name])?;
    run_uze(
        &environment,
        &["plugin", "install", payload.to_str().unwrap_or_default()],
    )?;
    let package_id = manifest_name(&payload)?;
    Ok(Run {
        environment,
        package_id,
        proof,
    })
}

fn manifest_name(package_dir: &Path) -> Result<String, String> {
    let manifest =
        fs::read_to_string(package_dir.join("plugin.json")).map_err(|error| error.to_string())?;
    let parsed: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|error| error.to_string())?;
    parsed
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or("fixture plugin.json has no name".to_owned())
}

fn main() -> ExitCode {
    let options = match parse() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(64);
        }
    };
    let mut records: Vec<EvidenceRecord> = Vec::new();

    for harness in &options.harnesses {
        if options.mode.runs_l2() {
            records.extend(run_l2(&options, harness));
        }
        if options.mode.runs_l4() {
            records.extend(run_l4(&options, harness));
        }
        if options.mode.runs_control() {
            match run_control(&options, harness) {
                Ok(record) => records.push(record),
                Err(detail) => {
                    records.push(EvidenceRecord::new(
                        harness.id,
                        "C1-native-skill-control",
                        Level::Control,
                        "skill",
                        Status::InfraFailure,
                        "control: does the harness find a project-local skill with UZE absent?",
                        detail,
                        Duration::ZERO,
                    ));
                }
            }
        }
    }

    let l2_failed = records
        .iter()
        .any(|record| record.level == Level::L2 && !record.status.is_evidence());

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&records).expect("evidence serialization is infallible")
        );
    } else {
        for record in &records {
            println!(
                "{:<10} {:<8} {:<33} {:<19} {:<10} {:<8.2?} {}",
                record.harness,
                record.level.label(),
                record.scenario,
                record.capability,
                format!("{:?}", record.status),
                record.elapsed,
                record.evidence.lines().next().unwrap_or_default()
            );
            for line in record.evidence.lines().skip(1) {
                println!("             {line}");
            }
        }
        if records
            .iter()
            .any(|record| record.status == Status::ModelFailure)
        {
            println!(
                "\nL4 MODEL_FAILURE recorded: a model ran cleanly but did not exercise a\n\
                 capability. It never downgrades the L2 records above."
            );
        }
    }
    if l2_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_l2(options: &Options, harness: &HarnessSpec) -> Vec<EvidenceRecord> {
    let mut records = Vec::new();

    // R1/R3/R4/R7 — one canonical package installation exercises skill
    // discovery, package-native registration, MCP registration and the
    // Antigravity setup-idempotency regression.
    match prepare_full(options, harness, "r1r3r4") {
        Err(detail) => {
            for scenario in [
                "R1-canonical-skill-discovery",
                "R3-package-native-install",
                "R4-mcp-registration",
                "R7-repeated-setup-idempotency",
            ] {
                records.push(blocked(harness, scenario, detail.clone()));
            }
        }
        Ok(run) => {
            let name = scenario::discover_attachment_name(
                &run.environment,
                harness,
                &run.package_id,
                ProbeCapability::Skill,
            )
            .ok();
            records.push(scenario::l2_probe_record(
                &run.environment,
                harness,
                "R1-canonical-skill-discovery",
                "skill",
                "the real harness recognizes the UZE-delivered canonical Skill",
                harness
                    .probe_for(ProbeCapability::Skill)
                    .map(|p| (p, name.clone())),
            ));
            let package_name = scenario::discover_attachment_name(
                &run.environment,
                harness,
                &run.package_id,
                ProbeCapability::Package,
            )
            .ok();
            records.push(scenario::l2_probe_record(
                &run.environment,
                harness,
                "R3-package-native-install",
                "package",
                "the real harness reports the UZE-installed package in its own registry",
                harness
                    .probe_for(ProbeCapability::Package)
                    .map(|p| (p, package_name.clone())),
            ));
            let mcp_name = scenario::discover_attachment_name(
                &run.environment,
                harness,
                &run.package_id,
                ProbeCapability::Mcp,
            )
            .ok();
            records.push(scenario::l2_probe_record(
                &run.environment,
                harness,
                "R4-mcp-registration",
                "mcp",
                "the real harness registers (and where possible connects) the UZE-delivered MCP server",
                harness
                    .probe_for(ProbeCapability::Mcp)
                    .map(|p| (p, mcp_name.clone())),
            ));
            records.push(scenario::l2_idempotency_record(
                &run.environment,
                harness,
                &run.package_id,
            ));
        }
    }

    // R2 — invocation policy (real Codex prompt-input; others Unverified).
    match prepare_policy(options, harness) {
        Err(detail) if detail.contains("no shared-root entry") => {
            // Channel ≥ 0.149: `codex debug prompt-input` no longer
            // enumerates plugin-delivered skills, so no model-free policy
            // surface exists — record Unverified with the reason, never
            // InfraFailure (the Lab machinery is fine; the surface is gone).
            records.push(EvidenceRecord::new(
                harness.id,
                "R2-user-only-invocation-policy",
                Level::L2,
                "invocation-policy",
                Status::Unverified,
                "the real harness's model-visible prompt is honest about who may invoke (where a model-free policy surface exists)",
                detail,
                Duration::ZERO,
            ));
        }
        Err(detail) => records.push(blocked(harness, "R2-user-only-invocation-policy", detail)),
        Ok((environment, normal_name, user_only_name)) => records.push(scenario::l2_policy_record(
            &environment,
            harness,
            &normal_name,
            &user_only_name,
        )),
    }

    // G1 — the golden environment chain (L3 fixture → real uze → real harness).
    match prepare_golden(options, harness) {
        Err(detail) => records.push(blocked(harness, "G1-golden-environment", detail)),
        Ok(run) => records.push(scenario::l2_golden_record(
            &run.environment,
            harness,
            &run.package_id,
        )),
    }

    // R5 — remove/reinstall round-trip through the real harness.
    match prepare_full(options, harness, "r5") {
        Err(detail) => records.push(blocked(harness, "R5-lifecycle-remove-reinstall", detail)),
        Ok(run) => records.push(lifecycle_roundtrip(options, harness, &run)),
    }

    // R6 — runtime shim with the real harness (setup first: it creates the
    // shims under the run's UZE_HOME).
    match prepare_environment(options, harness, "r6", false) {
        Err(detail) => records.push(blocked(harness, "R6-runtime-shim", detail)),
        Ok(environment) => match run_uze(&environment, &["setup", harness.uze_name]) {
            Err(detail) => records.push(blocked(harness, "R6-runtime-shim", detail)),
            Ok(_) => {
                let shims_dir = environment.uze_home.join("shims");
                records.push(scenario::l2_shim_record(&environment, harness, &shims_dir));
            }
        },
    }

    records
}

fn blocked(harness: &HarnessSpec, scenario: &str, detail: String) -> EvidenceRecord {
    EvidenceRecord::new(
        harness.id,
        scenario,
        Level::L2,
        "setup",
        Status::InfraFailure,
        "the disposable installation itself could not be produced",
        detail,
        Duration::ZERO,
    )
}

/// R2 uses two canonical packages side by side: `flow` (default policy) and
/// the composed user-only `workflow` package.
fn prepare_policy(
    options: &Options,
    harness: &HarnessSpec,
) -> Result<(LabEnvironment, String, String), String> {
    let environment = prepare_environment(options, harness, "r2", false)?;
    run_uze(&environment, &["setup", harness.uze_name])?;
    let normal = uze_testkit::fixtures::canonical("flow");
    run_uze(
        &environment,
        &["plugin", "install", normal.to_str().unwrap_or_default()],
    )?;
    let stage = scenario::safe_root(&options.root, "r2-user-only")?;
    let user_only = stage.join("package");
    compose_user_only_package(&user_only).map_err(|error| error.to_string())?;
    run_uze(
        &environment,
        &["plugin", "install", user_only.to_str().unwrap_or_default()],
    )?;
    let entries = scenario::shared_entry_names(&environment, &["flow", "workflow"]);
    let normal_name = entries
        .iter()
        .find(|name| name.starts_with("flow"))
        .cloned()
        .ok_or("no shared-root entry for the default `flow` skill".to_owned())?;
    let user_only_name = entries
        .iter()
        .find(|name| name.starts_with("workflow"))
        .cloned()
        .ok_or("no shared-root entry for the user-only `workflow` skill".to_owned())?;
    Ok((environment, normal_name, user_only_name))
}

/// G1 — materialize the golden environment (the same fixture
/// `golden_environment_is_healthy` uses) and install it through the real
/// `uze` binary.
fn prepare_golden(options: &Options, harness: &HarnessSpec) -> Result<Run, String> {
    let golden = uze_testkit::fixtures::golden();
    let golden_market = golden.join("marketplace");
    let agents = fs::read_to_string(golden_market.join("agents.json"))
        .map_err(|error| format!("golden agents.json: {error}"))?;

    let environment = prepare_environment(options, harness, "golden", false)?;
    let market = environment
        .workspace
        .parent()
        .ok_or("run root has no parent")?
        .join("golden-market");
    fs::create_dir_all(&market).map_err(|error| error.to_string())?;
    fs::write(market.join("agents.json"), agents).map_err(|error| error.to_string())?;
    copy_tree(
        &golden_market.join("plugins/flow"),
        &market.join("plugins/flow"),
    )?;

    let lock = format!(
        "version: 1\nmarketplaces:\n  golden:\n    source:\n      type: path\n      path: {}\nplugins:\n  flow:\n    source:\n      type: marketplace\n      marketplace: golden\n      plugin: flow\n    resolved: {{}}\n",
        market.display()
    );
    fs::write(environment.workspace.join("agents.lock"), lock)
        .map_err(|error| error.to_string())?;
    fs::write(
        environment.workspace.join("AGENTS.md"),
        "# Golden project\n",
    )
    .map_err(|error| error.to_string())?;

    run_uze(
        &environment,
        &["market", "add", market.to_str().unwrap_or_default()],
    )?;
    run_uze(&environment, &["install"])?;
    let proof = DynamicProof::from_nonce(&nonce());
    Ok(Run {
        environment,
        package_id: "flow".to_owned(),
        proof,
    })
}

/// The capability the R5 probe (Skill-first selection) is keyed to.
fn probe_capability_for(
    harness: &HarnessSpec,
    probe: &uze_conformance::harness::Probe,
) -> ProbeCapability {
    if harness
        .probe_for(ProbeCapability::Skill)
        .is_some_and(|skill_probe| std::ptr::eq(skill_probe, probe))
    {
        ProbeCapability::Skill
    } else {
        ProbeCapability::Package
    }
}

/// R5 — remove/reinstall round-trip: the real harness must agree at every
/// phase.
fn lifecycle_roundtrip(options: &Options, harness: &HarnessSpec, run: &Run) -> EvidenceRecord {
    let started = Instant::now();
    let claim = "the real harness agrees at every phase of remove → reinstall";
    // The removal signal is the probe keyed to the *installed component*
    // (Skill listing for Claude/OpenCode, package registry for Codex and
    // Antigravity) — an always-present marketplace catalogue is not a
    // signal that the package is gone.
    let probe_spec = harness
        .probe_for(ProbeCapability::Skill)
        .or_else(|| harness.probe_for(ProbeCapability::Package));
    let mut phases = Vec::new();
    let mut status = Status::Pass;
    let name = probe_spec.as_ref().and_then(|probe| {
        scenario::discover_attachment_name(
            &run.environment,
            harness,
            &run.package_id,
            probe_capability_for(harness, probe),
        )
        .ok()
    });

    let present = probe_spec.map(|spec| probe(&run.environment, harness, spec, name.as_deref()));
    phases.push(format!(
        "installed: present={:?}",
        present.as_ref().is_some_and(|r| r.is_ok())
    ));
    if !present.as_ref().is_some_and(|r| r.is_ok()) {
        status = present
            .and_then(|result| result.err())
            .map(|(status, _)| status)
            .unwrap_or(Status::CapabilityFailure);
    } else {
        match run_uze(&run.environment, &["plugin", "remove", &run.package_id]) {
            Err(error) => {
                status = Status::HarnessFailure;
                phases.push(format!("remove failed: {error}"));
            }
            Ok(_) => {
                let after_remove =
                    probe_spec.map(|spec| probe(&run.environment, harness, spec, name.as_deref()));
                match after_remove {
                    Some(Ok(_)) => {
                        status = Status::CapabilityFailure;
                        phases.push("harness still reports the package after remove".to_owned());
                    }
                    Some(Err((_status, _reason))) => {
                        phases.push("removed: harness no longer reports the package".to_owned());
                    }
                    None => {
                        status = Status::Unverified;
                        phases.push("no probe for this surface".to_owned());
                    }
                }
            }
        }
        if status == Status::Pass {
            match prepare_full(options, harness, "r5-reinstall") {
                Err(error) => {
                    status = Status::HarnessFailure;
                    phases.push(format!("reinstall failed: {error}"));
                }
                Ok(again) => {
                    let after = probe_spec
                        .map(|spec| probe(&again.environment, harness, spec, name.as_deref()));
                    match after {
                        Some(Ok(_)) => phases.push("reinstalled: harness agrees".to_owned()),
                        Some(Err((s, reason))) => {
                            status = s;
                            phases.push(format!(
                                "harness does not report the reinstalled package: {reason}"
                            ));
                        }
                        None => {
                            status = Status::Unverified;
                            phases.push("no probe for this surface".to_owned());
                        }
                    }
                }
            }
        }
    }
    EvidenceRecord::new(
        harness.id,
        "R5-lifecycle-remove-reinstall",
        Level::L2,
        "lifecycle",
        status,
        claim,
        phases.join(" | "),
        started.elapsed(),
    )
}

fn run_l4(options: &Options, harness: &HarnessSpec) -> Vec<EvidenceRecord> {
    let mut records = Vec::new();
    let fail = |scenario: &str, capability: &str, detail: String| {
        EvidenceRecord::new(
            harness.id,
            scenario,
            Level::L4,
            capability,
            Status::InfraFailure,
            "a real model turn surfaces the capability proof the prompt never contains",
            detail,
            Duration::ZERO,
        )
    };
    // B1 — normal Skill, explicit model turn.
    match prepare_full(options, harness, "b1") {
        Err(detail) => records.push(fail("B1-normal-skill-model-behavior", "skill", detail)),
        Ok(run) => records.push(scenario::l4(
            &run.environment,
            harness,
            &options.gateway,
            SKILL_PROMPT,
            &run.proof.skill,
            "B1-normal-skill-model-behavior",
            "skill",
        )),
    }
    // B2 — user-only Skill, explicit user action.
    match prepare_policy(options, harness) {
        Err(detail) => records.push(fail(
            "B2-user-only-skill-model-behavior",
            "invocation-policy",
            detail,
        )),
        Ok((environment, _, _)) => {
            // The user-only skill's proof value is a per-run replacement of
            // `__UZE_SKILL_PROOF__`; `prepare_policy` installs the composed
            // package with the placeholder, so substitute it here.
            let proof = DynamicProof::from_nonce(&nonce());
            records.push(scenario::l4(
                &environment,
                harness,
                &options.gateway,
                USER_ONLY_PROMPT,
                &proof.skill,
                "B2-user-only-skill-model-behavior",
                "invocation-policy",
            ));
        }
    }
    // B3 — MCP proof-tool invocation.
    match prepare_full(options, harness, "b3") {
        Err(detail) => records.push(fail("B3-mcp-model-behavior", "mcp", detail)),
        Ok(run) => records.push(scenario::l4(
            &run.environment,
            harness,
            &options.gateway,
            MCP_PROMPT,
            &run.proof.mcp,
            "B3-mcp-model-behavior",
            "mcp",
        )),
    }
    records
}

/// The control: the harness finds a project-local skill with UZE absent.
fn run_control(options: &Options, harness: &HarnessSpec) -> Result<EvidenceRecord, String> {
    let environment = prepare_environment(options, harness, "control", true)?;
    let proof = DynamicProof::from_nonce(&nonce());
    uze_conformance::materialize_fixture(
        &options.native_fixture,
        &environment.workspace.join("native"),
        &FixtureSpec {
            variant: FixtureVariant::SkillOnly,
            mcp_binary: options.mcp_binary.clone(),
            proof: proof.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    let staged = environment.workspace.join("native/.agents");
    fs::rename(&staged, environment.workspace.join(".agents"))
        .map_err(|error| error.to_string())?;
    fs::remove_dir_all(environment.workspace.join("native")).map_err(|error| error.to_string())?;

    let mut record = scenario::l4(
        &environment,
        harness,
        &options.gateway,
        NATIVE_PROMPT,
        &proof.skill,
        "C1-native-skill-control",
        "skill",
    );
    record.level = Level::Control;
    Ok(record)
}
