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
//!
//! ## Module Organization
//!
//! - `types`: Result and error types
//! - `system`: System commands (Ping, Shutdown, RefreshAccount)
//! - `sync`: Sync commands (ForceSync, RequestState)
//! - `network`: Network/Peer commands
//! - `query`: Query commands (ListParticipants, GetUserInfo)
//! - `context`: Context commands (SetContext, MovePosition)
//! - `settings`: Settings commands
//! - `invitations`: Invitation export/import commands
//! - `messaging`: Direct messaging commands
//! - `steward`: Steward role management commands

mod context;
mod invitations;
mod messaging;
mod network;
mod query;
mod settings;
mod steward;
mod sync;
mod system;
pub mod types;

use std::collections::HashSet;
use std::sync::Arc;

use aura_app::signal_defs::{
    AppError, ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, ERROR_SIGNAL,
    SYNC_STATUS_SIGNAL,
};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use tokio::sync::RwLock;

pub use types::{OpError, OpResponse, OpResult};

use super::EffectCommand;
use crate::error::TerminalError;

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
        // Try each handler in sequence until one handles the command

        // System commands
        if let Some(result) = system::handle_system(command, &self.app_core).await {
            return Some(result);
        }

        // Sync commands
        if let Some(result) = sync::handle_sync(command, &self.app_core).await {
            return Some(result);
        }

        // Network/Peer commands
        if let Some(result) = network::handle_network(command, &self.app_core, &self.peers).await {
            return Some(result);
        }

        // Query commands
        if let Some(result) = query::handle_query(command, &self.app_core).await {
            return Some(result);
        }

        // Context commands
        if let Some(result) = context::handle_context(command, &self.app_core).await {
            return Some(result);
        }

        // Settings commands
        if let Some(result) = settings::handle_settings(command, &self.app_core).await {
            return Some(result);
        }

        // Invitation commands
        if let Some(result) = invitations::handle_invitations(command, &self.app_core).await {
            return Some(result);
        }

        // Messaging commands
        if let Some(result) = messaging::handle_messaging(command, &self.app_core).await {
            return Some(result);
        }

        // Steward commands
        if let Some(result) = steward::handle_steward(command, &self.app_core).await {
            return Some(result);
        }

        // Command not handled - return None to indicate intent dispatch should handle it
        None
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
    pub async fn emit_error(&self, error: TerminalError) {
        let error = map_terminal_error(&error);
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

/// Map terminal-facing errors onto AppError for signal emission.
fn map_terminal_error(err: &TerminalError) -> AppError {
    match err {
        TerminalError::Input(msg) => AppError::new("INPUT_ERROR", msg),
        TerminalError::Config(msg) => AppError::new("CONFIG_ERROR", msg),
        TerminalError::Capability(msg) => AppError::new("CAPABILITY_DENIED", msg),
        TerminalError::NotFound(msg) => AppError::new("NOT_FOUND", msg),
        TerminalError::Network(msg) => AppError::new("NETWORK_ERROR", msg),
        TerminalError::NotImplemented(msg) => AppError::new("NOT_IMPLEMENTED", msg),
        TerminalError::Operation(msg) => AppError::new("OPERATION_FAILED", msg),
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
                // Now generates proper shareable invitation codes in aura:v1: format
                assert!(
                    code.starts_with("aura:v1:"),
                    "Expected aura:v1: prefix, got: {}",
                    code
                );
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

    #[tokio::test]
    async fn test_import_invitation_parses_valid_code() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        // First export to get a valid code
        let export_result = handler
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: "roundtrip-test".to_string(),
            })
            .await;

        let code = match export_result {
            Some(Ok(OpResponse::InvitationCode { code, .. })) => code,
            _ => panic!("Expected InvitationCode response"),
        };

        // Now import the exported code
        let import_result = handler
            .execute(&EffectCommand::ImportInvitation { code })
            .await;

        match import_result {
            Some(Ok(OpResponse::InvitationImported {
                invitation_id,
                sender_id,
                invitation_type,
                ..
            })) => {
                assert_eq!(invitation_id, "roundtrip-test");
                assert!(!sender_id.is_empty());
                assert_eq!(invitation_type, "contact"); // Default type for minimal invitation
            }
            Some(Err(e)) => panic!("Import failed: {:?}", e),
            _ => panic!("Expected InvitationImported response"),
        }
    }

    #[tokio::test]
    async fn test_import_invitation_rejects_invalid_code() {
        let app_core = test_app_core();
        let handler = OperationalHandler::new(app_core);

        let result = handler
            .execute(&EffectCommand::ImportInvitation {
                code: "invalid-code".to_string(),
            })
            .await;

        match result {
            Some(Err(OpError::InvalidArgument(_))) => {
                // Expected - invalid format
            }
            _ => panic!("Expected InvalidArgument error for invalid code"),
        }
    }
}
