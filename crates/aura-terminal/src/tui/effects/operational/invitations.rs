//! Invitation command handlers
//!
//! Handlers for invitation import/export and runtime-backed accept/decline.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::invitation
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::types::InvitationBridgeType;
use aura_core::effects::reactive::ReactiveEffects;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::ui::workflows::invitation::{
    accept_invitation, accept_invitation_by_str, cancel_invitation_by_str,
    create_channel_invitation, create_contact_invitation, create_guardian_invitation,
    decline_invitation_by_str, export_invitation, export_invitation_by_str,
    import_invitation_details,
};

/// Handle invitation commands
pub async fn handle_invitations(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::CreateInvitation {
            receiver_id,
            invitation_type,
            message,
            ttl_secs,
        } => {
            let receiver: aura_core::identifiers::AuthorityId = match receiver_id.parse() {
                Ok(id) => id,
                Err(_) => {
                    return Some(Err(OpError::InvalidArgument(format!(
                        "Invalid receiver authority ID: {receiver_id}"
                    ))));
                }
            };

            let ttl_ms = ttl_secs.map(|s| s.saturating_mul(1000));
            let invitation_type_lc = invitation_type.to_lowercase();
            let (kind, extra) = invitation_type_lc
                .split_once(':')
                .map(|(k, rest)| (k, Some(rest.to_string())))
                .unwrap_or((invitation_type_lc.as_str(), None));

            let info = match kind {
                "contact" | "personal" => {
                    match create_contact_invitation(
                        app_core,
                        receiver,
                        None,
                        message.clone(),
                        ttl_ms,
                    )
                    .await
                    {
                        Ok(info) => info,
                        Err(e) => {
                            return Some(Err(OpError::Failed(format!(
                                "Failed to create contact invitation: {e}"
                            ))));
                        }
                    }
                }
                "guardian" => {
                    let subject = {
                        let core = app_core.read().await;
                        match core.authority() {
                            Some(id) => id.clone(),
                            None => {
                                return Some(Err(OpError::Failed(
                                    "No local authority is set; cannot create guardian invitation"
                                        .to_string(),
                                )));
                            }
                        }
                    };

                    match create_guardian_invitation(
                        app_core,
                        receiver,
                        subject,
                        message.clone(),
                        ttl_ms,
                    )
                    .await
                    {
                        Ok(info) => info,
                        Err(e) => {
                            return Some(Err(OpError::Failed(format!(
                                "Failed to create guardian invitation: {e}"
                            ))));
                        }
                    }
                }
                "channel" | "chat" | "group" => {
                    let home_id = if let Some(id) = extra {
                        id
                    } else {
                        // Best effort: use the currently-selected home/channel from the reactive view.
                        let core = app_core.read().await;
                        let homes_state = core
                            .read(&*aura_app::ui::signals::HOMES_SIGNAL)
                            .await
                            .unwrap_or_default();
                        homes_state
                            .current_home_id()
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "home".to_string())
                    };

                    if home_id.trim().is_empty() {
                        return Some(Err(OpError::InvalidArgument(
                            "No active home/channel to invite to".to_string(),
                        )));
                    }

                    match create_channel_invitation(
                        app_core,
                        receiver,
                        home_id,
                        None,
                        message.clone(),
                        ttl_ms,
                    )
                    .await
                    {
                        Ok(info) => info,
                        Err(e) => {
                            return Some(Err(OpError::Failed(format!(
                                "Failed to create channel invitation: {e}"
                            ))));
                        }
                    }
                }
                other => {
                    return Some(Err(OpError::InvalidArgument(format!(
                        "Unknown invitation type: {other}"
                    ))));
                }
            };

            match export_invitation(app_core, &info.invitation_id).await {
                Ok(code) => Some(Ok(OpResponse::InvitationCode {
                    id: info.invitation_id.as_str().to_string(),
                    code,
                })),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to export invitation: {e}"
                )))),
            }
        }

        EffectCommand::SendHomeInvitation { contact_id } => {
            let receiver: aura_core::identifiers::AuthorityId = match contact_id.parse() {
                Ok(id) => id,
                Err(_) => {
                    return Some(Err(OpError::InvalidArgument(format!(
                        "Invalid contact authority ID: {contact_id}"
                    ))));
                }
            };

            // Best effort: use the currently-selected home from the reactive view.
            let home_id = {
                use aura_core::effects::reactive::ReactiveEffects;

                let core = app_core.read().await;

                if let Ok(homes) = core.read(&*aura_app::ui::signals::HOMES_SIGNAL).await {
                    homes
                        .current_home_id()
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "home".to_string())
                } else {
                    "home".to_string()
                }
            };

            match create_channel_invitation(app_core, receiver, home_id, None, None, None).await {
                Ok(info) => Some(Ok(OpResponse::Data(format!(
                    "Home invitation sent: {}",
                    info.invitation_id.as_str()
                )))),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to send home invitation: {e}"
                )))),
            }
        }

        EffectCommand::ExportInvitation { invitation_id } => {
            // Delegate to workflow
            match export_invitation_by_str(app_core, invitation_id).await {
                Ok(code) => Some(Ok(OpResponse::InvitationCode {
                    id: invitation_id.clone(),
                    code,
                })),
                Err(e) => {
                    // Workflow failed (likely RuntimeBridge unavailable in demo mode)
                    // Return error - the UI layer can decide how to handle this
                    Some(Err(OpError::Failed(format!(
                        "Failed to export invitation: {e}"
                    ))))
                }
            }
        }

        EffectCommand::ImportInvitation { code } => {
            // Delegate to workflow for parsing via RuntimeBridge
            match import_invitation_details(app_core, code).await {
                Ok(invitation) => {
                    // Demo semantics: importing a CONTACT invite code is the acceptance step.
                    if matches!(
                        invitation.invitation_type,
                        InvitationBridgeType::Contact { .. }
                    ) {
                        if let Err(e) = accept_invitation(app_core, &invitation.invitation_id).await
                        {
                            return Some(Err(OpError::InvalidArgument(format!(
                                "Failed to accept invitation: {e}"
                            ))));
                        }
                    }

                    // Format invitation type for display
                    let invitation_type = match &invitation.invitation_type {
                        InvitationBridgeType::Channel { home_id, .. } => {
                            format!("channel:{home_id}")
                        }
                        InvitationBridgeType::Guardian { .. } => "guardian".to_string(),
                        InvitationBridgeType::Contact { nickname } => {
                            if let Some(name) = nickname {
                                format!("contact:{name}")
                            } else {
                                "contact".to_string()
                            }
                        }
                        InvitationBridgeType::DeviceEnrollment {
                            nickname_suggestion,
                            device_id,
                            ..
                        } => {
                            if let Some(name) = nickname_suggestion {
                                format!("device:{name}")
                            } else {
                                format!("device:{device_id}")
                            }
                        }
                    };

                    Some(Ok(OpResponse::InvitationImported {
                        invitation_id: invitation.invitation_id.as_str().to_string(),
                        sender_id: invitation.sender_id.to_string(),
                        invitation_type,
                        expires_at: invitation.expires_at_ms,
                        message: invitation.message,
                    }))
                }
                Err(e) => Some(Err(OpError::InvalidArgument(format!(
                    "Invalid invitation code: {e}"
                )))),
            }
        }

        EffectCommand::AcceptInvitation { invitation_id } => {
            match accept_invitation_by_str(app_core, invitation_id).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to accept invitation: {e}"
                )))),
            }
        }

        EffectCommand::DeclineInvitation { invitation_id } => {
            match decline_invitation_by_str(app_core, invitation_id).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to decline invitation: {e}"
                )))),
            }
        }

        EffectCommand::CancelInvitation { invitation_id } => {
            match cancel_invitation_by_str(app_core, invitation_id).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to cancel invitation: {e}"
                )))),
            }
        }

        EffectCommand::InviteGuardian { contact_id } => {
            // Without contact_id: UI should show selection modal
            // With contact_id: handled by intent mapper -> Intent::CreateInvitation
            if contact_id.is_none() {
                // Return Ok to signal UI should show the guardian selection modal
                Some(Ok(OpResponse::Ok))
            } else {
                // This case is handled by intent dispatch
                None
            }
        }

        EffectCommand::SubmitGuardianApproval { guardian_id: _ } => {
            // Handled by intent mapper -> Intent::ApproveRecovery
            // Shouldn't reach here, but if it does, pass through to intent dispatch
            None
        }

        _ => None,
    }
}
