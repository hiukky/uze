//! The two answers every integration gives in the same shape: a resource it
//! will not deliver, and a receipt it cannot verify.

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentInspection, AttachmentState},
    router::CompatibilityRoute,
};

/// A plan that delivers nothing, stating why as both rationale and evidence.
pub(crate) fn unsupported(rationale: impl Into<String>) -> ExposurePlan {
    let rationale = rationale.into();
    ExposurePlan {
        route: CompatibilityRoute::Unsupported,
        mechanism: ExposureMechanism::Unsupported {
            rationale: rationale.clone(),
        },
        evidence: rationale,
    }
}

/// An inspection that could not establish the receipt's state, so nothing
/// destructive may follow it.
pub(crate) fn blocked(reason: impl Into<String>) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason: reason.into(),
    }
}
