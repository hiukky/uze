use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    capability::{Classification, Harness, ItemKind, ProjectItem, classify},
    project::ResolvedProject,
    runtime::{RuntimeIntegration, select_runtime_integration, unverified_runtime_support},
};

#[derive(Clone, Debug, Serialize)]
pub struct CompatibilityReport {
    pub project_root: String,
    pub portable_core: Vec<ReportItem>,
    pub optional_enhancements: Vec<ReportItem>,
    pub runtime_integration: BTreeMap<Harness, RuntimeIntegration>,
    pub standards_coverage: Vec<StandardsCoverage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportItem {
    pub name: String,
    pub path: String,
    pub kind: ItemKind,
    pub outcomes: BTreeMap<Harness, Outcome>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub classification: Classification,
    pub rationale: String,
    pub evidence_source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StandardsCoverage {
    pub concern: String,
    pub standard: Option<String>,
    pub coverage: String,
    pub remaining_gap: String,
}

pub fn build_report(project: &ResolvedProject) -> CompatibilityReport {
    let runtime_integration = Harness::ALL
        .into_iter()
        .map(|harness| {
            (
                harness,
                select_runtime_integration(&unverified_runtime_support(harness)),
            )
        })
        .collect();
    CompatibilityReport {
        project_root: project.root.to_string_lossy().into_owned(),
        portable_core: project
            .portable_core
            .iter()
            .map(|item| report_item(project, item))
            .collect(),
        optional_enhancements: project
            .enhancements
            .iter()
            .map(|item| report_item(project, item))
            .collect(),
        runtime_integration,
        standards_coverage: standards_coverage(),
    }
}

pub fn render_text(report: &CompatibilityReport) -> String {
    let mut output = format!(
        "UZE compatibility report\nProject: {}\n\n",
        report.project_root
    );
    append_items(&mut output, "Portable core", &report.portable_core);
    append_items(
        &mut output,
        "Optional enhancements",
        &report.optional_enhancements,
    );
    output.push_str("Runtime integration\n");
    for (harness, integration) in &report.runtime_integration {
        output.push_str(&format!(
            "- {}: {}\n",
            harness.as_str(),
            runtime_path(integration)
        ));
    }
    output.push_str("\nStandards Coverage / Remaining Gap\n");
    for row in &report.standards_coverage {
        let standard = row.standard.as_deref().unwrap_or("None");
        output.push_str(&format!(
            "- {} | {} | {} | {}\n",
            row.concern, standard, row.coverage, row.remaining_gap
        ));
    }
    output
}

fn report_item(project: &ResolvedProject, item: &ProjectItem) -> ReportItem {
    let outcomes = Harness::ALL
        .into_iter()
        .map(|harness| {
            let result = classify(item, harness);
            (
                harness,
                Outcome {
                    classification: result.classification,
                    rationale: result.rationale,
                    evidence_source: result.evidence_source.to_owned(),
                },
            )
        })
        .collect();
    ReportItem {
        name: item.name(),
        path: item.display_path(&project.root),
        kind: item.kind.clone(),
        outcomes,
    }
}

fn append_items(output: &mut String, heading: &str, items: &[ReportItem]) {
    output.push_str(heading);
    output.push('\n');
    if items.is_empty() {
        output.push_str("- none discovered\n\n");
        return;
    }
    for item in items {
        output.push_str(&format!("- {} ({})\n", item.path, item.name));
        for (harness, outcome) in &item.outcomes {
            output.push_str(&format!(
                "  - {}: {:?} — {}\n",
                harness.as_str(),
                outcome.classification,
                outcome.rationale
            ));
        }
    }
    output.push('\n');
}

fn runtime_path(integration: &RuntimeIntegration) -> String {
    match integration {
        RuntimeIntegration::NativeAcp => "native ACP".to_owned(),
        RuntimeIntegration::ReliableAcpAdapter { adapter } => {
            format!("reliable ACP adapter ({adapter})")
        }
        RuntimeIntegration::MinimalExplicitAdapter { adapter } => {
            format!("minimal explicit adapter ({adapter})")
        }
        RuntimeIntegration::Unavailable { rationale } => format!("unavailable — {rationale}"),
    }
}

fn standards_coverage() -> Vec<StandardsCoverage> {
    vec![
        coverage(
            "Project instructions",
            Some("AGENTS.md"),
            "Portable instructions where a runtime supports discovery.",
            "Discovery and proprietary rules vary.",
        ),
        coverage(
            "Reusable capabilities",
            Some("Agent Skills"),
            "Portable SKILL.md capabilities and assets.",
            "Locations and vendor extensions vary.",
        ),
        coverage(
            "Tools/resources",
            Some("MCP"),
            "Portable tool, resource, and prompt/context interoperability.",
            "Configuration and authorization vary.",
        ),
        coverage(
            "Client ↔ Agent",
            Some("ACP"),
            "Negotiated runtime interaction when both endpoints support ACP.",
            "Target adoption and adapters vary.",
        ),
        coverage(
            "Agent ↔ Agent",
            Some("A2A"),
            "Future candidate only.",
            "No MVP orchestration need.",
        ),
        coverage(
            "Project composition",
            None,
            "No current composition standard.",
            "UZE resolves and explains the effective environment without creating a format.",
        ),
        coverage(
            "Harness-specific capabilities",
            None,
            "Optional native enhancements may be used.",
            "Hooks, commands, subagents, permissions, and memory have no safe universal equivalence.",
        ),
    ]
}

fn coverage(
    concern: &str,
    standard: Option<&str>,
    coverage: &str,
    remaining_gap: &str,
) -> StandardsCoverage {
    StandardsCoverage {
        concern: concern.to_owned(),
        standard: standard.map(str::to_owned),
        coverage: coverage.to_owned(),
        remaining_gap: remaining_gap.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{project::resolve_project, report::build_report};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn report_is_reproducible_for_unchanged_project() {
        let root = temp_project("report");
        fs::create_dir_all(root.join(".agents/skills/review")).unwrap();
        fs::write(root.join("AGENTS.md"), "# Instructions\n").unwrap();
        fs::write(
            root.join(".agents/skills/review/SKILL.md"),
            "---\nname: review\n---\n",
        )
        .unwrap();
        fs::write(root.join("mcp.json"), "{\"mcpServers\":{}}\n").unwrap();

        let project = resolve_project(&root).unwrap();
        let first = serde_json::to_vec(&build_report(&project)).unwrap();
        let second = serde_json::to_vec(&build_report(&project)).unwrap();
        assert_eq!(first, second);
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_project(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
    }
}
