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

use super::{SnapshotHelper, ToastHelper};
use crate::error::TerminalError;
use crate::tui::effects::{EffectCommand, OpResponse, OperationalHandler};
use crate::tui::types::ChannelMode;

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

    pub fn has_account(&self) -> bool {
        self.has_existing_account.load(Ordering::Relaxed)
    }

    pub fn set_account_created(&self) {
        self.has_existing_account.store(true, Ordering::Relaxed);
    }

    pub async fn create_account(
        &self,
        display_name: &str,
    ) -> Result<(AuthorityId, ContextId), String> {
        match crate::handlers::tui::create_account(
            &self.base_path,
            &self.device_id_str,
            display_name,
        )
        .await
        {
            Ok((authority_id, context_id)) => {
                self.set_account_created();
                Ok((authority_id, context_id))
            }
            Err(e) => {
                tracing::error!("Failed to create account: {}", e);
                Err(format!("Failed to create account: {e}"))
            }
        }
    }

    pub async fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::identifiers::ContextId>,
    ) -> Result<(), String> {
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
                Err(format!("Failed to restore recovered account: {e}"))
            }
        }
    }

    pub async fn export_account_backup(&self) -> Result<String, String> {
        if !self.has_account() {
            return Err("No account exists to backup".to_string());
        }

        crate::handlers::tui::export_account_backup(&self.base_path, Some(&self.device_id_str))
            .await
            .map_err(|e| format!("Failed to export backup: {e}"))
    }

    pub async fn import_account_backup(&self, backup_code: &str) -> Result<(), String> {
        match crate::handlers::tui::import_account_backup(&self.base_path, backup_code, true).await
        {
            Ok((_authority_id, _context_id)) => {
                self.set_account_created();
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to import backup: {}", e);
                Err(format!("Failed to import backup: {e}"))
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
    invited_lan_peers: Arc<RwLock<HashSet<String>>>,
}

impl DispatchHelper {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operational: Arc<OperationalHandler>,
        snapshots: SnapshotHelper,
        toasts: ToastHelper,
        account_files: AccountFilesHelper,
        invited_lan_peers: Arc<RwLock<HashSet<String>>>,
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
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        if let Err(msg) = self.check_authorization(&command) {
            self.operational
                .emit_error(TerminalError::Capability(msg.clone()))
                .await;
            return Err(msg);
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
                    Err(msg) => {
                        self.operational
                            .emit_error(TerminalError::Operation(msg.clone()))
                            .await;
                        return Err(msg);
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
                    Err(msg) => {
                        self.operational
                            .emit_error(TerminalError::Operation(msg.clone()))
                            .await;
                        return Err(msg);
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
                Err(e) => Err(e.to_string()),
            }
        } else {
            // Unknown command.
            tracing::warn!("Unknown command not handled by Operational: {:?}", command);
            let msg = format!("Unknown command: {command:?}");
            self.operational
                .emit_error(TerminalError::Operation(msg.clone()))
                .await;
            Err(msg)
        }
    }

    /// Dispatch a command and wait for completion.
    ///
    /// This is an alias for `dispatch()` with more explicit semantics.
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        self.dispatch(command).await
    }

    async fn handle_op_response(&self, response: OpResponse) -> Result<(), String> {
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

            OpResponse::Ok | OpResponse::Data(_) | OpResponse::List(_) => Ok(()),
        }
    }

    /// Best-effort authorization gate for Admin-level commands.
    ///
    /// This does not replace Biscuit enforcement (guard chain); it provides
    /// immediate UX feedback and avoids attempting admin ops outside a home.
    fn check_authorization(&self, command: &EffectCommand) -> Result<(), String> {
        use crate::tui::effects::CommandAuthorizationLevel;
        use aura_app::views::home::ResidentRole;

        let level = command.authorization_level();
        match level {
            CommandAuthorizationLevel::Public
            | CommandAuthorizationLevel::Basic
            | CommandAuthorizationLevel::Sensitive => Ok(()),
            CommandAuthorizationLevel::Admin => {
                let snapshot = self.snapshots.try_state_snapshot();
                let role = snapshot.and_then(|s| s.homes.current_home().map(|h| h.my_role));
                match role {
                    Some(ResidentRole::Admin | ResidentRole::Owner) => Ok(()),
                    Some(ResidentRole::Resident) => Err(format!(
                        "Permission denied: {} requires administrator privileges",
                        command_name(command)
                    )),
                    None => Err(format!(
                        "Permission denied: {} requires a home context",
                        command_name(command)
                    )),
                }
            }
        }
    }

    pub async fn mark_peer_invited(&self, authority_id: &str) {
        self.invited_lan_peers
            .write()
            .await
            .insert(authority_id.to_string());
    }

    pub async fn is_peer_invited(&self, authority_id: &str) -> bool {
        self.invited_lan_peers.read().await.contains(authority_id)
    }

    pub async fn invited_peer_ids(&self) -> HashSet<String> {
        self.invited_lan_peers.read().await.clone()
    }
}

fn command_name(command: &EffectCommand) -> &'static str {
    match command {
        EffectCommand::KickUser { .. } => "Kick user",
        EffectCommand::BanUser { .. } => "Ban user",
        EffectCommand::UnbanUser { .. } => "Unban user",
        EffectCommand::GrantSteward { .. } => "Grant steward",
        EffectCommand::RevokeSteward { .. } => "Revoke steward",
        EffectCommand::SetChannelMode { .. } => "Set channel mode",
        EffectCommand::Shutdown => "Shutdown",
        _ => "This operation",
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect() for cleaner error handling
mod tests {
    use std::sync::Arc;

    use async_lock::RwLock;
    use aura_app::{signal_defs::ERROR_SIGNAL, AppConfig, AppCore};
    use aura_core::effects::reactive::ReactiveEffects;

    use crate::handlers::tui::TuiMode;
    use crate::tui::context::{InitializedAppCore, IoContext};
    use crate::tui::effects::EffectCommand;

    async fn wait_for_error(app_core: &Arc<RwLock<AppCore>>) -> aura_app::AppError {
        tokio::time::timeout(std::time::Duration::from_millis(500), async {
            loop {
                {
                    let core = app_core.read().await;
                    if let Ok(Some(err)) = core.read(&*ERROR_SIGNAL).await {
                        return err;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Timed out waiting for ERROR_SIGNAL to become Some")
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
        assert!(err.to_string().contains("Unknown command"));
    }
}
