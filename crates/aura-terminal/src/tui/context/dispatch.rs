//! # Command Dispatch Helpers
//!
//! Focused helpers for:
//! - Account file operations (create/restore/backup import/export)
//! - Command dispatch (Operational via OperationalHandler)
//! - Emitting `ERROR_SIGNAL` on all error paths

use aura_core::{AuthorityId, ContextId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_lock::RwLock;

use super::snapshots::StateSnapshotAvailability;
use super::{SnapshotHelper, ToastHelper};
use crate::error::{TerminalError, TerminalResult};
use crate::tui::components::copy_to_clipboard;
use crate::tui::effects::{EffectCommand, OpResponse, OperationalHandler};
use crate::tui::types::ChannelMode;
use aura_app::ui::types::BootstrapRuntimeIdentity;

const BOOTSTRAP_RUNTIME_HANDOFF_READY_FILENAME: &str = ".bootstrap-runtime-handoff-ready";

/// File-based account operations used by the TUI.
///
/// Note: These are currently implemented via the TUI handler helpers (disk I/O).
/// We isolate them here so we can later swap to effect-based storage without
/// touching UI dispatch sites.
///
/// Mode isolation is achieved via mode-specific base_path directories:
/// - Production: `$AURA_PATH/.aura` (default: `~/.aura`)
/// - Demo: `$AURA_PATH/.aura-demo` (default: `~/.aura-demo`)
#[derive(Clone)]
pub struct AccountFilesHelper {
    base_path: PathBuf,
    device_id_str: String,
    has_existing_account: Arc<AtomicBool>,
}

impl AccountFilesHelper {
    pub fn new(
        base_path: PathBuf,
        device_id_str: String,
        has_existing_account: Arc<AtomicBool>,
    ) -> Self {
        Self {
            base_path,
            device_id_str,
            has_existing_account,
        }
    }

    #[must_use]
    pub fn has_account(&self) -> bool {
        self.has_existing_account.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }

    #[must_use]
    pub fn bootstrap_runtime_handoff_committed(&self) -> bool {
        self.base_path
            .join(BOOTSTRAP_RUNTIME_HANDOFF_READY_FILENAME)
            .exists()
    }

    pub fn mark_bootstrap_runtime_handoff_committed(&self) -> TerminalResult<()> {
        std::fs::write(
            self.base_path
                .join(BOOTSTRAP_RUNTIME_HANDOFF_READY_FILENAME),
            b"ready",
        )
        .map_err(|error| {
            TerminalError::structured_operation(
                "TUI_BOOTSTRAP_HANDOFF_PERSIST_FAILED",
                format!("failed to persist bootstrap runtime handoff marker: {error}"),
            )
        })
    }

    pub fn clear_bootstrap_runtime_handoff_committed(&self) -> TerminalResult<()> {
        let path = self
            .base_path
            .join(BOOTSTRAP_RUNTIME_HANDOFF_READY_FILENAME);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(TerminalError::structured_operation(
                "TUI_BOOTSTRAP_HANDOFF_CLEAR_FAILED",
                format!("failed to clear bootstrap runtime handoff marker: {error}"),
            )),
        }
    }

    pub fn set_account_created(&self) {
        self.has_existing_account.store(true, Ordering::Relaxed);
    }

    pub async fn create_account(
        &self,
        nickname_suggestion: &str,
    ) -> TerminalResult<(AuthorityId, ContextId)> {
        match crate::handlers::tui::create_account(&self.base_path, nickname_suggestion).await {
            Ok((authority_id, context_id)) => {
                self.set_account_created();
                Ok((authority_id, context_id))
            }
            Err(e) => {
                tracing::error!("Failed to create account: {}", e);
                Err(TerminalError::Operation(e.to_string()))
            }
        }
    }

    pub async fn create_account_with_device_enrollment(
        &self,
        nickname_suggestion: &str,
        device_enrollment_code: &str,
    ) -> TerminalResult<(AuthorityId, ContextId)> {
        match crate::handlers::tui::create_account_with_device_enrollment(
            &self.base_path,
            nickname_suggestion,
            device_enrollment_code,
        )
        .await
        {
            Ok((authority_id, context_id)) => {
                self.set_account_created();
                Ok((authority_id, context_id))
            }
            Err(e) => {
                tracing::error!("Failed to create account with device enrollment: {}", e);
                Err(TerminalError::Operation(e.to_string()))
            }
        }
    }

    pub async fn create_account_with_device_enrollment_runtime_identity(
        &self,
        runtime_identity: BootstrapRuntimeIdentity,
        nickname_suggestion: &str,
        device_enrollment_code: &str,
    ) -> TerminalResult<(AuthorityId, ContextId)> {
        match crate::handlers::tui::create_account_with_device_enrollment_runtime_identity(
            &self.base_path,
            runtime_identity,
            nickname_suggestion,
            device_enrollment_code,
        )
        .await
        {
            Ok((authority_id, context_id)) => {
                self.set_account_created();
                Ok((authority_id, context_id))
            }
            Err(e) => {
                tracing::error!(
                    "Failed to create account with device enrollment runtime identity: {}",
                    e
                );
                Err(TerminalError::Operation(e.to_string()))
            }
        }
    }

    pub async fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::types::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::types::identifiers::ContextId>,
    ) -> TerminalResult<()> {
        match crate::handlers::tui::restore_recovered_account(
            &self.base_path,
            recovered_authority_id,
            recovered_context_id,
        )
        .await
        {
            Ok((_authority_id, _context_id)) => {
                self.set_account_created();
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to restore recovered account: {}", e);
                Err(TerminalError::Operation(e.to_string()))
            }
        }
    }

    pub async fn export_account_backup(&self) -> TerminalResult<String> {
        if !self.has_account() {
            return Err(TerminalError::NotFound(
                "No account exists to backup".to_string(),
            ));
        }

        crate::handlers::tui::export_account_backup(&self.base_path, Some(&self.device_id_str))
            .await
            .map_err(|e| TerminalError::Operation(e.to_string()))
    }

    pub async fn import_account_backup(&self, backup_code: &str) -> TerminalResult<()> {
        match crate::handlers::tui::import_account_backup(&self.base_path, backup_code, true).await
        {
            Ok((_authority_id, _context_id)) => {
                self.set_account_created();
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to import backup: {}", e);
                Err(TerminalError::Operation(e.to_string()))
            }
        }
    }
}

