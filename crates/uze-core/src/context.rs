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

use crate::{integration::AttachmentInspection, store::PackageId, text_region};

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
pub fn region_identity_for(package_id: &PackageId) -> String {
    format!("package:{}:instructions", package_id.as_str())
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
    let mut report = AgentsMdReconciliation::default();
    let mut expected_identities = std::collections::BTreeSet::new();

    for contribution in contributions {
        let identity = region_identity_for(&contribution.package_id);
        expected_identities.insert(identity.clone());
        let _ = text_region::attach(agents_md, &identity, &contribution.content);
        let inspection = text_region::inspect(agents_md, &identity, &contribution.content);
        report.packages.push((contribution.package_id.clone(), inspection));
    }

    for present in text_region::region_identities_present(agents_md) {
        let is_our_shape = present.starts_with(CONTRIBUTION_PREFIX) && present.ends_with(CONTRIBUTION_SUFFIX);
        if !is_our_shape || expected_identities.contains(&present) {
            continue;
        }
        match text_region::remove_unconditionally(agents_md, &present) {
            Ok(inspection) if inspection.state == crate::integration::AttachmentState::Missing => {
                report.removed_orphans.push(present);
            }
            Ok(inspection) => report.blocked_orphans.push((present, inspection.reason)),
            Err(error) => report.blocked_orphans.push((present, error.to_string())),
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::AttachmentState;
    use std::{fs, path::PathBuf};

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-context-{label}-{}-{nonce}", std::process::id()))
    }

    fn package_id(name: &str) -> PackageId {
        PackageId::from_plugin_name(name, std::path::Path::new("plugin.json")).unwrap()
    }

    #[test]
    fn a_single_package_creates_one_matched_region_leaving_user_content_alone() {
        let root = temp("single");
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
        let root = temp("two-packages");
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
        let root = temp("orphan-removal");
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
        assert_eq!(report.removed_orphans, vec![region_identity_for(&package_id("pkg-a"))]);
        assert_eq!(report.packages.len(), 1);
        assert_eq!(report.packages[0].1.state, AttachmentState::Matched);

        let content = fs::read_to_string(&agents_md).unwrap();
        assert!(!content.contains("content A"));
        assert!(content.contains("content B"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_every_package_leaves_no_matched_contribution() {
        let root = temp("all-removed");
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
        assert_eq!(fs::read_to_string(&agents_md).unwrap(), "user content survives\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_user_edited_contribution_is_reported_drifted_never_overwritten() {
        let root = temp("drift");
        let agents_md = root.join("AGENTS.md");
        reconcile_agents_md(
            &agents_md,
            &[InstructionContribution {
                package_id: package_id("pkg-a"),
                content: "original".to_owned(),
            }],
        );
        let tampered = fs::read_to_string(&agents_md).unwrap().replace("original", "user-edited");
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

    #[test]
    fn reconcile_is_idempotent_across_repeated_calls() {
        let root = temp("idempotent");
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
