use aura_app::runtime_bridge::KeyRotationCeremonyStatus;
use aura_app::ui::types::CeremonyKind;
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
        CeremonyLifecycleState::Completed | CeremonyLifecycleState::Failed => None,
    }
}
