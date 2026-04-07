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
//! - **Invite Codes**: ExportInvitation, ImportInvitation
//! - **Recovery**: StartRecovery, SubmitGuardianApproval, CompleteRecovery, CancelRecovery
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
//! - `recovery`: Recovery commands (StartRecovery, SubmitGuardianApproval, etc.)
//! - `messaging`: Direct messaging commands
//! - `moderator`: Moderator role management commands

mod contacts;
mod context;
mod invitations;
mod messaging;
mod moderation;
mod moderator;
mod network;
mod query;
mod recovery;
mod settings;
mod sync;
mod system;
mod time;
pub mod types;

use std::sync::Arc;
use std::time::Duration;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::{ConnectionStatus, SyncStatus, ERROR_SIGNAL};
use aura_core::effects::reactive::ReactiveEffects;
use std::convert::Infallible;

pub use types::{OpError, OpFailureCode, OpResponse, OpResult};

use super::EffectCommand;
use crate::error::TerminalError;
use crate::tui::tasks::UiTaskOwner;
use crate::tui::timeout_support::{execute_with_terminal_timeout, TerminalTimeoutError};

/// Handles operational commands that don't create journal facts.
///
/// This handler processes commands that
/// are purely runtime operations (sync, peer management, etc.).
///
/// Note: Peer state is managed through AppCore signals (via the network workflow),
/// not through local state in this handler.
pub struct OperationalHandler {
    app_core: Arc<RwLock<AppCore>>,
    tasks: Arc<UiTaskOwner>,
}

impl OperationalHandler {
    /// Create a new operational handler
    pub fn new(app_core: Arc<RwLock<AppCore>>, tasks: Arc<UiTaskOwner>) -> Self {
        Self { app_core, tasks }
    }

    /// Execute an operational command
    ///
    /// Returns `Some(result)` if the command was handled, `None` if it should
    /// be handled elsewhere (e.g., by intent dispatch).
    pub async fn execute(&self, command: &EffectCommand) -> Option<OpResult> {
        // Exhaustive match ensures every EffectCommand variant is routed to a handler.
        // Adding a new variant without a routing arm is a compile error.
        match command {
            // System
            EffectCommand::Ping
            | EffectCommand::Shutdown
            | EffectCommand::RefreshAccount
            | EffectCommand::CreateAccount { .. } => {
                system::handle_system(command, &self.app_core).await
            }

            // Sync
            EffectCommand::ForceSync | EffectCommand::RequestState { .. } => {
                sync::handle_sync(command, &self.app_core).await
            }

            // Network
            EffectCommand::AddPeer { .. }
            | EffectCommand::RemovePeer { .. }
            | EffectCommand::ListPeers
            | EffectCommand::DiscoverPeers
            | EffectCommand::ListLanPeers
            | EffectCommand::InviteLanPeer { .. } => {
                network::handle_network(command, &self.app_core).await
            }

            // Query
            EffectCommand::ListParticipants { .. } | EffectCommand::GetUserInfo { .. } => {
                query::handle_query(command, &self.app_core).await
            }

            // Context + Home
            EffectCommand::SetContext { .. }
            | EffectCommand::MovePosition { .. }
            | EffectCommand::AcceptPendingChannelInvitation
            | EffectCommand::CreateHome { .. }
            | EffectCommand::CreateNeighborhood { .. }
            | EffectCommand::AddHomeToNeighborhood { .. }
            | EffectCommand::LinkHomeOneHopLink { .. } => {
                context::handle_context(command, &self.app_core).await
            }

            // Contacts
            EffectCommand::UpdateContactNickname { .. }
            | EffectCommand::RemoveContact { .. }
            | EffectCommand::ToggleContactGuardian { .. } => {
                contacts::handle_contacts(command, &self.app_core).await
            }

            // Settings
            EffectCommand::AddDevice { .. }
            | EffectCommand::RemoveDevice { .. }
            | EffectCommand::UpdateMfaPolicy { .. }
            | EffectCommand::UpdateNickname { .. }
            | EffectCommand::UpdateThreshold { .. }
            | EffectCommand::SetChannelMode { .. } => {
                settings::handle_settings(command, &self.app_core).await
            }

            // Invitations
            EffectCommand::CreateInvitation { .. }
            | EffectCommand::SendHomeInvitation { .. }
            | EffectCommand::ExportInvitation { .. }
            | EffectCommand::ImportInvitation { .. }
            | EffectCommand::AcceptInvitation { .. }
            | EffectCommand::DeclineInvitation { .. }
            | EffectCommand::CancelInvitation { .. } => {
                invitations::handle_invitations(command, &self.app_core).await
            }

            // Recovery
            // InviteGuardian routes here: recovery.rs handles both contact_id=None
            // and contact_id=Some cases.
            EffectCommand::StartRecovery
            | EffectCommand::SubmitGuardianApproval { .. }
            | EffectCommand::CompleteRecovery
            | EffectCommand::CancelRecovery
            | EffectCommand::InviteGuardian { .. } => {
                recovery::handle_recovery(command, &self.app_core).await
            }

            // Messaging
            EffectCommand::SendMessage { .. }
            | EffectCommand::CreateChannel { .. }
            | EffectCommand::CloseChannel { .. }
            | EffectCommand::SendDirectMessage { .. }
            | EffectCommand::StartDirectChat { .. }
            | EffectCommand::SendAction { .. }
            | EffectCommand::JoinChannel { .. }
            | EffectCommand::LeaveChannel { .. }
            | EffectCommand::RetryMessage { .. }
            | EffectCommand::SetTopic { .. }
            | EffectCommand::InviteUser { .. } => {
                messaging::handle_messaging(command, &self.app_core, &self.tasks).await
            }

            // Moderation
            EffectCommand::KickUser { .. }
            | EffectCommand::BanUser { .. }
            | EffectCommand::UnbanUser { .. }
            | EffectCommand::MuteUser { .. }
            | EffectCommand::UnmuteUser { .. }
            | EffectCommand::PinMessage { .. }
            | EffectCommand::UnpinMessage { .. } => {
                moderation::handle_moderation(command, &self.app_core).await
            }

            // Moderator
            EffectCommand::GrantModerator { .. } | EffectCommand::RevokeModerator { .. } => {
                moderator::handle_moderator(command, &self.app_core).await
            }

            // Backup commands — handled by DispatchHelper before reaching here.
            // Return None so the dispatch layer handles them directly.
            EffectCommand::ExportAccountBackup | EffectCommand::ImportAccountBackup { .. } => None,

            // Test-only: intentionally unhandled to exercise "unknown command" error path.
            #[cfg(test)]
            EffectCommand::UnknownCommandForTest => None,
        }
    }