/// Helper for dispatching commands through `OperationalHandler`.
#[derive(Clone)]
pub struct DispatchHelper {
    operational: Arc<OperationalHandler>,
    snapshots: SnapshotHelper,
    toasts: ToastHelper,
    account_files: AccountFilesHelper,

    // Local, UI-only state updates driven by OpResponse.
    current_context: Arc<RwLock<Option<String>>>,
    channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
    invited_lan_peers: Arc<RwLock<HashSet<AuthorityId>>>,
}

impl DispatchHelper {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operational: Arc<OperationalHandler>,
        snapshots: SnapshotHelper,
        toasts: ToastHelper,
        account_files: AccountFilesHelper,
        invited_lan_peers: Arc<RwLock<HashSet<AuthorityId>>>,
        current_context: Arc<RwLock<Option<String>>>,
        channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
    ) -> Self {
        Self {
            operational,
            snapshots,
            toasts,
            account_files,
            current_context,
            channel_modes,
            invited_lan_peers,
        }
    }

    /// Dispatch a command (fire-and-forget semantics; emits `ERROR_SIGNAL` on failure).
    pub async fn dispatch(&self, command: EffectCommand) -> TerminalResult<()> {
        if let Err(error) = self.check_authorization(&command) {
            self.operational.emit_error(error.clone()).await;
            return Err(error);
        }

        // Backup commands need filesystem access; handle here.
        match &command {
            EffectCommand::ExportAccountBackup => {
                // Show the code via a toast so users can copy it.
                match self.account_files.export_account_backup().await {
                    Ok(code) => {
                        self.toasts
                            .success("account-backup", format!("Backup code: {code}"))
                            .await;
                        return Ok(());
                    }
                    Err(error) => {
                        self.operational.emit_error(error.clone()).await;
                        return Err(error);
                    }
                }
            }
            EffectCommand::ImportAccountBackup { backup_code } => {
                match self.account_files.import_account_backup(backup_code).await {
                    Ok(()) => {
                        self.toasts
                            .success("account-backup", "Backup imported successfully")
                            .await;
                        return Ok(());
                    }
                    Err(error) => {
                        self.operational.emit_error(error.clone()).await;
                        return Err(error);
                    }
                }
            }
            _ => {}
        }

        // Operational path (runtime-backed).
        if let Some(result) = self.operational.execute_with_errors(&command).await {
            // Operational path.
            match result {
                Ok(response) => self.handle_op_response(response).await,
                Err(e) => {
                    tracing::error!("dispatch operation error: {e}");
                    Err(e)
                }
            }
        } else {
            // Unknown command.
            tracing::warn!("Unknown command not handled by Operational: {:?}", command);
            let error = TerminalError::structured_operation(
                "UNHANDLED_COMMAND",
                format!("Operational pipeline has no handler for command: {command:?}"),
            );
            self.operational.emit_error(error.clone()).await;
            Err(error)
        }
    }

    pub async fn dispatch_with_response(
        &self,
        command: EffectCommand,
    ) -> TerminalResult<OpResponse> {
        if let Err(error) = self.check_authorization(&command) {
            self.operational.emit_error(error.clone()).await;
            return Err(error);
        }

        match &command {
            EffectCommand::ExportAccountBackup => {
                match self.account_files.export_account_backup().await {
                    Ok(code) => {
                        self.toasts
                            .success("account-backup", format!("Backup code: {code}"))
                            .await;
                        return Ok(OpResponse::Data(code));
                    }
                    Err(error) => {
                        self.operational.emit_error(error.clone()).await;
                        return Err(error);
                    }
                }
            }
            EffectCommand::ImportAccountBackup { backup_code } => {
                match self.account_files.import_account_backup(backup_code).await {
                    Ok(()) => {
                        self.toasts
                            .success("account-backup", "Backup imported successfully")
                            .await;
                        return Ok(OpResponse::Ok);
                    }
                    Err(error) => {
                        self.operational.emit_error(error.clone()).await;
                        return Err(error);
                    }
                }
            }
            _ => {}
        }

        if let Some(result) = self.operational.execute_with_errors(&command).await {
            match result {
                Ok(response) => {
                    self.handle_op_response(response.clone()).await?;
                    Ok(response)
                }
                Err(e) => {
                    tracing::error!("dispatch operation error: {e}");
                    Err(e)
                }
            }
        } else {
            tracing::warn!("Unknown command not handled by Operational: {:?}", command);
            let error = TerminalError::structured_operation(
                "UNHANDLED_COMMAND",
                format!("Operational pipeline has no handler for command: {command:?}"),
            );
            self.operational.emit_error(error.clone()).await;
            Err(error)
        }
    }

    async fn handle_op_response(&self, response: OpResponse) -> TerminalResult<()> {
        match response {
            OpResponse::ContextChanged { context_id } => {
                *self.current_context.write().await = context_id;
                Ok(())
            }
            OpResponse::ChannelModeSet { channel_id, flags } => {
                let mut modes = self.channel_modes.write().await;
                let mode = modes.entry(channel_id).or_default();
                mode.parse_flags(&flags);
                Ok(())
            }
            OpResponse::NicknameUpdated { name: _ } => Ok(()),
            OpResponse::MfaPolicyUpdated { require_mfa: _ } => Ok(()),
            OpResponse::InvitationImported {
                invitation_type: _,
                message,
                ..
            } => {
                // Importing a demo contact code triggers a runtime-backed accept which
                // commits a ContactFact; the reactive scheduler will update CONTACTS_SIGNAL.
                let summary = message.unwrap_or_else(|| "Invitation imported".to_string());
                self.toasts.success("invitation", summary).await;
                Ok(())
            }
            OpResponse::InvitationCode { id: _, code } => {
                self.toasts
                    .success("invitation-code", format!("Invitation code: {code}"))
                    .await;
                Ok(())
            }
            OpResponse::DeviceEnrollmentStarted {
                ceremony_id: _,
                enrollment_code,
                pending_epoch: _,
                device_id: _,
            } => {
                let _ = copy_to_clipboard(&enrollment_code);
                self.toasts
                    .success(
                        "device-enrollment",
                        format!("Device enrollment code: {enrollment_code}"),
                    )
                    .await;
                Ok(())
            }
            OpResponse::DeviceRemovalStarted { ceremony_id: _ } => {
                self.toasts
                    .success("device-removal", "Device removal started".to_string())
                    .await;
                Ok(())
            }

            OpResponse::Ok
            | OpResponse::Data(_)
            | OpResponse::List(_)
            | OpResponse::PeersListed { .. }
            | OpResponse::LanPeersListed { .. }
            | OpResponse::PeerDiscoveryTriggered { .. }
            | OpResponse::LanInvitationStatus { .. }
            | OpResponse::ParticipantsListed { .. }
            | OpResponse::UserInfo { .. }
            | OpResponse::RecoveryStarted { .. }
            | OpResponse::RecoveryCancelled
            | OpResponse::RecoveryCompleted
            | OpResponse::RecoveryGuardianInvited { .. }
            | OpResponse::HomeInvitationAccepted { .. }
            | OpResponse::InvitationAccepted { .. }
            | OpResponse::HomeCreated { .. }
            | OpResponse::NeighborhoodCreated { .. }
            | OpResponse::HomeAddedToNeighborhood { .. }
            | OpResponse::HomeOneHopLinkSet { .. }
            | OpResponse::ChannelMessageSent { .. }
            | OpResponse::ChannelCreated { .. }
            | OpResponse::DirectMessageSent { .. }
            | OpResponse::ActionSent { .. }
            | OpResponse::ChannelInvitationSent { .. }
            | OpResponse::ChannelJoined { .. }
            | OpResponse::RetrySent { .. }
            | OpResponse::PeerStateRequested { .. }
            | OpResponse::ContactGuardianToggled { .. } => Ok(()),
        }
    }

    /// Best-effort authorization gate for Admin-level commands.
    ///
    /// This does not replace Biscuit enforcement (guard chain); it provides
    /// immediate UX feedback and avoids attempting admin ops outside a home.
    fn check_authorization(&self, command: &EffectCommand) -> TerminalResult<()> {
        use crate::tui::effects::CommandAuthorizationLevel;

        let level = command.authorization_level();
        match level {
            CommandAuthorizationLevel::Public
            | CommandAuthorizationLevel::Basic
            | CommandAuthorizationLevel::Sensitive => Ok(()),
            CommandAuthorizationLevel::Admin => {
                if has_explicit_admin_scope(command) {
                    return Ok(());
                }
                match self.snapshots.state_snapshot_availability() {
                    StateSnapshotAvailability::Available(snapshot) => {
                        aura_app::ui::authorization::require_admin(
                            Some(&snapshot),
                            command_name(command),
                        )
                        .map_err(|e| TerminalError::Capability(e.to_string()))
                    }
                    StateSnapshotAvailability::Contended => Err(TerminalError::Capability(
                        format!(
                            "{} requires an authoritative home snapshot, but the snapshot lock is contended",
                            command_name(command)
                        ),
                    )),
                }
            }
        }
    }

    pub async fn mark_peer_invited(&self, authority_id: &AuthorityId) {
        self.invited_lan_peers.write().await.insert(*authority_id);
    }

    pub async fn is_peer_invited(&self, authority_id: &AuthorityId) -> bool {
        self.invited_lan_peers.read().await.contains(authority_id)
    }

    pub async fn invited_peer_ids(&self) -> HashSet<AuthorityId> {
        self.invited_lan_peers.read().await.clone()
    }
}

