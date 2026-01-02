use aura_app::runtime_bridge::{InvitationBridgeStatus, InvitationBridgeType, InvitationInfo};
use aura_core::types::Epoch;

/// Convert domain Invitation to bridge InvitationInfo.
pub(super) fn convert_invitation_to_bridge_info(
    invitation: &crate::handlers::invitation::Invitation,
) -> InvitationInfo {
    InvitationInfo {
        invitation_id: invitation.invitation_id.to_string(),
        sender_id: invitation.sender_id,
        receiver_id: invitation.receiver_id,
        invitation_type: convert_invitation_type_to_bridge(&invitation.invitation_type),
        status: convert_invitation_status_to_bridge(&invitation.status),
        created_at_ms: invitation.created_at,
        expires_at_ms: invitation.expires_at,
        message: invitation.message.clone(),
    }
}

/// Convert domain InvitationType to bridge InvitationBridgeType.
pub(super) fn convert_invitation_type_to_bridge(
    inv_type: &crate::handlers::invitation::InvitationType,
) -> InvitationBridgeType {
    match inv_type {
        crate::handlers::invitation::InvitationType::Contact { nickname } => {
            InvitationBridgeType::Contact {
                nickname: nickname.clone(),
            }
        }
        crate::handlers::invitation::InvitationType::Guardian { subject_authority } => {
            InvitationBridgeType::Guardian {
                subject_authority: *subject_authority,
            }
        }
        crate::handlers::invitation::InvitationType::Channel { home_id } => {
            InvitationBridgeType::Channel {
                home_id: home_id.clone(),
            }
        }
        crate::handlers::invitation::InvitationType::DeviceEnrollment {
            subject_authority,
            initiator_device_id,
            device_id,
            device_name,
            ceremony_id,
            pending_epoch,
            key_package: _,
            threshold_config: _,
            public_key_package: _,
        } => InvitationBridgeType::DeviceEnrollment {
            subject_authority: *subject_authority,
            initiator_device_id: *initiator_device_id,
            device_id: *device_id,
            device_name: device_name.clone(),
            ceremony_id: ceremony_id.to_string(),
            pending_epoch: Epoch::new(*pending_epoch),
        },
    }
}

/// Convert domain InvitationStatus to bridge InvitationBridgeStatus.
pub(super) fn convert_invitation_status_to_bridge(
    status: &crate::handlers::invitation::InvitationStatus,
) -> InvitationBridgeStatus {
    match status {
        crate::handlers::invitation::InvitationStatus::Pending => InvitationBridgeStatus::Pending,
        crate::handlers::invitation::InvitationStatus::Accepted => InvitationBridgeStatus::Accepted,
        crate::handlers::invitation::InvitationStatus::Declined => InvitationBridgeStatus::Declined,
        crate::handlers::invitation::InvitationStatus::Cancelled => {
            InvitationBridgeStatus::Cancelled
        }
        crate::handlers::invitation::InvitationStatus::Expired => InvitationBridgeStatus::Expired,
    }
}