    /// Execute and map failures to TerminalError while emitting ERROR_SIGNAL.
    pub async fn execute_with_errors(
        &self,
        command: &EffectCommand,
    ) -> Option<Result<OpResponse, TerminalError>> {
        match self.execute(command).await {
            Some(Ok(resp)) => Some(Ok(resp)),
            Some(Err(err)) => {
                let terr: TerminalError = err.into();
                self.emit_error(terr.clone()).await;
                Some(Err(terr))
            }
            None => None,
        }
    }

    /// Update connection status signal
    pub async fn set_connection_status(&self, status: ConnectionStatus) {
        let _ =
            aura_app::ui::workflows::network::set_connection_status(&self.app_core, status).await;
    }

    /// Update sync status signal
    pub async fn set_sync_status(&self, status: SyncStatus) {
        let _ = aura_app::ui::workflows::sync::set_sync_status(&self.app_core, status).await;
    }

    /// Emit an error to the error signal.
    ///
    /// Uses a bounded read to avoid silently discarding errors when the
    /// AppCore write lock is held. On contention timeout, logs a structured
    /// warning so the failure is observable.
    pub async fn emit_error(&self, error: TerminalError) {
        let mapped = map_terminal_error(&error);
        match execute_with_terminal_timeout(
            "error_signal_emit",
            Duration::from_millis(500),
            || async { Ok::<_, Infallible>(self.app_core.read().await) },
        )
        .await
        {
            Ok(core) => {
                let _ = core.emit(&*ERROR_SIGNAL, Some(mapped)).await;
            }
            Err(TerminalTimeoutError::Timeout) => {
                tracing::warn!(
                    error_code = "ERROR_SIGNAL_CONTENDED",
                    original_error = %error,
                    "failed to emit ERROR_SIGNAL: AppCore write-locked for >500ms"
                );
            }
            Err(TerminalTimeoutError::Setup { context, detail }) => {
                tracing::warn!(
                    error_code = "ERROR_SIGNAL_TIMEOUT_SETUP_FAILED",
                    timeout_context = context,
                    %detail,
                    original_error = %error,
                    "failed to emit ERROR_SIGNAL because terminal timeout setup failed"
                );
            }
            Err(TerminalTimeoutError::Operation(error)) => match error {},
        }
    }

