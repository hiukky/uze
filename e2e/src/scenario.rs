//! L2/L4/CONTROL scenario runners.
//!
//! Each runner produces exactly one [`EvidenceRecord`] per harness per
//! scenario, with the level flat on the record. The Lab's L2 scenarios ask
//! only questions a real harness can answer without a model; L4 scenarios
//! are the only place a model may fail; the control scenario measures the
//! harness/provider path with UZE absent and is never a UZE verdict.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::evidence::{EvidenceRecord, Level, Status, excerpt, provider_failure};
use crate::{
    HarnessRunResult, HarnessRunSpec,
    harness::{HarnessSpec, Probe, ProbeCapability},
    run,
};

/// The canonical fixture skill's proof token. The compositor replaces it per
/// run (see [`crate::fixture::compose_lab_package`]), so evidence is never
/// attributable to stale bytes.
pub const SKILL_PROOF_TOKEN: &str = "UZE_E2E_SKILL_PROOF_20260820";

/// What every spawned process receives on top of HOME/UZE_HOME. The runner
/// clears the ambient environment, so anything a harness needs must be
/// declared rather than inherited from the operator's shell.
#[derive(Clone, Debug)]
pub struct LabEnvironment {
    pub uze: PathBuf,
    pub home: PathBuf,
    pub uze_home: PathBuf,
    pub workspace: PathBuf,
    pub timeout: Duration,
    /// Resolved path of the fixture MCP server, so a probe can identify this
    /// package through the binary a harness reports rather than through a
    /// name the harness may have rewritten.
    pub mcp_binary: PathBuf,
    pub environment: BTreeMap<String, String>,
}

impl LabEnvironment {
    pub fn spec(&self, executable: &str, arguments: Vec<String>) -> HarnessRunSpec {
        HarnessRunSpec {
            executable: PathBuf::from(executable),
            arguments,
            environment: self.environment.clone(),
            home: self.home.clone(),
            uze_home: self.uze_home.clone(),
            working_directory: self.workspace.clone(),
            stdin: None,
            timeout: self.timeout,
        }
    }

    /// The declared `PATH` for spawned processes.
    pub fn path(&self) -> &str {
        self.environment
            .get("PATH")
            .map(String::as_str)
            .unwrap_or("/usr/local/bin:/usr/bin:/bin")
    }
}

/// Builds a fresh disposable root under `root`, panicking if the path could
/// overlap the developer's real home (the same guard `TestEnvironment`
/// uses; the Lab must fail early rather than touch operator state).
pub fn safe_root(root: &Path, label: &str) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = root.join(format!("{label}-{}-{}", std::process::id(), stamp));
    uze_testkit::temp::assert_not_real_home(&path);
    let root = root.to_path_buf();
    std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok(root.join(path.file_name().unwrap_or_default()))
}

fn combined_output(result: &HarnessRunResult) -> String {
    let mut text = String::from_utf8_lossy(&result.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&result.stderr));
    text
}

/// Runs a harness subcommand and classifies the outcome. Returns
/// `Ok(evidence)` when the probe's claim held, `Err((status, reason))`
/// otherwise — with the status as specific as the failure allows.
pub fn probe(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    capability_probe: &Probe,
    attachment_name: Option<&str>,
) -> Result<String, (Status, String)> {
    let arguments: Vec<String> = capability_probe
        .arguments
        .iter()
        .map(|value| value.to_string())
        .collect();
    let result = run(&environment.spec(harness.executable, arguments.clone()))
        .map_err(|error| (Status::InfraFailure, error.to_string()))?;
    let output = combined_output(&result);
    if result.timed_out {
        return Err((
            Status::InfraFailure,
            format!("timed out ({:?}): {}", result.elapsed, excerpt(&output)),
        ));
    }
    if let Some(reason) = provider_failure(&output) {
        return Err((
            Status::ProviderFailure,
            format!("{reason}: {}", excerpt(&output)),
        ));
    }
    let mut missing = Vec::new();
    if capability_probe.matches_attached_name {
        match attachment_name {
            None => {
                return Err((
                    Status::InfraFailure,
                    "the probe requires the attached name, but none could be determined".to_owned(),
                ));
            }
            Some(name) if !output.contains(name) => {
                missing.push(format!("attached name {name}"));
            }
            Some(_) => {}
        }
    }
    for fragment in capability_probe.required {
        let fragment = fragment.replace("{mcp_binary}", &environment.mcp_binary.to_string_lossy());
        if !output.contains(&fragment) {
            missing.push(format!("required fragment {fragment}"));
        }
    }
    match result.exit_code {
        Some(0) if missing.is_empty() => Ok(excerpt(&output)),
        Some(0) => Err((
            Status::CapabilityFailure,
            format!(
                "missing {}; output: {}",
                missing.join(", "),
                excerpt(&output)
            ),
        )),
        Some(code) => Err((
            Status::HarnessFailure,
            format!("exit {code}: {}", excerpt(&output)),
        )),
        None => Err((
            Status::HarnessFailure,
            format!("no exit status: {}", excerpt(&output)),
        )),
    }
}

