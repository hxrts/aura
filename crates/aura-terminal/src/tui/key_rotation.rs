use aura_app::ui::types::{CeremonyKind, KeyRotationCeremonyStatus};
use aura_app::ui::workflows::ceremonies::CeremonyLifecycleState;

use crate::tui::components::ToastMessage;
use crate::tui::updates::UiUpdate;

pub(crate) fn key_rotation_status_update(status: &KeyRotationCeremonyStatus) -> UiUpdate {
    UiUpdate::KeyRotationCeremonyStatus {
        ceremony_id: status.ceremony_id.to_string(),
        kind: status.kind,
        accepted_count: status.accepted_count,
        total_count: status.total_count,
        threshold: status.threshold,
        is_complete: status.is_complete,
        has_failed: status.has_failed,
        accepted_participants: status.accepted_participants.clone(),
        error_message: status.error_message.clone(),
        pending_epoch: status.pending_epoch,
        agreement_mode: status.agreement_mode,
        reversion_risk: status.reversion_risk,
    }
}

pub(crate) fn key_rotation_lifecycle_toast(
    kind: CeremonyKind,
    state: CeremonyLifecycleState,
    error: Option<&str>,
) -> Option<ToastMessage> {
    let (id_prefix, label) = match kind {
        CeremonyKind::GuardianRotation => ("guardian-ceremony", "Guardian ceremony"),
        CeremonyKind::DeviceRotation => ("mfa-ceremony", "Multifactor ceremony"),
        CeremonyKind::DeviceEnrollment => ("device-enrollment", "Device enrollment"),
        CeremonyKind::DeviceRemoval => ("device-removal", "Device removal"),
        CeremonyKind::Recovery => ("recovery-ceremony", "Recovery ceremony"),
        CeremonyKind::Invitation => ("invitation-ceremony", "Invitation ceremony"),
        CeremonyKind::RendezvousSecureChannel => ("rendezvous-ceremony", "Rendezvous ceremony"),
        CeremonyKind::OtaActivation => ("ota-activation-ceremony", "OTA activation ceremony"),
    };

    match state {
        CeremonyLifecycleState::TimedOut => Some(ToastMessage::error(
            format!("{id_prefix}-lifecycle-timeout"),
            format!("{label} did not settle before timeout"),
        )),
        CeremonyLifecycleState::FailedRollbackIncomplete => Some(ToastMessage::error(
            format!("{id_prefix}-rollback-incomplete"),
            format!(
                "{label} failed and rollback was incomplete; manual intervention may be required"
            ),
        )),
        // A failed ceremony must be visible: the start reported success, so
        // without this the failure reached only the log (work/8.md Task 56).
        CeremonyLifecycleState::Failed => Some(ToastMessage::error(
            format!("{id_prefix}-failed"),
            match error.map(str::trim).filter(|error| !error.is_empty()) {
                Some(error) => format!("{label} failed: {error}"),
                None => format!("{label} failed"),
            },
        )),
        CeremonyLifecycleState::Completed => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_ceremony_produces_an_error_toast_with_its_reason() {
        let toast = key_rotation_lifecycle_toast(
            CeremonyKind::GuardianRotation,
            CeremonyLifecycleState::Failed,
            Some("guardian declined"),
        )
        .expect("failure is surfaced");
        assert!(toast
            .message
            .contains("Guardian ceremony failed: guardian declined"));
        assert!(key_rotation_lifecycle_toast(
            CeremonyKind::DeviceRotation,
            CeremonyLifecycleState::Failed,
            None,
        )
        .is_some_and(|toast| toast.message == "Multifactor ceremony failed"));
        assert!(key_rotation_lifecycle_toast(
            CeremonyKind::GuardianRotation,
            CeremonyLifecycleState::Completed,
            None,
        )
        .is_none());
    }
}
