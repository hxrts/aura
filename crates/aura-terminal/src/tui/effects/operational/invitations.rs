//! Invitation command handlers
//!
//! Handlers for invitation import/export and runtime-backed accept/decline.
//!
//! This module delegates to portable workflows in aura_app::ui::workflows::invitation
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::SETTINGS_SIGNAL;
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

async fn resolve_contact_authority_id(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<aura_core::identifiers::AuthorityId, OpError> {
    let target = target.trim();
    if target.is_empty() {
        return Err(OpError::InvalidArgument(
            "Contact target cannot be empty".to_string(),
        ));
    }

    if let Ok(id) = target.parse() {
        return Ok(id);
    }

    match aura_app::ui::workflows::query::resolve_contact(app_core, target).await {
        Ok(contact) => Ok(contact.id),
        Err(error) => Err(OpError::InvalidArgument(format!(
            "Invalid contact authority ID: {target} ({error})"
        ))),
    }
}

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
            let receiver = match resolve_contact_authority_id(app_core, receiver_id).await {
                Ok(id) => id,
                Err(error) => return Some(Err(error)),
            };

            let ttl_ms = ttl_secs.map(|s| s.saturating_mul(1000));
            let invitation_type_lc = invitation_type.to_lowercase();
            let (kind, extra) = invitation_type_lc
                .split_once(':')
                .map(|(k, rest)| (k, Some(rest.to_string())))
                .unwrap_or((invitation_type_lc.as_str(), None));

            let info = match kind {
                "contact" | "personal" => {
                    // If no explicit contact nickname is provided, default to the
                    // sender's current nickname suggestion so recipients can render
                    // a friendly display name immediately after import.
                    let contact_nickname = if let Some(name) = extra.as_ref() {
                        let name = name.trim();
                        if name.is_empty() {
                            None
                        } else {
                            Some(name.to_string())
                        }
                    } else {
                        let core = app_core.read().await;
                        core.read(&*SETTINGS_SIGNAL)
                            .await
                            .ok()
                            .map(|settings| settings.nickname_suggestion.trim().to_string())
                            .filter(|name| !name.is_empty())
                    };

                    match create_contact_invitation(
                        app_core,
                        receiver,
                        contact_nickname,
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
                        match aura_app::ui::workflows::context::current_home_id_or_fallback(
                            app_core,
                        )
                        .await
                        {
                            Ok(id) => id.to_string(),
                            Err(error) => {
                                return Some(Err(OpError::InvalidArgument(error.to_string())));
                            }
                        }
                    };

                    if home_id.trim().is_empty() {
                        return Some(Err(OpError::InvalidArgument(
                            aura_app::ui::workflows::context::missing_active_home_message()
                                .to_string(),
                        )));
                    }

                    match create_channel_invitation(
                        app_core,
                        receiver,
                        home_id,
                        None,
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
            let receiver = match resolve_contact_authority_id(app_core, contact_id).await {
                Ok(id) => id,
                Err(error) => return Some(Err(error)),
            };

            let home_id =
                match aura_app::ui::workflows::context::current_home_id_or_fallback(app_core).await
                {
                    Ok(id) => id.to_string(),
                    Err(error) => return Some(Err(OpError::InvalidArgument(error.to_string()))),
                };

            match create_channel_invitation(app_core, receiver, home_id, None, None, None, None)
                .await
            {
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
                    // Interactive semantics: importing non-device invitations
                    // performs the acceptance step immediately.
                    if matches!(
                        invitation.invitation_type,
                        InvitationBridgeType::Contact { .. }
                            | InvitationBridgeType::Channel { .. }
                            | InvitationBridgeType::Guardian { .. }
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
