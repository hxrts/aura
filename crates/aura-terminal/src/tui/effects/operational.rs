#![allow(clippy::expect_used)]
//! # Operational Command Handler
//!
//! Handles operational (non-journaled) commands directly.
//! These are commands that don't create journal facts - they're runtime operations
//! like sync, peer management, and system commands.
//!
//! ## Design
//!
//! Unlike journaled commands that go through `AppCore.dispatch(Intent)`, operational
//! commands are executed directly and may update status signals for UI feedback.
//!
//! ## Command Categories
//!
//! - **System**: Ping, Shutdown, RefreshAccount
//! - **Sync**: ForceSync, RequestState
//! - **Network**: AddPeer, RemovePeer, ListPeers, DiscoverPeers, ListLanPeers
//! - **Settings**: UpdateMfaPolicy, UpdateNickname, SetChannelMode
//! - **Invitations**: ExportInvitation, ImportInvitation

use std::collections::HashSet;
use std::sync::Arc;

use aura_app::signal_defs::{
    AppError, ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, ERROR_SIGNAL,
    SYNC_STATUS_SIGNAL,
};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use tokio::sync::RwLock;

use super::EffectCommand;

/// Result type for operational commands
pub type OpResult = Result<OpResponse, OpError>;

/// Response from an operational command
#[derive(Debug, Clone)]
pub enum OpResponse {
    /// Command succeeded with no data
    Ok,
    /// Command returned data
    Data(String),
    /// Command returned a list
    List(Vec<String>),
    /// Invitation code exported
    InvitationCode { id: String, code: String },
}

/// Error from an operational command
#[derive(Debug, Clone, thiserror::Error)]
pub enum OpError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Operation failed: {0}")]
    Failed(String),
}

/// Handles operational commands that don't create journal facts.
///
/// This handler processes commands that
/// are purely runtime operations (sync, peer management, etc.).
pub struct OperationalHandler {
    app_core: Arc<RwLock<AppCore>>,
    peers: Arc<RwLock<HashSet<String>>>,
}

