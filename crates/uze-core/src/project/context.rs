//! Project-scoped Instructions reconciliation.
//!
//! Composes what currently-installed packages contribute into one shared,
//! canonical `AGENTS.md`-shaped file, using `text_region` for ownership of
//! each package's own delimited slice. This module knows a package id and a
//! project path; it knows nothing about any harness, any vendor bridge
//! file, or how many other integrations might also read the result — that
//! is a separate, explicit concern layered on top (see
//! `uze-application`'s context orchestration).
//!
//! `uze add`/`uze remove` never call anything here: package installation
//! stays global and project-independent. Reconciling a project's shared
//! instructions file is its own explicit, re-runnable operation that takes
//! a project root as ordinary input, not a persisted concept.

use std::path::Path;

use serde::Serialize;

use crate::{
    integration::{AttachmentInspection, AttachmentState},
    store::PackageId,
    text_region,
};

/// One package's contributed Instruction content, ready to compose into a
/// project's shared file.
#[derive(Clone, Debug)]
pub struct InstructionContribution {
    pub package_id: PackageId,
    pub content: String,
}

/// The stable, collision-safe region identity a package's contribution owns
/// inside the shared file. Deterministic from the package id alone — two
/// reconcile calls for the same package always agree on this identity.
/// `:`-joined rather than `id.as_str()`'s `@`: `text_region::identity_is_valid`
/// restricts a region identity to `[a-zA-Z0-9:_./-]`, which `@` falls outside
/// of. `plugin_name`/`marketplace` can themselves never contain `:` (the one
/// chokepoint every `PackageId` is validated through forbids it), so the two
/// parts joined by `:` remain exactly as collision-safe as the qualified id.
pub fn region_identity_for(package_id: &PackageId) -> String {
    format!(
        "package:{}:{}:instructions",
        package_id.plugin_name(),
        package_id.marketplace()
    )
}

/// Prefix every identity this module creates carries, used only to decide
/// whether an unrecognized region in the file is "ours to consider
/// orphaned" versus something else entirely that happens to also use UZE's
/// marker syntax (e.g. a future, differently-scoped capability).
const CONTRIBUTION_PREFIX: &str = "package:";
const CONTRIBUTION_SUFFIX: &str = ":instructions";

/// Per-package outcome plus any orphaned regions this pass removed —
/// regions whose identity matches this module's own naming shape but no
/// longer corresponds to any contribution in the current call. See
/// `text_region::remove_unconditionally` for exactly what safety guarantee
/// that removal carries (structural, not content-drift-verified).
#[derive(Clone, Debug, Default)]
pub struct AgentsMdReconciliation {
    pub packages: Vec<(PackageId, AttachmentInspection)>,
    pub removed_orphans: Vec<String>,
    pub blocked_orphans: Vec<(String, String)>,
}

impl AgentsMdReconciliation {
    /// Whether the shared file, after this reconciliation, still carries at
    /// least one well-formed contribution — the exact question a bridge's
    /// own reconciliation needs answered.
    pub fn has_any_matched_contribution(&self) -> bool {
        self.packages
            .iter()
            .any(|(_, inspection)| inspection.state == crate::integration::AttachmentState::Matched)
    }
}

/// A region identity this module created but that no longer corresponds to
/// any contribution passed to the current call — the package that used to
/// own it is no longer installed (or was never passed in). Structural, not
/// content-verified: see `text_region::remove_unconditionally`/
/// `text_region::region_shape` for exactly what that means.
fn is_our_orphan_shape(identity: &str) -> bool {
    identity.starts_with(CONTRIBUTION_PREFIX) && identity.ends_with(CONTRIBUTION_SUFFIX)
}

/// Read-only observation of `agents_md` against `contributions` — never
/// writes, whatever state the file is in. This is the one place the
/// package-vs-file diff is computed; `reconcile_agents_md` and
/// `plan_agents_md` both build on it rather than recomputing the diff
/// themselves.
#[derive(Clone, Debug, Default)]
pub struct AgentsMdObservation {
    pub packages: Vec<(PackageId, AttachmentInspection)>,
    /// Well-formed regions matching this module's own naming shape, present
    /// in the file, that no `contributions` entry claims any more.
    pub orphaned_regions: Vec<String>,
    /// A region matching this module's own naming shape whose markers are
    /// themselves malformed (duplicated, out of order, half-present) —
    /// reported, never silently ignored or guessed at.
    pub malformed_regions: Vec<String>,
}