    /// Clear the error signal.
    pub async fn clear_error(&self) {
        match execute_with_terminal_timeout(
            "error_signal_clear",
            Duration::from_millis(500),
            || async { Ok::<_, Infallible>(self.app_core.read().await) },
        )
        .await
        {
            Ok(core) => {
                let _ = core.emit(&*ERROR_SIGNAL, None).await;
            }
            Err(TerminalTimeoutError::Timeout) => {
                tracing::warn!(
                    error_code = "ERROR_SIGNAL_CONTENDED",
                    "failed to clear ERROR_SIGNAL: AppCore write-locked for >500ms"
                );
            }
            Err(TerminalTimeoutError::Setup { context, detail }) => {
                tracing::warn!(
                    error_code = "ERROR_SIGNAL_TIMEOUT_SETUP_FAILED",
                    timeout_context = context,
                    %detail,
                    "failed to clear ERROR_SIGNAL because terminal timeout setup failed"
                );
            }
            Err(TerminalTimeoutError::Operation(error)) => match error {},
        }
    }

    /// Convert OpResult to Result<OpResponse, String> and emit errors to ERROR_SIGNAL.
    ///
    /// This helper simplifies error handling in operational command execution by:
    /// 1. Converting OpError → TerminalError
    /// 2. Emitting the error to ERROR_SIGNAL for UI feedback
    /// 3. Returning a String error for call-site handling
    ///
    /// # Example
    /// ```ignore
    /// let result = self.operational.execute(&command).await;
    /// match self.operational.handle_op_result(result).await {
    ///     Ok(response) => // handle response
    ///     Err(msg) => // error already emitted to ERROR_SIGNAL
    /// }
    /// ```
    pub async fn handle_op_result(
        &self,
        result: Option<OpResult>,
    ) -> Option<Result<OpResponse, String>> {
        match result {
            Some(Ok(response)) => Some(Ok(response)),
            Some(Err(e)) => {
                let terr: TerminalError = e.into();
                self.emit_error(terr.clone()).await;
                Some(Err(terr.to_string()))
            }
            None => None,
        }
    }
}