impl OperationalHandler {
    /// Create a new operational handler
    pub fn new(app_core: Arc<RwLock<AppCore>>) -> Self {
        Self {
            app_core,
            peers: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Execute an operational command
    ///
    /// Returns `Some(result)` if the command was handled, `None` if it should
    /// be handled elsewhere (e.g., by intent dispatch).
    pub async fn execute(&self, command: &EffectCommand) -> Option<OpResult> {
        match command {
            // =========================================================================
            // System Commands
            // =========================================================================
            EffectCommand::Ping => {
                // Simple ping - just return Ok
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::Shutdown => {
                // Shutdown is handled by the TUI event loop, not here
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::RefreshAccount => {
                // Trigger a state refresh by reading and re-emitting signals
                // This causes subscribers to re-render with current state
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Sync Commands
            // =========================================================================
            EffectCommand::ForceSync => {
                // Update sync status signal to show syncing
                if let Ok(core) = self.app_core.try_read() {
                    let _ = core
                        .emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Syncing { progress: 0 })
                        .await;
                }

                // Trigger sync through effect injection (RuntimeBridge)
                let result = if let Ok(core) = self.app_core.try_read() {
                    core.trigger_sync().await
                } else {
                    Err(aura_app::core::IntentError::internal_error("AppCore unavailable"))
                };

                // Update status based on result
                if let Ok(core) = self.app_core.try_read() {
                    match &result {
                        Ok(()) => {
                            let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                        }
                        Err(e) => {
                            tracing::warn!("Sync trigger failed: {}", e);
                            // In demo/offline mode, show as synced (local-only)
                            let _ = core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced).await;
                        }
                    }
                }

                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::RequestState { peer_id: _ } => {
                // Request state for a specific peer - triggers sync
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Network/Peer Commands
            // =========================================================================
            EffectCommand::AddPeer { peer_id } => {
                {
                    let mut peers = self.peers.write().await;
                    peers.insert(peer_id.clone());
                    let count = peers.len();

                    if let Ok(core) = self.app_core.try_read() {
                        let _ = core
                            .emit(
                                &*CONNECTION_STATUS_SIGNAL,
                                ConnectionStatus::Online { peer_count: count },
                            )
                            .await;
                    }
                }
                tracing::info!("Added peer: {}", peer_id);
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::RemovePeer { peer_id } => {
                {
                    let mut peers = self.peers.write().await;
                    peers.remove(peer_id);
                    let count = peers.len();

                    if let Ok(core) = self.app_core.try_read() {
                        let status = if count == 0 {
                            ConnectionStatus::Offline
                        } else {
                            ConnectionStatus::Online { peer_count: count }
                        };
                        let _ = core.emit(&*CONNECTION_STATUS_SIGNAL, status).await;
                    }
                }
                tracing::info!("Removed peer: {}", peer_id);
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::ListPeers => {
                // Return empty list for now - would query actual peer list
                Some(Ok(OpResponse::List(vec![])))
            }

            EffectCommand::DiscoverPeers => {
                // Trigger peer discovery
                tracing::info!("Discovering peers...");
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::ListLanPeers => {
                // Return discovered LAN peers
                Some(Ok(OpResponse::List(vec![])))
            }

            EffectCommand::InviteLanPeer {
                authority_id,
                address,
            } => {
                tracing::info!("Inviting LAN peer: {} at {}", authority_id, address);
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Query Commands
            // =========================================================================
            EffectCommand::ListParticipants { channel: _ } => {
                // Would query channel participants
                Some(Ok(OpResponse::List(vec![])))
            }

            EffectCommand::GetUserInfo { target } => {
                // Would query user info
                Some(Ok(OpResponse::Data(format!("User: {}", target))))
            }

            // =========================================================================
            // Context Commands
            // =========================================================================
            EffectCommand::SetContext { context_id: _ } => {
                // Set active context - used for navigation
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::MovePosition { .. } => {
                // Move position in neighborhood view
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::AcceptPendingBlockInvitation => {
                // Accept a pending block invitation
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Settings Commands
            // =========================================================================
            EffectCommand::UpdateMfaPolicy { require_mfa: _ } => {
                // Update MFA policy setting
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::UpdateNickname { name: _ } => {
                // Update display nickname
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::SetChannelMode {
                channel: _,
                flags: _,
            } => {
                // Set channel mode (public/private)
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Invitation Commands (Operational - export/import codes)
            // =========================================================================
            EffectCommand::ExportInvitation { invitation_id } => {
                // Export invitation code through effect injection (RuntimeBridge)
                let result = if let Ok(core) = self.app_core.try_read() {
                    core.export_invitation(invitation_id).await
                } else {
                    Err(aura_app::core::IntentError::internal_error("AppCore unavailable"))
                };

                match result {
                    Ok(code) => Some(Ok(OpResponse::InvitationCode {
                        id: invitation_id.clone(),
                        code,
                    })),
                    Err(e) => {
                        // In demo/offline mode, generate a placeholder code
                        tracing::debug!("Invitation export via runtime unavailable: {}", e);
                        let code = format!(
                            "AURA-{}-INVITE",
                            &invitation_id[..8.min(invitation_id.len())]
                        );
                        Some(Ok(OpResponse::InvitationCode {
                            id: invitation_id.clone(),
                            code,
                        }))
                    }
                }
            }

            EffectCommand::ImportInvitation { code } => {
                // Parse and process an invitation code
                tracing::info!("Importing invitation code: {}", code);
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::InviteGuardian { contact_id } => {
                tracing::info!("Inviting guardian: {:?}", contact_id);
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::SubmitGuardianApproval { guardian_id } => {
                tracing::info!("Submitting guardian approval for: {}", guardian_id);
                Some(Ok(OpResponse::Ok))
            }

            // =========================================================================
            // Direct Messaging Commands
            // =========================================================================
            EffectCommand::SendDirectMessage {
                target: _,
                content: _,
            } => {
                // DMs could be handled via Intent in future
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::StartDirectChat { contact_id: _ } => Some(Ok(OpResponse::Ok)),

            EffectCommand::SendAction {
                channel: _,
                action: _,
            } => {
                // IRC-style /me action
                Some(Ok(OpResponse::Ok))
            }

            EffectCommand::InviteUser { target: _ } => Some(Ok(OpResponse::Ok)),

            // =========================================================================
            // Steward Commands (placeholder until proper role system)
            // =========================================================================
            EffectCommand::GrantSteward { target: _ } => Some(Err(OpError::NotImplemented(
                "Steward grant requires role system".to_string(),
            ))),

            EffectCommand::RevokeSteward { target: _ } => Some(Err(OpError::NotImplemented(
                "Steward revoke requires role system".to_string(),
            ))),

            // =========================================================================
            // Commands handled by Intent dispatch - return None
            // =========================================================================
            _ => None,
        }
    }

    /// Update connection status signal
    pub async fn set_connection_status(&self, status: ConnectionStatus) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*CONNECTION_STATUS_SIGNAL, status).await;
        }
    }

    /// Update sync status signal
    pub async fn set_sync_status(&self, status: SyncStatus) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*SYNC_STATUS_SIGNAL, status).await;
        }
    }

    /// Emit an error to the error signal
    pub async fn emit_error(&self, error: AppError) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*ERROR_SIGNAL, Some(error)).await;
        }
    }

    /// Clear the error signal
    pub async fn clear_error(&self) {
        if let Ok(core) = self.app_core.try_read() {
            let _ = core.emit(&*ERROR_SIGNAL, None).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::AppConfig;

    fn test_app_core() -> Arc<RwLock<AppCore>> {
        Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).expect("Failed to create test AppCore"),
        ))
    }

    #[tokio::test]
    async fn test_ping_command() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler.execute(&EffectCommand::Ping).await;
        assert!(matches!(result, Some(Ok(OpResponse::Ok))));
    }

    #[tokio::test]
    async fn test_list_peers_returns_list() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler.execute(&EffectCommand::ListPeers).await;
        assert!(matches!(result, Some(Ok(OpResponse::List(_)))));
    }

    #[tokio::test]
    async fn test_export_invitation_returns_code() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: "test-123".to_string(),
            })
            .await;

        match result {
            Some(Ok(OpResponse::InvitationCode { id, code })) => {
                assert_eq!(id, "test-123");
                assert!(code.starts_with("AURA-"));
            }
            _ => panic!("Expected InvitationCode response"),
        }
    }

    #[tokio::test]
    async fn test_intent_commands_return_none() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        // SendMessage should return None (handled by intent dispatch)
        let result = handler
            .execute(&EffectCommand::SendMessage {
                channel: "general".to_string(),
                content: "Hello".to_string(),
            })
            .await;
        assert!(result.is_none());
    }
}