impl AgentsMdObservation {
    pub fn has_any_matched_contribution(&self) -> bool {
        self.packages
            .iter()
            .any(|(_, inspection)| inspection.state == AttachmentState::Matched)
    }
}

/// Observes what `agents_md` currently holds against the desired
/// `contributions`, without changing anything — regardless of whether the
/// file exists, whether a contribution is drifted, or whether an orphaned
/// region's markers are malformed. Every branch here calls only
/// `text_region::inspect`/`region_shape`/`region_identities_present`, all
/// themselves zero-write.
pub fn inspect_agents_md(
    agents_md: &Path,
    contributions: &[InstructionContribution],
) -> AgentsMdObservation {
    let mut observation = AgentsMdObservation::default();
    let mut expected_identities = std::collections::BTreeSet::new();

    for contribution in contributions {
        let identity = region_identity_for(&contribution.package_id);
        expected_identities.insert(identity.clone());
        let inspection = text_region::inspect(agents_md, &identity, &contribution.content);
        observation
            .packages
            .push((contribution.package_id.clone(), inspection));
    }

    // Deduplicated: a malformed, duplicated marker for one identity makes
    // `region_identities_present` return that same identity once per
    // occurrence — this loop must still evaluate and report it exactly
    // once, not once per stray marker line.
    let present_identities: std::collections::BTreeSet<String> =
        text_region::region_identities_present(agents_md)
            .into_iter()
            .collect();
    for present in present_identities {
        if !is_our_orphan_shape(&present) || expected_identities.contains(&present) {
            continue;
        }
        match text_region::region_shape(agents_md, &present) {
            text_region::RegionShape::WellFormed => observation.orphaned_regions.push(present),
            text_region::RegionShape::Malformed => observation.malformed_regions.push(present),
            text_region::RegionShape::Absent => {
                // Listed by `region_identities_present` a moment ago, gone
                // now — a benign race with something else touching the file
                // between the two reads. Nothing to report; the next
                // observation will see whatever is actually there.
            }
        }
    }

    observation
}

/// Ensures `agents_md` holds exactly one region per `contributions` entry,
/// content-matched, and removes any region this module previously created
/// for a package no longer present in `contributions`.
///
/// Idempotent and safe to call repeatedly: a package already matched is
/// never rewritten; a package whose region drifted (user-edited) is
/// reported, never silently overwritten; an orphaned region whose markers
/// are malformed is reported blocked, never guessed at.
pub fn reconcile_agents_md(
    agents_md: &Path,
    contributions: &[InstructionContribution],
) -> AgentsMdReconciliation {
    for contribution in contributions {
        let identity = region_identity_for(&contribution.package_id);
        let _ = text_region::attach(agents_md, &identity, &contribution.content);
    }

    // The attach pass above only ever creates a *missing* region or leaves
    // an already-matched one untouched (see `text_region::attach`); it never
    // silently resolves drift. Re-observing afterwards is therefore always
    // an accurate account of the result, not a second, competing diff.
    let observation = inspect_agents_md(agents_md, contributions);
    let mut report = AgentsMdReconciliation {
        packages: observation.packages,
        removed_orphans: Vec::new(),
        blocked_orphans: observation
            .malformed_regions
            .into_iter()
            .map(|identity| {
                (
                    identity,
                    "managed text region markers are duplicated, out of order, or only half present"
                        .to_owned(),
                )
            })
            .collect(),
    };

    for identity in observation.orphaned_regions {
        match text_region::remove_unconditionally(agents_md, &identity) {
            Ok(inspection) if inspection.state == AttachmentState::Missing => {
                report.removed_orphans.push(identity);
            }
            Ok(inspection) => report.blocked_orphans.push((identity, inspection.reason)),
            Err(error) => report.blocked_orphans.push((identity, error.to_string())),
        }
    }

    report
}

/// What `reconcile_agents_md` would do for one identity, derived purely
/// from an `AttachmentInspection`/`RegionShape` already observed — never a
/// second, independent decision. `Attach`/`Remove` name the exact
/// `text_region` operation that would run; `NoChange` and `Blocked` name why
/// nothing would.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "action",
    content = "reason",
    rename_all = "SCREAMING_SNAKE_CASE"
)]
pub enum PlannedAction {
    /// The region does not exist yet and would be created.
    Attach,
    /// Already exactly as desired; reconcile would not touch it.
    NoChange,
    /// Exists but reconcile refuses to touch it — content drifted, or
    /// markers are malformed. Carries why.
    Blocked(String),
    /// No longer desired (its package is not among the current
    /// contributions) and would be removed.
    Remove,
}