/// Map terminal-facing errors onto AppError for signal emission.
fn map_terminal_error(err: &TerminalError) -> AppError {
    match err {
        TerminalError::Input(msg) => AppError::user_action("Invalid input", msg),
        TerminalError::Config(msg) => AppError::internal("config", msg),
        TerminalError::Capability(msg) => AppError::auth(AuthFailure::CapabilityDenied, msg),
        TerminalError::NotFound(msg) => AppError::user_action("Not found", msg),
        TerminalError::Network(msg) => AppError::network(NetworkErrorCode::Other, msg),
        TerminalError::NotImplemented(msg) => AppError::internal("not_implemented", msg),
        TerminalError::Operation(msg) => AppError::internal("operation", msg),
        TerminalError::StructuredOperation { code, message } => AppError::internal(*code, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui::types::AppConfig;

    async fn test_app_core() -> Arc<RwLock<AppCore>> {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).expect("Failed to create test AppCore"),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .expect("Failed to init signals");
        app_core
    }

    fn test_handler(app_core: Arc<RwLock<AppCore>>) -> OperationalHandler {
        OperationalHandler::new(app_core, Arc::new(crate::tui::tasks::UiTaskOwner::new()))
    }

    #[tokio::test]
    async fn test_ping_command() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        let result = handler.execute(&EffectCommand::Ping).await;
        assert!(matches!(result, Some(Ok(OpResponse::Ok))));
    }

    #[tokio::test]
    async fn test_list_peers_returns_list() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        let result = handler.execute(&EffectCommand::ListPeers).await;
        match result {
            Some(Ok(OpResponse::PeersListed { .. })) => {}
            Some(Err(OpError::TypedFailure(failure))) => {
                assert!(
                    failure.message().contains("Runtime bridge not available")
                        || failure.message().contains("Failed to query")
                        || failure.message().contains("No agent configured")
                        || failure.message().contains("requires a runtime"),
                    "Expected runtime-availability error, got: {failure}"
                );
            }
            Some(Err(OpError::Failed(msg))) => {
                assert!(
                    msg.contains("Runtime bridge not available")
                        || msg.contains("Failed to query")
                        || msg.contains("No agent configured")
                        || msg.contains("requires a runtime"),
                    "Expected runtime-availability error, got: {msg}"
                );
            }
            _ => panic!("Expected peer list or runtime error, got: {result:?}"),
        }
    }

    #[tokio::test]
    async fn test_export_invitation_fails_without_runtime() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        // Without RuntimeBridge, export should fail gracefully
        let result = handler
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: "test-123".to_string(),
            })
            .await;

        match result {
            Some(Err(OpError::TypedFailure(failure))) => {
                assert_eq!(
                    failure.code(),
                    crate::tui::effects::operational::types::OpFailureCode::ExportInvitation
                );
                assert!(
                    failure.message().contains("Runtime bridge not available")
                        || failure.message().contains("Failed to export"),
                    "Expected runtime error, got: {failure}"
                );
            }
            Some(Err(OpError::Failed(msg))) => {
                assert!(
                    msg.contains("Runtime bridge not available")
                        || msg.contains("Failed to export"),
                    "Expected runtime error, got: {msg}"
                );
            }
            _ => panic!("Expected Failed error without RuntimeBridge, got: {result:?}"),
        }
    }

    #[tokio::test]
    async fn test_chat_commands_work_in_local_mode() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        // LocalOnly messaging still requires a real local channel; it no longer
        // synthesizes a send target via fallback resolution.
        let result = handler
            .execute(&EffectCommand::CreateChannel {
                name: "general".to_string(),
                topic: None,
                members: Vec::new(),
                threshold_k: 1,
            })
            .await;
        assert!(
            matches!(result, Some(Ok(OpResponse::ChannelCreated { .. }))),
            "CreateChannel should succeed in LocalOnly mode, got: {result:?}"
        );

        // Messaging commands now work in LocalOnly mode without RuntimeBridge
        let result = handler
            .execute(&EffectCommand::SendMessage {
                channel: "general".to_string(),
                content: "Hello".to_string(),
            })
            .await;
        assert!(
            matches!(result, Some(Ok(OpResponse::ChannelMessageSent { .. }))),
            "SendMessage should succeed in LocalOnly mode, got: {result:?}"
        );

        // Additional channel creation should also succeed in LocalOnly mode
        let result = handler
            .execute(&EffectCommand::CreateChannel {
                name: "Guardians".to_string(),
                topic: Some("Guardian coordination".to_string()),
                members: vec!["authority-00000000-0000-0000-0000-000000000000".to_string()],
                threshold_k: 2,
            })
            .await;
        assert!(
            matches!(result, Some(Ok(OpResponse::ChannelCreated { .. }))),
            "CreateChannel should succeed in LocalOnly mode, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_import_invitation_fails_without_runtime() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        // Without RuntimeBridge, import should fail gracefully
        // Use a valid-format code to test the runtime requirement, not format validation
        let result = handler
            .execute(&EffectCommand::ImportInvitation {
                code: "aura:v1:eyJ0ZXN0IjoidmFsdWUifQ==".to_string(),
            })
            .await;

        match result {
            Some(Err(OpError::InvalidArgument(msg))) => {
                // Expected - either invalid format or runtime unavailable
                assert!(
                    msg.contains("Runtime bridge not available")
                        || msg.contains("Invalid invitation")
                        || msg.contains("Failed"),
                    "Expected error message, got: {msg}"
                );
            }
            Some(Err(OpError::TypedFailure(failure))) => {
                assert!(
                    failure.message().contains("Runtime bridge not available")
                        || failure.message().contains("Invalid invitation")
                        || failure.message().contains("Failed"),
                    "Expected error message, got: {failure}"
                );
            }
            _ => {
                panic!("Expected invitation import failure without RuntimeBridge, got: {result:?}")
            }
        }
    }

    /// Regression test: CreateAccount must be handled by the operational handler.
    ///
    /// Previously, `EffectCommand::CreateAccount` was not matched by any operational
    /// sub-handler, so `execute()` returned `None` and the dispatch layer emitted:
    ///   INTERNAL: operation: Unknown command: CreateAccount { nickname_suggestion: "..." }
    #[tokio::test]
    async fn test_create_account_is_handled() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        let result = handler
            .execute(&EffectCommand::CreateAccount {
                nickname_suggestion: "Sam2".to_string(),
            })
            .await;

        // Must return Some(_) — i.e. the command is recognized.
        // None would mean "Unknown command" in the dispatch layer.
        assert!(
            result.is_some(),
            "CreateAccount must be handled by OperationalHandler, got None (Unknown command)"
        );
    }

    #[tokio::test]
    async fn test_import_invitation_rejects_invalid_code() {
        let app_core = test_app_core().await;
        let handler = test_handler(app_core);

        let result = handler
            .execute(&EffectCommand::ImportInvitation {
                code: "invalid-code".to_string(),
            })
            .await;

        match result {
            Some(Err(OpError::InvalidArgument(_))) => {
                // Expected - invalid format
            }
            Some(Err(OpError::TypedFailure(failure))) => {
                assert!(
                    failure.message().contains("Invalid invite code")
                        || failure.message().contains("Invalid invitation"),
                    "Expected invalid invitation failure, got: {failure}"
                );
            }
            _ => panic!("Expected invalid invitation error for invalid code"),
        }
    }
}
