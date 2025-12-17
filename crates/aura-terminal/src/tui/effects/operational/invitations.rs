//! Invitation command handlers
//!
//! Handlers for ExportInvitation, ImportInvitation, InviteGuardian, SubmitGuardianApproval.

use std::str::FromStr;
use std::sync::Arc;

use aura_agent::handlers::{ShareableInvitation, ShareableInvitationError};
use aura_app::views::invitations::InvitationType as ViewInvitationType;
use aura_app::AppCore;
use aura_core::identifiers::AuthorityId;
use aura_invitation::InvitationType as DomainInvitationType;
use tokio::sync::RwLock;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

/// Handle invitation commands
pub async fn handle_invitations(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::ExportInvitation { invitation_id } => {
            // Export invitation code through effect injection (RuntimeBridge)
            let result = if let Ok(core) = app_core.try_read() {
                core.export_invitation(invitation_id).await
            } else {
                Err(aura_app::core::IntentError::internal_error(
                    "AppCore unavailable",
                ))
            };

            match result {
                Ok(code) => Some(Ok(OpResponse::InvitationCode {
                    id: invitation_id.clone(),
                    code,
                })),
                Err(e) => {
                    // In demo/offline mode, generate a proper invitation code from ViewState
                    tracing::debug!(
                        "Invitation export via runtime unavailable: {}, generating from ViewState",
                        e
                    );

                    // Get authority and invitation data from AppCore
                    let code = if let Ok(core) = app_core.try_read() {
                        // Get authority (use default if not set)
                        let authority = core
                            .authority()
                            .copied()
                            .unwrap_or_else(|| AuthorityId::new_from_entropy([0u8; 32]));

                        // Get invitation from ViewState
                        let snapshot = core.snapshot();
                        if let Some(inv) = snapshot.invitations.invitation(invitation_id) {
                            // Map view invitation type to domain type
                            let domain_type = match inv.invitation_type {
                                ViewInvitationType::Block => DomainInvitationType::Channel {
                                    block_id: inv.block_id.clone().unwrap_or_default(),
                                },
                                ViewInvitationType::Guardian => DomainInvitationType::Guardian {
                                    subject_authority: authority,
                                },
                                ViewInvitationType::Chat => DomainInvitationType::Contact {
                                    petname: inv.from_name.clone().into(),
                                },
                            };

                            // Parse sender authority from string
                            let sender_id =
                                AuthorityId::from_str(&inv.from_id).unwrap_or(authority);

                            // Create ShareableInvitation
                            let shareable = ShareableInvitation {
                                version: ShareableInvitation::CURRENT_VERSION,
                                invitation_id: inv.id.clone(),
                                sender_id,
                                invitation_type: domain_type,
                                expires_at: inv.expires_at,
                                message: inv.message.clone(),
                            };

                            shareable.to_code()
                        } else {
                            // Invitation not found in ViewState, create minimal code
                            let shareable = ShareableInvitation {
                                version: ShareableInvitation::CURRENT_VERSION,
                                invitation_id: invitation_id.clone(),
                                sender_id: authority,
                                invitation_type: DomainInvitationType::Contact { petname: None },
                                expires_at: None,
                                message: None,
                            };
                            shareable.to_code()
                        }
                    } else {
                        // AppCore unavailable, create minimal fallback code
                        let shareable = ShareableInvitation {
                            version: ShareableInvitation::CURRENT_VERSION,
                            invitation_id: invitation_id.clone(),
                            sender_id: AuthorityId::new_from_entropy([0u8; 32]),
                            invitation_type: DomainInvitationType::Contact { petname: None },
                            expires_at: None,
                            message: None,
                        };
                        shareable.to_code()
                    };

                    Some(Ok(OpResponse::InvitationCode {
                        id: invitation_id.clone(),
                        code,
                    }))
                }
            }
        }

        EffectCommand::ImportInvitation { code } => {
            // Parse the invitation code
            tracing::info!("Importing invitation code: {}", code);

            match ShareableInvitation::from_code(code) {
                Ok(invitation) => {
                    // Extract invitation type as string
                    let invitation_type = match &invitation.invitation_type {
                        DomainInvitationType::Channel { block_id } => {
                            format!("channel:{}", block_id)
                        }
                        DomainInvitationType::Guardian { .. } => "guardian".to_string(),
                        DomainInvitationType::Contact { petname } => {
                            if let Some(name) = petname {
                                format!("contact:{}", name)
                            } else {
                                "contact".to_string()
                            }
                        }
                    };

                    tracing::info!(
                        "Successfully parsed invitation: id={}, sender={}, type={}",
                        invitation.invitation_id,
                        invitation.sender_id,
                        invitation_type
                    );

                    Some(Ok(OpResponse::InvitationImported {
                        invitation_id: invitation.invitation_id,
                        sender_id: invitation.sender_id.to_string(),
                        invitation_type,
                        expires_at: invitation.expires_at,
                        message: invitation.message,
                    }))
                }
                Err(e) => {
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
                    tracing::warn!("Failed to import invitation: {}", error_msg);
                    Some(Err(OpError::InvalidArgument(error_msg.to_string())))
                }
            }
        }

        EffectCommand::InviteGuardian { contact_id } => {
            // With contact_id: handled by intent mapper -> Intent::CreateInvitation
            // Without contact_id: UI should show selection modal
            if contact_id.is_none() {
                tracing::info!(
                    "InviteGuardian without contact_id - UI should show selection modal"
                );
                // Return Ok to indicate the command was "handled" - UI interprets this
                // as a signal to show the guardian selection modal
                Some(Ok(OpResponse::Ok))
            } else {
                // This case is handled by intent dispatch, shouldn't reach here
                None
            }
        }

        EffectCommand::SubmitGuardianApproval { guardian_id: _ } => {
            // Now handled by intent mapper -> Intent::ApproveRecovery
            // This shouldn't reach here, but if it does, pass through to intent dispatch
            None
        }

        _ => None,
    }
}