/// Reads `uze inspect --format json` and returns the probe-able name UZE
/// reports for this integration's attachment to `package_id`. This is input
/// to probes, never Lab evidence: the main suite owns the reconciliation
/// contract (a receipt that does not match fails the L3 suite first).
/// The artifact tag a probe should ask about, per capability. OpenCode's
/// Skill proof is the SYMLINK_REFERENCE in the shared root, while its MCP
/// proof is the VENDOR_CONFIG_ENTRY — the same package can carry both, and
/// picking the first receipt would probe the wrong surface.
pub fn preferred_tag(capability: ProbeCapability) -> &'static [&'static str] {
    match capability {
        ProbeCapability::Skill => &["SYMLINK_REFERENCE", "INTEGRATION_OWNED"],
        ProbeCapability::Mcp => &["VENDOR_CONFIG_ENTRY", "INTEGRATION_OWNED"],
        ProbeCapability::Package => &["INTEGRATION_OWNED"],
    }
}

pub fn discover_attachment_name(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    package_id: &str,
    capability: ProbeCapability,
) -> Result<String, String> {
    let uze = environment.uze.to_string_lossy().into_owned();
    let result = run(&environment.spec(
        &uze,
        vec![
            "plugin".to_owned(),
            "inspect".to_owned(),
            package_id.to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ],
    ));
    let result = result.map_err(|error| error.to_string())?;
    if result.exit_code != Some(0) {
        return Err(format!(
            "uze inspect exited with {:?}: {}",
            result.exit_code,
            excerpt(&combined_output(&result))
        ));
    }
    let document: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|error| format!("inspect JSON: {error}"))?;
    let receipts = document
        .get("reconciliation")
        .and_then(|value| value.get("receipts"))
        .and_then(serde_json::Value::as_array)
        .ok_or("uze inspect JSON has no reconciliation.receipts array")?;
    for entry in receipts {
        let receipt = entry.get("receipt");
        let integration = receipt
            .and_then(|value| value.get("integration"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if integration != harness.uze_name {
            continue;
        }
        let Some((tag, body)) = receipt
            .and_then(|value| value.get("artifact"))
            .and_then(serde_json::Value::as_object)
            .and_then(|artifact| artifact.iter().next())
        else {
            continue;
        };
        if !preferred_tag(capability).contains(&tag.as_str()) {
            continue;
        }
        let name = match tag.as_str() {
            "VENDOR_CONFIG_ENTRY" => body
                .get("entry_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            "SYMLINK_REFERENCE" => body
                .get("path")
                .and_then(serde_json::Value::as_str)
                .and_then(|path| {
                    Path::new(path)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                }),
            "INTEGRATION_OWNED" => body
                .get("selector")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            _ => None,
        };
        if let Some(name) = name {
            return Ok(name);
        }
    }
    Err(format!(
        "UZE reports no probe-able attachment for integration {}",
        harness.uze_name
    ))
}

/// `(key, inspection state)` pairs for this integration's receipts, read
/// from `plugin inspect --format json`.
pub fn attachment_states(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    package_id: &str,
) -> Vec<(String, String)> {
    let uze = environment.uze.to_string_lossy().into_owned();
    let Ok(result) = run(&environment.spec(
        &uze,
        vec![
            "plugin".to_owned(),
            "inspect".to_owned(),
            package_id.to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ],
    )) else {
        return Vec::new();
    };
    let Ok(document) = serde_json::from_slice::<serde_json::Value>(&result.stdout) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Some(receipts) = document
        .get("reconciliation")
        .and_then(|value| value.get("receipts"))
        .and_then(serde_json::Value::as_array)
    else {
        return out;
    };
    for entry in receipts {
        let receipt = entry.get("receipt");
        let integration = receipt
            .and_then(|value| value.get("integration"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if integration != harness.uze_name {
            continue;
        }
        let state = entry
            .get("inspection")
            .and_then(|value| value.get("state"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_owned();
        let name = receipt
            .and_then(|value| value.get("package_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(package_id)
            .to_owned();
        out.push((name, state));
    }
    out
}

/// Runs one L2 probe for `capability` if the harness declares one; records
/// `Unverified` when it does not.
pub fn l2_probe_record(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    scenario: &str,
    capability: &str,
    claim: &str,
    capability_probe: Option<(&Probe, Option<String>)>,
) -> EvidenceRecord {
    let started = Instant::now();
    let Some((capability_spec, attachment_name)) = capability_probe else {
        return EvidenceRecord::new(
            harness.id,
            scenario,
            Level::L2,
            capability,
            Status::Unverified,
            &format!(
                "{claim} — {} declares no model-free probe for this surface",
                harness.id
            ),
            "no deterministic probe exists".to_owned(),
            started.elapsed(),
        );
    };
    match probe(
        environment,
        harness,
        capability_spec,
        attachment_name.as_deref(),
    ) {
        Ok(evidence) => EvidenceRecord::new(
            harness.id,
            scenario,
            Level::L2,
            capability,
            Status::Pass,
            claim,
            evidence,
            started.elapsed(),
        ),
        Err((status, reason)) => EvidenceRecord::new(
            harness.id,
            scenario,
            Level::L2,
            capability,
            status,
            claim,
            reason,
            started.elapsed(),
        ),
    }
}

/// The shared-root `.agents/skills` entries matching `prefixes` (e.g.
/// `"flow"` → `flow:commit`) — the exact strings Codex's prompt input lists.
pub fn shared_entry_names(environment: &LabEnvironment, prefixes: &[&str]) -> Vec<String> {
    let root = environment.home.join(".agents/skills");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if prefixes.iter().any(|prefix| name.starts_with(prefix)) {
            names.push(name);
        }
    }
    names.sort();
    names
}

/// R2 — invocation policy. Renders the model-visible prompt input with `codex
/// debug prompt-input` (zero model calls) and asserts the default Skill is
/// visible while the user-only Skill is not. Harnesses without a model-free
/// policy surface record `Unverified`.
pub fn l2_policy_record(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    normal_name: &str,
    user_only_name: &str,
) -> EvidenceRecord {
    let started = Instant::now();
    let claim = "the real harness's model-visible prompt includes the default Skill and excludes the user-only Skill";
    let Some(policy) = harness.policy else {
        return EvidenceRecord::new(
            harness.id,
            "R2-user-only-invocation-policy",
            Level::L2,
            "invocation-policy",
            Status::Unverified,
            claim,
            "no model-free policy surface exists for this harness".to_owned(),
            started.elapsed(),
        );
    };
    let result = run(&environment.spec(
        harness.executable,
        policy.arguments.iter().map(|s| s.to_string()).collect(),
    ))
    .map_err(|error| (Status::InfraFailure, error.to_string()));
    let (evidence, status) = match result {
        Err((status, reason)) => (reason, status),
        Ok(result) => {
            let output = combined_output(&result);
            if result.timed_out {
                (excerpt(&output), Status::InfraFailure)
            } else if let Some(reason) = provider_failure(&output) {
                (
                    format!("{reason}: {}", excerpt(&output)),
                    Status::ProviderFailure,
                )
            } else if result.exit_code != Some(0) {
                (excerpt(&output), Status::HarnessFailure)
            } else if !output.contains(normal_name) {
                (
                    format!(
                        "default Skill {normal_name} missing from prompt input: {}",
                        excerpt(&output)
                    ),
                    Status::CapabilityFailure,
                )
            } else if output.contains(user_only_name) {
                (
                    format!(
                        "user-only Skill {user_only_name} is model-visible: {}",
                        excerpt(&output)
                    ),
                    Status::CapabilityFailure,
                )
            } else {
                (excerpt(&output), Status::Pass)
            }
        }
    };
    EvidenceRecord::new(
        harness.id,
        "R2-user-only-invocation-policy",
        Level::L2,
        "invocation-policy",
        status,
        claim,
        evidence,
        started.elapsed(),
    )
}

/// R6 — runtime shim boundary with the real harness. After `uze setup`,
/// UZE's own shim sits on PATH ahead of the real executable; running the
/// harness version probe through that PATH must still reach the real CLI.
pub fn l2_shim_record(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    shims_dir: &Path,
) -> EvidenceRecord {
    let started = Instant::now();
    let claim = "the real harness still resolves through UZE's runtime shim and answers with its own version";
    let Some(arguments) = harness.shim_probe else {
        return EvidenceRecord::new(
            harness.id,
            "R6-runtime-shim",
            Level::L2,
            "runtime-shim",
            Status::Unverified,
            claim,
            "no shim probe declared".to_owned(),
            started.elapsed(),
        );
    };
    let arguments: Vec<String> = arguments.iter().map(|s| s.to_string()).collect();
    let mut through_spec = environment.spec(harness.executable, arguments.clone());
    let mut parts = vec![shims_dir.to_path_buf()];
    parts.extend(std::env::split_paths(environment.path()));
    through_spec.environment.insert(
        "PATH".to_owned(),
        std::env::join_paths(parts)
            .unwrap_or_else(|error| panic!("could not join shim PATH: {error}"))
            .to_string_lossy()
            .into_owned(),
    );
    let through_shim = run(&through_spec);
    let direct = run(&environment.spec(harness.executable, arguments.clone()));
    let (evidence, status) = match (through_shim, direct) {
        (Err(error), _) => (error.to_string(), Status::InfraFailure),
        (Ok(result), Err(_)) if result.timed_out => (
            format!(
                "shim probe timed out: {}",
                excerpt(&combined_output(&result))
            ),
            Status::InfraFailure,
        ),
        (Ok(result), Err(_)) => (excerpt(&combined_output(&result)), Status::HarnessFailure),
        (Ok(result), Ok(reference)) => {
            if result.exit_code == Some(0)
                && combined_output(&result) == combined_output(&reference)
            {
                (excerpt(&combined_output(&result)), Status::Pass)
            } else {
                (
                    format!(
                        "shim run differs from direct run: shim={:?} direct={:?}: {}",
                        result.exit_code,
                        reference.exit_code,
                        excerpt(&combined_output(&result))
                    ),
                    Status::CapabilityFailure,
                )
            }
        }
    };
    EvidenceRecord::new(
        harness.id,
        "R6-runtime-shim",
        Level::L2,
        "runtime-shim",
        status,
        claim,
        evidence,
        started.elapsed(),
    )
}

/// R7 — repeated-setup idempotency against a truthful vendor CLI. Only
/// Antigravity has a staged-copy install whose preflight refuses an existing
/// same-name import, which is exactly the surface `attach_package_to`'s
/// idempotency guard protects. Harnesses with merge-style installs record
/// `Skipped` (the scenario is not theirs).
pub fn l2_idempotency_record(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    package_id: &str,
) -> EvidenceRecord {
    let started = Instant::now();
    let claim = "re-running `uze setup` after a Matched package receipt performs no native re-install and the real harness stays consistent";
    if harness.id != "antigravity" {
        return EvidenceRecord::new(
            harness.id,
            "R7-repeated-setup-idempotency",
            Level::L2,
            "lifecycle",
            Status::Skipped,
            claim,
            "scenario targets Antigravity's staged-copy install semantics".to_owned(),
            started.elapsed(),
        );
    }
    let first = vendor_import_names(environment, harness);
    let re_run = run(&environment.spec(
        &environment.uze.to_string_lossy(),
        vec!["setup".to_owned(), harness.uze_name.to_owned()],
    ));
    let second = vendor_import_names(environment, harness);
    let inspect = attachment_states(environment, harness, package_id);
    let (evidence, status) = match (re_run, first, second, inspect) {
        (Err(error), _, _, _) => (error.to_string(), Status::InfraFailure),
        (Ok(result), _, _, _) if result.timed_out => (
            "second `uze setup` timed out".to_owned(),
            Status::InfraFailure,
        ),
        (Ok(result), _, _, _) if result.exit_code != Some(0) => (
            format!(
                "second `uze setup` exited with {:?}: {}",
                result.exit_code,
                excerpt(&combined_output(&result))
            ),
            Status::HarnessFailure,
        ),
        (Ok(_), Err(error), _, _) => (error, Status::InfraFailure),
        (Ok(_), Ok(_), Err(error), _) => (error, Status::InfraFailure),
        (Ok(_), Ok(first), Ok(second), _) if second != first => (
            format!(
                "agy import manifest changed between setups: before={first:?} after={second:?}"
            ),
            Status::CapabilityFailure,
        ),
        (Ok(_), Ok(_), Ok(_), inspect) if inspect.is_empty() => (
            format!("no receipts for {package_id} on {}", harness.uze_name),
            Status::InfraFailure,
        ),
        (Ok(_), Ok(_), Ok(_), inspect) if inspect.iter().any(|(_, state)| state != "MATCHED") => (
            format!("receipt did not stay Matched after repeat setup: {inspect:?}"),
            Status::CapabilityFailure,
        ),
        (Ok(_), Ok(first), Ok(_), _) => (
            format!("import manifest stable: {first:?}; second setup made no changes"),
            Status::Pass,
        ),
    };
    EvidenceRecord::new(
        harness.id,
        "R7-repeated-setup-idempotency",
        Level::L2,
        "lifecycle",
        status,
        claim,
        evidence,
        started.elapsed(),
    )
}

/// The plugin names `agy plugin list` reports, sorted — the truthful vendor
/// view the Lab's regression scenario works against.
fn vendor_import_names(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
) -> Result<Vec<String>, String> {
    let result = run(&environment.spec(harness.executable, vec!["plugin".into(), "list".into()]))
        .map_err(|error| error.to_string())?;
    if result.exit_code != Some(0) {
        return Err(format!("agy plugin list exited {:?}", result.exit_code));
    }
    let listing: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|error| error.to_string())?;
    let mut names: Vec<String> = listing
        .get("imports")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    Ok(names)
}

/// G1 — the golden environment chain: the same fixture
/// `golden_environment_is_healthy` materializes, installed through the real
/// `uze` binary, and reported by the real harness. This is the L3 → L2
/// evidence link, not a duplicate of the L3 test.
pub fn l2_golden_record(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    package_id: &str,
) -> EvidenceRecord {
    let claim = "the golden environment UZE declares healthy is recognized by the real harness's own reporting";
    let probe = harness
        .probe_for(ProbeCapability::Package)
        .or_else(|| harness.probe_for(ProbeCapability::Skill));
    let capability = if harness.probe_for(ProbeCapability::Package).is_some() {
        ProbeCapability::Package
    } else {
        ProbeCapability::Skill
    };
    let name = discover_attachment_name(environment, harness, package_id, capability);
    l2_probe_record(
        environment,
        harness,
        "G1-golden-environment",
        "package",
        claim,
        probe.map(|p| (p, name.ok())),
    )
}

/// L4 — one real model turn that must surface a proof the prompt never
/// contains. The only level a model can fail. Deliberately no retry: a retry
/// loop previously hid a deterministic fixture defect by making it look
/// intermittent.
pub fn l4(
    environment: &LabEnvironment,
    harness: &HarnessSpec,
    gateway: &str,
    prompt: &str,
    proof: &str,
    scenario: &str,
    capability: &str,
) -> EvidenceRecord {
    let started = Instant::now();
    let claim = "a real model turn surfaces the capability proof the prompt never contains";
    let Some(l4) = harness.l4 else {
        return EvidenceRecord::new(
            harness.id,
            scenario,
            Level::L4,
            capability,
            Status::Unverified,
            &format!(
                "{claim} — {} declares no gateway-routable L4 route",
                harness.id
            ),
            "no L4 route declared".to_owned(),
            started.elapsed(),
        );
    };
    let workspace = environment.workspace.to_string_lossy().into_owned();
    let substitute = |value: &str| {
        value
            .replace("{model}", l4.model)
            .replace("{gateway}", gateway)
            .replace("{workspace}", &workspace)
            .replace("{prompt}", prompt)
    };
    let arguments: Vec<String> = l4.arguments.iter().map(|value| substitute(value)).collect();
    let mut spec = environment.spec(harness.executable, arguments);
    for (name, value) in l4.environment {
        spec.environment
            .insert((*name).to_owned(), substitute(value));
    }
    let (evidence, status) = match run(&spec) {
        Err(error) => (error.to_string(), Status::InfraFailure),
        Ok(result) if result.timed_out => (
            format!("timed out: {}", excerpt(&combined_output(&result))),
            Status::InfraFailure,
        ),
        Ok(result) => {
            let output = combined_output(&result);
            if output.contains(proof) {
                (excerpt(&output), Status::Pass)
            } else if result.exit_code == Some(0) {
                // The harness ran cleanly and the model simply did not use
                // the capability. That is a model failure, and per the
                // evidence model it must never be rewritten into an
                // integration incompatibility claim.
                (excerpt(&output), Status::ModelFailure)
            } else if let Some(reason) = provider_failure(&output) {
                (
                    format!("{reason}: {}", excerpt(&output)),
                    Status::ProviderFailure,
                )
            } else {
                (
                    format!("exit {:?}: {}", result.exit_code, excerpt(&output)),
                    Status::HarnessFailure,
                )
            }
        }
    };
    EvidenceRecord::new(
        harness.id,
        scenario,
        Level::L4,
        capability,
        status,
        claim,
        evidence,
        started.elapsed(),
    )
}
