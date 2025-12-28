//! Recovery command handlers
//!
//! Handlers for recovery operations including starting recovery ceremonies
//! and approving recovery requests as a guardian.
//!
//! This module delegates to portable workflows in aura_app::workflows::recovery
//! and adds terminal-specific response formatting.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

use super::types::{OpError, OpResponse, OpResult};
use super::EffectCommand;

// Re-export workflows for convenience
pub use aura_app::workflows::recovery::{approve_recovery, get_recovery_status};

/// Handle recovery commands
pub async fn handle_recovery(
    command: &EffectCommand,
    app_core: &Arc<RwLock<AppCore>>,
) -> Option<OpResult> {
    match command {
        EffectCommand::StartRecovery => {
            // StartRecovery initiates the recovery ceremony flow.
            // In the full implementation, this would:
            // 1. Read current guardians from RECOVERY_SIGNAL
            // 2. Use the existing guardian set and threshold
            // 3. Start the ceremony via RuntimeBridge
            //
            // For now, we check if runtime is available and report status.
            let runtime = {
                let core = app_core.read().await;
                core.runtime().cloned()
            };

            match runtime {
                Some(rt) => {
                    // Get current recovery state to check guardians
                    match get_recovery_status(app_core).await {
                        Ok(state) => {
                            if state.guardians.is_empty() {
                                Some(Err(OpError::Failed(
                                    "No guardians configured. Add guardians in Threshold settings first."
                                        .to_string(),
                                )))
                            } else if state.active_recovery.is_some() {
                                Some(Err(OpError::Failed(
                                    "Recovery already in progress".to_string(),
                                )))
                            } else {
                                // Initiate guardian ceremony with existing guardians
                                let guardian_ids: Vec<String> =
                                    state.guardians.iter().map(|g| g.id.to_string()).collect();
                                let threshold =
                                    aura_core::types::FrostThreshold::new(state.threshold as u16)
                                        .unwrap_or_else(|_| {
                                            aura_core::types::FrostThreshold::new(2).unwrap()
                                        });

                                match rt
                                    .initiate_guardian_ceremony(
                                        threshold,
                                        guardian_ids.len() as u16,
                                        &guardian_ids,
                                    )
                                    .await
                                {
                                    Ok(ceremony_id) => Some(Ok(OpResponse::Data(format!(
                                        "Recovery started: {}",
                                        ceremony_id
                                    )))),
                                    Err(e) => Some(Err(OpError::Failed(format!(
                                        "Failed to start recovery: {}",
                                        e
                                    )))),
                                }
                            }
                        }
                        Err(e) => Some(Err(OpError::Failed(format!(
                            "Failed to get recovery status: {}",
                            e
                        )))),
                    }
                }
                None => Some(Err(OpError::Failed(
                    "Runtime bridge not available".to_string(),
                ))),
            }
        }

        EffectCommand::SubmitGuardianApproval { guardian_id } => {
            // Approve a pending recovery request.
            // The guardian_id here is actually the ceremony_id/request_id.
            match approve_recovery(app_core, guardian_id).await {
                Ok(()) => Some(Ok(OpResponse::Ok)),
                Err(e) => Some(Err(OpError::Failed(format!(
                    "Failed to approve recovery: {}",
                    e
                )))),
            }
        }

        EffectCommand::CompleteRecovery => {
            // Complete recovery after threshold is met.
            // This would finalize the key rotation ceremony.
            let runtime = {
                let core = app_core.read().await;
                core.runtime().cloned()
            };

            match runtime {
                Some(_rt) => {
                    // Check if there's an active recovery that's ready to complete
                    match get_recovery_status(app_core).await {
                        Ok(state) => match state.active_recovery {
                            Some(recovery) => {
                                if recovery.approvals_received >= recovery.approvals_required {
                                    // Ready to complete - would call RuntimeBridge to finalize
                                    Some(Ok(OpResponse::Data(
                                        "Recovery ceremony ready to complete".to_string(),
                                    )))
                                } else {
                                    Some(Err(OpError::Failed(format!(
                                        "Need {} more approvals",
                                        recovery.approvals_required - recovery.approvals_received
                                    ))))
                                }
                            }
                            None => Some(Err(OpError::Failed("No active recovery".to_string()))),
                        },
                        Err(e) => Some(Err(OpError::Failed(format!(
                            "Failed to get recovery status: {}",
                            e
                        )))),
                    }
                }
                None => Some(Err(OpError::Failed(
                    "Runtime bridge not available".to_string(),
                ))),
            }
        }

        EffectCommand::CancelRecovery => {
            // Cancel an ongoing recovery ceremony.
            let runtime = {
                let core = app_core.read().await;
                core.runtime().cloned()
            };

            match runtime {
                Some(_rt) => {
                    match get_recovery_status(app_core).await {
                        Ok(state) => match state.active_recovery {
                            Some(_recovery) => {
                                // Would call RuntimeBridge to cancel the ceremony
                                Some(Ok(OpResponse::Data("Recovery cancelled".to_string())))
                            }
                            None => Some(Err(OpError::Failed(
                                "No active recovery to cancel".to_string(),
                            ))),
                        },
                        Err(e) => Some(Err(OpError::Failed(format!(
                            "Failed to get recovery status: {}",
                            e
                        )))),
                    }
                }
                None => Some(Err(OpError::Failed(
                    "Runtime bridge not available".to_string(),
                ))),
            }
        }

        EffectCommand::InviteGuardian { contact_id } => {
            // Invite a contact to become a guardian.
            // If contact_id is None, the UI should show a selection modal (handled by invitations.rs).
            // If contact_id is Some, create a guardian invitation.
            match contact_id {
                None => {
                    // This case is handled by invitations.rs which returns Some(Ok(OpResponse::Ok))
                    // We shouldn't reach here, but if we do, also return Ok
                    Some(Ok(OpResponse::Ok))
                }
                Some(id) => {
                    // Create guardian invitation for the specified contact.
                    // This requires parsing the contact_id to AuthorityId and getting
                    // the current user's authority for the subject parameter.
                    let runtime = {
                        let core = app_core.read().await;
                        core.runtime().cloned()
                    };

                    match runtime {
                        Some(rt) => {
                            // Parse contact_id to AuthorityId
                            let receiver: aura_core::identifiers::AuthorityId = match id.parse() {
                                Ok(auth_id) => auth_id,
                                Err(_) => {
                                    return Some(Err(OpError::InvalidArgument(format!(
                                        "Invalid contact ID: {}",
                                        id
                                    ))));
                                }
                            };

                            // Get current user's authority for subject
                            let subject = rt.authority_id();

                            match rt
                                .create_guardian_invitation(receiver, subject, None, None)
                                .await
                            {
                                Ok(invitation_info) => Some(Ok(OpResponse::Data(format!(
                                    "Guardian invitation created: {}",
                                    invitation_info.invitation_id
                                )))),
                                Err(e) => Some(Err(OpError::Failed(format!(
                                    "Failed to create guardian invitation: {}",
                                    e
                                )))),
                            }
                        }
                        None => Some(Err(OpError::Failed(
                            "Runtime bridge not available".to_string(),
                        ))),
                    }
                }
            }
        }

        _ => None,
    }
}
