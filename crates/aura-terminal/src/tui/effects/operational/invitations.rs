//! Invitation command handlers
//!
//! Handlers for ExportInvitation, ImportInvitation, InviteGuardian, SubmitGuardianApproval.
//!
//! This module delegates to portable workflows in aura_app::workflows::invitation
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use aura_agent::handlers::ShareableInvitation;
use aura_app::AppCore;
use async_lock::RwLock;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::workflows::invitation::export_invitation;

/// Handle invitation commands
pub async fn handle_invitations(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::ExportInvitation { invitation_id } => {
            // Delegate to workflow
            match export_invitation(app_core, invitation_id).await {
                Ok(code) => Some(Ok(OpResponse::InvitationCode {
                    id: invitation_id.clone(),
                    code,
                })),
                Err(e) => {
                    // Workflow failed (likely RuntimeBridge unavailable in demo mode)
                    // Return error - the UI layer can decide how to handle this
                    Some(Err(OpError::Failed(format!(
                        "Failed to export invitation: {}",
                        e
                    ))))
                }
            }
        }

        EffectCommand::ImportInvitation { code } => {
            // Parse the invitation code
            // TODO: Move to workflow once RuntimeBridge is extended
            match ShareableInvitation::from_code(code) {
                Ok(invitation) => {
                    use aura_invitation::InvitationType;

                    // Format invitation type for display
                    let invitation_type = match &invitation.invitation_type {
                        InvitationType::Channel { block_id } => {
                            format!("channel:{}", block_id)
                        }
                        InvitationType::Guardian { .. } => "guardian".to_string(),
                        InvitationType::Contact { petname } => {
                            if let Some(name) = petname {
                                format!("contact:{}", name)
                            } else {
                                "contact".to_string()
                            }
                        }
                    };

                    Some(Ok(OpResponse::InvitationImported {
                        invitation_id: invitation.invitation_id,
                        sender_id: invitation.sender_id.to_string(),
                        invitation_type,
                        expires_at: invitation.expires_at,
                        message: invitation.message,
                    }))
                }
                Err(e) => {
                    use aura_agent::handlers::ShareableInvitationError;

                    let error_msg = match e {
                        ShareableInvitationError::InvalidFormat => "Invalid invitation code format",
                        ShareableInvitationError::UnsupportedVersion(_) => {
                            "Unsupported invitation version"
                        }
                        ShareableInvitationError::DecodingFailed => {
                            "Failed to decode invitation data"
                        }
                        ShareableInvitationError::ParsingFailed => {
                            "Failed to parse invitation data"
                        }
                    };
                    Some(Err(OpError::InvalidArgument(error_msg.to_string())))
                }
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