fn plan_for_contribution(inspection: &AttachmentInspection) -> PlannedAction {
    match inspection.state {
        AttachmentState::Matched => PlannedAction::NoChange,
        AttachmentState::Missing => PlannedAction::Attach,
        AttachmentState::Drifted | AttachmentState::Blocked | AttachmentState::Conflict => {
            PlannedAction::Blocked(inspection.reason.clone())
        }
    }
}

/// One package's planned contribution.
#[derive(Clone, Debug, Serialize)]
pub struct ContributionPlan {
    pub package_id: PackageId,
    pub action: PlannedAction,
}

/// One orphaned region's planned fate.
#[derive(Clone, Debug, Serialize)]
pub struct OrphanPlan {
    pub region_identity: String,
    pub action: PlannedAction,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AgentsMdPlan {
    pub contributions: Vec<ContributionPlan>,
    pub orphans: Vec<OrphanPlan>,
}

impl AgentsMdPlan {
    /// Whether executing this plan would change anything on disk.
    /// `Blocked` is deliberately **not** a change: it means reconcile would
    /// refuse to touch that entry, exactly like `NoChange` in terms of
    /// bytes written, just for a different reason worth surfacing
    /// separately.
    pub fn has_changes(&self) -> bool {
        self.contributions
            .iter()
            .any(|plan| matches!(plan.action, PlannedAction::Attach))
            || self
                .orphans
                .iter()
                .any(|plan| matches!(plan.action, PlannedAction::Remove))
    }
}

/// The plan `reconcile_agents_md` would execute, computed without writing
/// anything — built directly from `inspect_agents_md`'s observation, so a
/// plan and a subsequent reconcile can never disagree about what "wrong"
/// looks like, only about whether it was fixed.
pub fn plan_agents_md(agents_md: &Path, contributions: &[InstructionContribution]) -> AgentsMdPlan {
    let observation = inspect_agents_md(agents_md, contributions);
    AgentsMdPlan {
        contributions: observation
            .packages
            .into_iter()
            .map(|(package_id, inspection)| ContributionPlan {
                action: plan_for_contribution(&inspection),
                package_id,
            })
            .collect(),
        orphans: observation
            .orphaned_regions
            .into_iter()
            .map(|region_identity| OrphanPlan {
                region_identity,
                action: PlannedAction::Remove,
            })
            .chain(
                observation
                    .malformed_regions
                    .into_iter()
                    .map(|region_identity| {
                        OrphanPlan {
                action: PlannedAction::Blocked(
                    "managed text region markers are duplicated, out of order, or only half present"
                        .to_owned(),
                ),
                region_identity,
            }
                    }),
            )
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::AttachmentState;
    use std::fs;

    fn package_id(name: &str) -> PackageId {
        PackageId::from_plugin_name(name, std::path::Path::new("plugin.json")).unwrap()
    }

    #[test]
    fn a_single_package_creates_one_matched_region_leaving_user_content_alone() {
        let root = uze_testkit::temp::scratch("single");
        fs::create_dir_all(&root).unwrap();
        let agents_md = root.join("AGENTS.md");
        fs::write(&agents_md, "# My Project\n\nSome user notes.\n").unwrap();

        let report = reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "Use 2-space indentation.".to_owned(),
            }],
        );
        assert_eq!(report.packages.len(), 1);
        assert_eq!(report.packages[0].1.state, AttachmentState::Matched);
        assert!(report.removed_orphans.is_empty());
        let content = fs::read_to_string(&agents_md).unwrap();
        assert!(content.starts_with("# My Project\n\nSome user notes.\n"));
        assert!(content.contains("Use 2-space indentation."));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn two_packages_coexist_as_independent_regions() {
        let root = uze_testkit::temp::scratch("two-packages");
        let agents_md = root.join("AGENTS.md");
        let report = reconcile_agents_md(
            &agents_md,
            &[
                InstructionContribution {
                    package_id: package_id("pkg-a"),
                    content: "content A".to_owned(),
                },
                InstructionContribution {
                    package_id: package_id("pkg-b"),
                    content: "content B".to_owned(),
                },
            ],
        );
        assert!(
            report
                .packages
                .iter()
                .all(|(_, inspection)| inspection.state == AttachmentState::Matched)
        );
        assert!(report.has_any_matched_contribution());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_a_package_from_the_desired_set_removes_its_orphaned_region() {
        let root = uze_testkit::temp::scratch("orphan-removal");
        let agents_md = root.join("AGENTS.md");
        reconcile_agents_md(
            &agents_md,
            &[
                InstructionContribution {
                    package_id: package_id("pkg-a"),
                    content: "content A".to_owned(),
                },
                InstructionContribution {
                    package_id: package_id("pkg-b"),
                    content: "content B".to_owned(),
                },
            ],
        );

        // pkg-a is no longer installed: only pkg-b is in the desired set now.
        let report = reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-b"),
                content: "content B".to_owned(),
            }],
        );
        assert_eq!(
            report.removed_orphans,
            vec![region_identity_for(&package_id("pkg-a"))]
        );
        assert_eq!(report.packages.len(), 1);
        assert_eq!(report.packages[0].1.state, AttachmentState::Matched);

        let content = fs::read_to_string(&agents_md).unwrap();
        assert!(!content.contains("content A"));
        assert!(content.contains("content B"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_every_package_leaves_no_matched_contribution() {
        let root = uze_testkit::temp::scratch("all-removed");
        let agents_md = root.join("AGENTS.md");
        fs::create_dir_all(&root).unwrap();
        fs::write(&agents_md, "user content survives\n").unwrap();
        reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        let report = reconcile_agents_md(&agents_md, &[]);
        assert!(!report.has_any_matched_contribution());
        assert_eq!(report.removed_orphans.len(), 1);
        assert_eq!(
            fs::read_to_string(&agents_md).unwrap(),
            "user content survives\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_user_edited_contribution_is_reported_drifted_never_overwritten() {
        let root = uze_testkit::temp::scratch("drift");
        let agents_md = root.join("AGENTS.md");
        reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "original".to_owned(),
            }],
        );
        let tampered = fs::read_to_string(&agents_md)
            .unwrap()
            .replace("original", "user-edited");
        fs::write(&agents_md, &tampered).unwrap();

        let report = reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "original".to_owned(),
            }],
        );
        assert_eq!(report.packages[0].1.state, AttachmentState::Drifted);
        assert_eq!(fs::read_to_string(&agents_md).unwrap(), tampered);
        fs::remove_dir_all(root).unwrap();
    }

    /// The core proof Fase 2 demands: a filesystem snapshot taken before and
    /// after `inspect_agents_md` must be byte-identical, in every state the
    /// file could be in — absent, matched, drifted, orphaned, malformed.
    #[test]
    fn inspect_agents_md_never_writes_in_any_state() {
        let root = uze_testkit::temp::scratch("inspect-never-writes");
        fs::create_dir_all(&root).unwrap();
        let agents_md = root.join("AGENTS.md");

        let snapshot = |path: &std::path::Path| -> Option<Vec<u8>> { fs::read(path).ok() };

        // 1. File does not exist at all.
        assert_eq!(snapshot(&agents_md), None);
        inspect_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        assert_eq!(
            snapshot(&agents_md),
            None,
            "inspecting a missing file must not create it"
        );

        // 2. File exists with user content only, no packages desired.
        fs::write(&agents_md, "just my own notes\n").unwrap();
        let before = snapshot(&agents_md);
        inspect_agents_md(&agents_md, &[]);
        assert_eq!(snapshot(&agents_md), before);

        // 3. Matched contribution.
        reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        let before = snapshot(&agents_md);
        let observation = inspect_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        assert_eq!(snapshot(&agents_md), before);
        assert_eq!(observation.packages[0].1.state, AttachmentState::Matched);

        // 4. Drifted contribution.
        let tampered = fs::read_to_string(&agents_md)
            .unwrap()
            .replace("content A", "TAMPERED");
        fs::write(&agents_md, &tampered).unwrap();
        let before = snapshot(&agents_md);
        let observation = inspect_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        assert_eq!(snapshot(&agents_md), before);
        assert_eq!(observation.packages[0].1.state, AttachmentState::Drifted);

        // 5. Orphaned region: desired set no longer includes pkg-a.
        fs::write(&agents_md, &tampered).unwrap(); // restore a well-formed (if drifted) region
        let before = snapshot(&agents_md);
        let observation = inspect_agents_md(&agents_md, &[]);
        assert_eq!(snapshot(&agents_md), before);
        assert_eq!(observation.orphaned_regions.len(), 1);

        // 6. Malformed markers.
        fs::write(
            &agents_md,
            "<!-- uze:begin package:pkg-a:instructions -->\na\n<!-- uze:end package:pkg-a:instructions -->\n<!-- uze:begin package:pkg-a:instructions -->\nb\n<!-- uze:end package:pkg-a:instructions -->\n",
        )
        .unwrap();
        let before = snapshot(&agents_md);
        let observation = inspect_agents_md(&agents_md, &[]);
        assert_eq!(snapshot(&agents_md), before);
        assert_eq!(observation.malformed_regions.len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    /// Regression: a duplicated/malformed marker for one identity used to be
    /// reported (and, in `reconcile_agents_md`, acted on) once per stray
    /// marker *line* rather than once per identity, because
    /// `region_identities_present` legitimately returns one entry per begin
    /// marker occurrence, not one per unique identity.
    #[test]
    fn a_malformed_duplicated_marker_is_reported_exactly_once_not_once_per_marker_line() {
        let root = uze_testkit::temp::scratch("dedup-malformed");
        fs::create_dir_all(&root).unwrap();
        let agents_md = root.join("AGENTS.md");
        fs::write(
            &agents_md,
            "<!-- uze:begin package:pkg-a:instructions -->\na\n<!-- uze:end package:pkg-a:instructions -->\n<!-- uze:begin package:pkg-a:instructions -->\nb\n<!-- uze:end package:pkg-a:instructions -->\n",
        )
        .unwrap();

        let observation = inspect_agents_md(&agents_md, &[]);
        assert_eq!(observation.malformed_regions.len(), 1);

        let plan = plan_agents_md(&agents_md, &[]);
        assert_eq!(plan.orphans.len(), 1);

        let report = reconcile_agents_md(&agents_md, &[]);
        assert_eq!(report.blocked_orphans.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_agents_md_never_writes_and_maps_every_state_correctly() {
        let root = uze_testkit::temp::scratch("plan-mapping");
        let agents_md = root.join("AGENTS.md");

        // Missing package region -> Attach.
        let plan = plan_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        assert!(!agents_md.exists(), "planning must never create the file");
        assert_eq!(plan.contributions[0].action, PlannedAction::Attach);
        assert!(plan.has_changes());

        // Actually reconcile, then plan again for the same desired state -> NoChange.
        reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        let before = fs::read(&agents_md).unwrap();
        let plan = plan_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        assert_eq!(fs::read(&agents_md).unwrap(), before);
        assert_eq!(plan.contributions[0].action, PlannedAction::NoChange);
        assert!(!plan.has_changes());

        // Drift the content -> Blocked, not Attach and not a silent fix.
        let tampered = fs::read_to_string(&agents_md)
            .unwrap()
            .replace("content A", "TAMPERED");
        fs::write(&agents_md, &tampered).unwrap();
        let plan = plan_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        assert!(matches!(
            plan.contributions[0].action,
            PlannedAction::Blocked(_)
        ));
        assert!(
            !plan.has_changes(),
            "a blocked entry is not a pending change"
        );
        assert_eq!(fs::read_to_string(&agents_md).unwrap(), tampered);

        // No longer desired -> Remove for the orphan.
        fs::write(&agents_md, "content A\n").unwrap(); // arbitrary; orphan detection is identity-based
        reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "content A".to_owned(),
            }],
        );
        let plan = plan_agents_md(&agents_md, &[]);
        assert_eq!(plan.orphans[0].action, PlannedAction::Remove);
        assert!(plan.has_changes());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconcile_is_idempotent_across_repeated_calls() {
        let root = uze_testkit::temp::scratch("idempotent");
        let agents_md = root.join("AGENTS.md");
        let contributions = vec![InstructionContribution {
            package_id: package_id("pkg-a"),
            content: "content".to_owned(),
        }];
        reconcile_agents_md(&agents_md, &contributions);
        let after_first = fs::read_to_string(&agents_md).unwrap();
        reconcile_agents_md(&agents_md, &contributions);
        let after_second = fs::read_to_string(&agents_md).unwrap();
        assert_eq!(after_first, after_second);
        fs::remove_dir_all(root).unwrap();
    }
}