fn command_name(command: &EffectCommand) -> &'static str {
    match command {
        EffectCommand::KickUser { .. } => "Kick user",
        EffectCommand::BanUser { .. } => "Ban user",
        EffectCommand::UnbanUser { .. } => "Unban user",
        EffectCommand::GrantModerator { .. } => "Grant moderator",
        EffectCommand::RevokeModerator { .. } => "Revoke moderator",
        EffectCommand::SetChannelMode { .. } => "Set channel mode",
        EffectCommand::Shutdown => "Shutdown",
        _ => "This operation",
    }
}

fn has_explicit_admin_scope(command: &EffectCommand) -> bool {
    match command {
        EffectCommand::KickUser { channel, .. } => !channel.trim().is_empty(),
        EffectCommand::BanUser { channel, .. } => channel
            .as_deref()
            .is_some_and(|channel| !channel.trim().is_empty()),
        EffectCommand::UnbanUser { channel, .. } => channel
            .as_deref()
            .is_some_and(|channel| !channel.trim().is_empty()),
        EffectCommand::GrantModerator { channel, .. } => channel
            .as_deref()
            .is_some_and(|channel| !channel.trim().is_empty()),
        EffectCommand::RevokeModerator { channel, .. } => channel
            .as_deref()
            .is_some_and(|channel| !channel.trim().is_empty()),
        EffectCommand::SetChannelMode { channel, .. } => !channel.trim().is_empty(),
        _ => false,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect() for cleaner error handling
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use async_lock::RwLock;
    use aura_app::ui::prelude::*;
    use aura_app::ui::signals::ERROR_SIGNAL;
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::effects::PhysicalTimeEffects;
    use aura_core::{
        execute_with_timeout_budget, TimeoutBudget, TimeoutExecutionProfile, TimeoutRunError,
    };
    use aura_effects::time::PhysicalTimeHandler;

    use crate::handlers::tui::TuiMode;
    use crate::tui::context::{AccountFilesHelper, DispatchHelper, InitializedAppCore, IoContext};
    use crate::tui::context::{SnapshotHelper, ToastHelper};
    use crate::tui::effects::EffectCommand;
    use crate::tui::effects::OperationalHandler;
    use crate::tui::tasks::UiTaskOwner;
    use crate::tui::types::ChannelMode;
    use crate::TerminalError;

    async fn wait_for_error(app_core: &Arc<RwLock<AppCore>>) -> AppError {
        let time = PhysicalTimeHandler::new();
        let started_at = time
            .physical_time()
            .await
            .expect("failed to read physical time");
        let timeout = TimeoutExecutionProfile::simulation_test()
            .scale_duration(Duration::from_millis(500))
            .expect("failed to scale timeout");
        let budget =
            TimeoutBudget::from_start_and_timeout(&started_at, timeout).expect("budget should fit");
        match execute_with_timeout_budget(&time, &budget, || async {
            loop {
                {
                    let core = app_core.read().await;
                    if let Ok(Some(err)) = core.read(&*ERROR_SIGNAL).await {
                        return Ok::<_, std::convert::Infallible>(err);
                    }
                }
                time.sleep_ms(10)
                    .await
                    .expect("sleep should succeed while waiting for error signal");
            }
        })
        .await
        {
            Ok(error) => error,
            Err(TimeoutRunError::Timeout(error)) => {
                panic!("Timed out waiting for ERROR_SIGNAL to become Some: {error}")
            }
            Err(TimeoutRunError::Operation(error)) => match error {},
        }
    }

    fn test_dispatch_helper(app_core: Arc<RwLock<AppCore>>, base_path: PathBuf) -> DispatchHelper {
        DispatchHelper::new(
            Arc::new(OperationalHandler::new(
                app_core.clone(),
                Arc::new(UiTaskOwner::new()),
            )),
            SnapshotHelper::new(app_core, "test-device"),
            ToastHelper::new(),
            AccountFilesHelper::new(
                base_path,
                "test-device".to_string(),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ),
            Arc::new(RwLock::new(std::collections::HashSet::new())),
            Arc::new(RwLock::new(None)),
            Arc::new(RwLock::new(
                std::collections::HashMap::<String, ChannelMode>::new(),
            )),
        )
    }

    #[tokio::test]
    async fn unknown_command_emits_error_signal() {
        let app_core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let app_core = InitializedAppCore::new(app_core)
            .await
            .expect("Failed to init signals");

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = IoContext::builder()
            .with_app_core(app_core.clone())
            .with_base_path(dir.path().to_path_buf())
            .with_device_id("test-device".to_string())
            .with_mode(TuiMode::Production)
            .with_existing_account(false)
            .build()
            .expect("Failed to build IoContext");

        let _ = ctx.dispatch(EffectCommand::UnknownCommandForTest).await;
        let err = wait_for_error(app_core.raw()).await;
        assert_eq!(err.code(), "INTERNAL");
        assert!(err.to_string().contains("no handler for command"));
    }

    /// Regression test: CreateAccount dispatch must not be rejected as an unhandled command.
    ///
    /// Previously, `EffectCommand::CreateAccount` was not handled by any operational
    /// sub-handler, so the dispatch layer emitted:
    ///   INTERNAL: operation: Operational pipeline has no handler for command: CreateAccount { nickname_suggestion: "..." }
    #[tokio::test]
    async fn create_account_dispatch_does_not_emit_unknown_command() {
        let app_core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let app_core = InitializedAppCore::new(app_core)
            .await
            .expect("Failed to init signals");

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = IoContext::builder()
            .with_app_core(app_core.clone())
            .with_base_path(dir.path().to_path_buf())
            .with_device_id("test-device".to_string())
            .with_mode(TuiMode::Production)
            .with_existing_account(false)
            .build()
            .expect("Failed to build IoContext");

        let result = ctx
            .dispatch(EffectCommand::CreateAccount {
                nickname_suggestion: "Sam2".to_string(),
            })
            .await;

        // The command may succeed or fail for domain reasons, but it must NOT
        // fail as an unhandled command — that means no handler recognized it.
        if let Err(TerminalError::Operation(message)) = result {
            assert!(
                !message.contains("no handler for command"),
                "CreateAccount must be handled, not rejected as unknown. Got: {message}"
            );
        }
    }

    #[tokio::test]
    async fn admin_authorization_reports_snapshot_contention_explicitly() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).expect("Failed to create test AppCore"),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .expect("Failed to init signals for test AppCore");

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dispatch = test_dispatch_helper(app_core.clone(), dir.path().to_path_buf());
        let _guard = app_core.write().await;

        let error = dispatch
            .check_authorization(&EffectCommand::Shutdown)
            .expect_err("contended snapshot must reject admin command");

        match error {
            TerminalError::Capability(message) => {
                assert!(
                    message.contains("authoritative home snapshot")
                        && message.contains("contended"),
                    "expected explicit contention message, got: {message}"
                );
            }
            other => panic!("expected capability error, got: {other:?}"),
        }
    }
}
