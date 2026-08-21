//! Core read-only comparison of persisted attachment intent with the live harness
//! state. Reconciliation intentionally never repairs or detaches anything.

use serde::Serialize;

use crate::{
    home::UzeHome,
    integration::{AttachmentInspection, AttachmentReceipt, AttachmentState, IntegrationPort},
    state,
};

#[derive(Clone, Debug, Serialize)]
pub struct ReconciledReceipt {
    pub ledger_key: String,
    pub receipt: AttachmentReceipt,
    pub inspection: AttachmentInspection,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ReconciliationReport {
    pub package_id: String,
    pub receipts: Vec<ReconciledReceipt>,
    /// A corrupt or unreadable ledger means UZE cannot prove ownership. It is
    /// represented in the report so callers naturally block destructive work.
    pub ledger_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PackageRemovalPlan {
    Safe {
        detachable_receipts: Vec<String>,
        already_missing_receipts: Vec<String>,
    },
    BlockedByDrift,
    BlockedByConflict,
    BlockedByInspection,
}

/// Inspects each ledger receipt through its owning integration. Unknown
/// integrations are BLOCKED; they are never silently skipped.
pub fn reconcile_package(
    home: &UzeHome,
    package_id: &str,
    integrations: &[&dyn IntegrationPort],
) -> ReconciliationReport {
    let entries = match state::receipts(home, Some(package_id)) {
        Ok(entries) => entries,
        Err(error) => {
            return ReconciliationReport {
                package_id: package_id.to_owned(),
                receipts: Vec::new(),
                ledger_error: Some(error.to_string()),
            };
        }
    };
    let receipts = entries
        .into_iter()
        .map(|(ledger_key, receipt)| {
            let inspection = integrations
                .iter()
                .find(|integration| integration.id() == receipt.integration)
                .map(|integration| integration.inspect_receipt(&receipt))
                .unwrap_or_else(|| AttachmentInspection {
                    state: AttachmentState::Blocked,
                    reason: format!("integration `{}` is unavailable", receipt.integration),
                });
            ReconciledReceipt {
                ledger_key,
                receipt,
                inspection,
            }
        })
        .collect();
    ReconciliationReport {
        package_id: package_id.to_owned(),
        receipts,
        ledger_error: None,
    }
}

/// Conservative remove planning. The caller may detach only the returned
/// MATCHED receipts. MISSING receipts have no external state left to remove.
pub fn plan_remove(report: &ReconciliationReport) -> PackageRemovalPlan {
    if report.ledger_error.is_some()
        || report
            .receipts
            .iter()
            .any(|receipt| receipt.inspection.state == AttachmentState::Blocked)
    {
        return PackageRemovalPlan::BlockedByInspection;
    }
    if report
        .receipts
        .iter()
        .any(|receipt| receipt.inspection.state == AttachmentState::Conflict)
    {
        return PackageRemovalPlan::BlockedByConflict;
    }
    if report
        .receipts
        .iter()
        .any(|receipt| receipt.inspection.state == AttachmentState::Drifted)
    {
        return PackageRemovalPlan::BlockedByDrift;
    }
    PackageRemovalPlan::Safe {
        detachable_receipts: report
            .receipts
            .iter()
            .filter(|receipt| receipt.inspection.state == AttachmentState::Matched)
            .map(|receipt| receipt.ledger_key.clone())
            .collect(),
        already_missing_receipts: report
            .receipts
            .iter()
            .filter(|receipt| receipt.inspection.state == AttachmentState::Missing)
            .map(|receipt| receipt.ledger_key.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        UzeHome,
        exposure::ExposurePlan,
        integration::{HarnessDetection, ManagedArtifact},
        router::HarnessCapabilities,
    };

    use super::*;

    struct TestIntegration;
    impl IntegrationPort for TestIntegration {
        fn id(&self) -> &'static str {
            "test"
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }
        fn exposure_plan(&self, _resource: &crate::Resource) -> ExposurePlan {
            panic!("not used")
        }
        fn detect(&self) -> HarnessDetection {
            HarnessDetection::default()
        }
    }

    fn home(label: &str) -> UzeHome {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        UzeHome::at(std::env::temp_dir().join(format!(
            "uze-reconcile-{label}-{}-{nonce}",
            std::process::id()
        )))
    }

    #[cfg(unix)]
    #[test]
    fn reconciliation_and_removal_plan_preserve_drift() {
        use std::os::unix::fs::symlink;
        let home = home("drift");
        let root = home.root().join("fixture");
        fs::create_dir_all(&root).unwrap();
        let expected = root.join("expected");
        let other = root.join("other");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&other).unwrap();
        let managed = root.join("managed");
        symlink(&expected, &managed).unwrap();
        let receipt = AttachmentReceipt {
            package_id: "plugin".to_owned(),
            resource_identity: Some("skill:x".to_owned()),
            integration: "test".to_owned(),
            strategy: "symlink".to_owned(),
            artifact: ManagedArtifact::SymlinkReference {
                path: managed.clone(),
                target: expected,
            },
        };
        state::record_receipt(&home, "plugin:test:skill".to_owned(), receipt).unwrap();
        let integration = TestIntegration;
        let report = reconcile_package(&home, "plugin", &[&integration]);
        assert!(matches!(
            plan_remove(&report),
            PackageRemovalPlan::Safe { .. }
        ));
        fs::remove_file(&managed).unwrap();
        symlink(&other, &managed).unwrap();
        let report = reconcile_package(&home, "plugin", &[&integration]);
        assert_eq!(
            report.receipts[0].inspection.state,
            AttachmentState::Drifted
        );
        assert_eq!(plan_remove(&report), PackageRemovalPlan::BlockedByDrift);
        fs::remove_dir_all(home.root()).unwrap();
    }

    #[test]
    fn corrupt_ledger_blocks_removal_planning() {
        let home = home("corrupt");
        home.ensure_layout().unwrap();
        fs::write(home.state_dir().join("attachments.json"), "bad").unwrap();
        let report = reconcile_package(&home, "plugin", &[]);
        assert!(report.ledger_error.is_some());
        assert_eq!(
            plan_remove(&report),
            PackageRemovalPlan::BlockedByInspection
        );
        fs::remove_dir_all(home.root()).unwrap();
    }
}
