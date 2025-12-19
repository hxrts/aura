//! # Command Dispatch Helpers
//!
//! Focused helpers for:
//! - Account file operations (create/restore/backup import/export)
//! - Command dispatch (Intent via AppCore, Operational via OperationalHandler)
//! - Emitting `ERROR_SIGNAL` on all error paths

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_lock::RwLock;
use aura_app::signal_defs::CONTACTS_SIGNAL;
use aura_app::views::contacts::Contact as ViewContact;
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::AuthorityId;

use super::{SnapshotHelper, ToastHelper};
use crate::error::TerminalError;
use crate::handlers::tui::TuiMode;
use crate::tui::effects::{command_to_intent, EffectCommand, OpResponse, OperationalHandler};
use crate::tui::types::ChannelMode;

/// File-based account operations used by the TUI.
///
/// Note: These are currently implemented via the TUI handler helpers (disk I/O).
/// We isolate them here so we can later swap to effect-based storage without
/// touching UI dispatch sites.
#[derive(Clone)]
pub struct AccountFilesHelper {
    base_path: PathBuf,
    device_id_str: String,
    mode: TuiMode,
    has_existing_account: Arc<AtomicBool>,
}

impl AccountFilesHelper {
    pub fn new(
        base_path: PathBuf,
        device_id_str: String,
        mode: TuiMode,
        has_existing_account: Arc<AtomicBool>,
    ) -> Self {
        Self {
            base_path,
            device_id_str,
            mode,
            has_existing_account,
        }
    }

    pub fn has_account(&self) -> bool {
        self.has_existing_account.load(Ordering::Relaxed)
    }

    pub fn set_account_created(&self) {
        self.has_existing_account.store(true, Ordering::Relaxed);
    }

    pub fn create_account(&self, display_name: &str) -> Result<(), String> {
        match crate::handlers::tui::create_account(
            &self.base_path,
            &self.device_id_str,
            self.mode,
            display_name,
        ) {
            Ok((_authority_id, _context_id)) => {
                self.set_account_created();
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to create account: {}", e);
                Err(format!("Failed to create account: {}", e))
            }
        }
    }

    pub fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::identifiers::ContextId>,
    ) -> Result<(), String> {
        match crate::handlers::tui::restore_recovered_account(
            &self.base_path,
            recovered_authority_id,
            recovered_context_id,
            self.mode,
        ) {
            Ok((_authority_id, _context_id)) => {
                self.set_account_created();
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to restore recovered account: {}", e);
                Err(format!("Failed to restore recovered account: {}", e))
            }
        }
    }

    pub fn export_account_backup(&self) -> Result<String, String> {
        if !self.has_account() {
            return Err("No account exists to backup".to_string());
        }

        crate::handlers::tui::export_account_backup(
            &self.base_path,
            Some(&self.device_id_str),
            self.mode,
        )
        .map_err(|e| format!("Failed to export backup: {}", e))
    }

    pub fn import_account_backup(&self, backup_code: &str) -> Result<(), String> {
        match crate::handlers::tui::import_account_backup(
            &self.base_path,
            backup_code,
            true,
            self.mode,
        ) {
            Ok((_authority_id, _context_id)) => {
                self.set_account_created();
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to import backup: {}", e);
                Err(format!("Failed to import backup: {}", e))
            }
        }
    }
}

/// Helper for dispatching commands through AppCore (intents) and OperationalHandler.
#[derive(Clone)]
pub struct DispatchHelper {
    app_core: Arc<RwLock<AppCore>>,
    operational: Arc<OperationalHandler>,
    snapshots: SnapshotHelper,
    toasts: ToastHelper,
    account_files: AccountFilesHelper,

    // Local, UI-only state updates driven by OpResponse.
    current_context: Arc<RwLock<Option<String>>>,
    channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
    display_name: Arc<RwLock<String>>,
    mfa_policy: Arc<RwLock<crate::tui::types::MfaPolicy>>,
    invited_lan_peers: Arc<RwLock<HashSet<String>>>,
}

impl DispatchHelper {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app_core: Arc<RwLock<AppCore>>,
        operational: Arc<OperationalHandler>,
        snapshots: SnapshotHelper,
        toasts: ToastHelper,
        account_files: AccountFilesHelper,
        invited_lan_peers: Arc<RwLock<HashSet<String>>>,
        display_name: Arc<RwLock<String>>,
        mfa_policy: Arc<RwLock<crate::tui::types::MfaPolicy>>,
        current_context: Arc<RwLock<Option<String>>>,
        channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
    ) -> Self {
        Self {
            app_core,
            operational,
            snapshots,
            toasts,
            account_files,
            current_context,
            channel_modes,
            display_name,
            mfa_policy,
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
                let account_files = self.account_files.clone();
                let result =
                    tokio::task::spawn_blocking(move || account_files.export_account_backup())
                        .await
                        .map_err(|e| format!("Backup export task failed: {}", e));

                match result {
                    Ok(Ok(code)) => {
                        self.toasts
                            .success("account-backup", format!("Backup code: {}", code))
                            .await;
                        return Ok(());
                    }
                    Ok(Err(msg)) | Err(msg) => {
                        self.operational
                            .emit_error(TerminalError::Operation(msg.clone()))
                            .await;
                        return Err(msg);
                    }
                }
            }
            EffectCommand::ImportAccountBackup { backup_code } => {
                let account_files = self.account_files.clone();
                let backup_code = backup_code.clone();
                let result = tokio::task::spawn_blocking(move || {
                    account_files.import_account_backup(&backup_code)
                })
                .await
                .map_err(|e| format!("Backup import task failed: {}", e));

                match result {
                    Ok(Ok(())) => {
                        self.toasts
                            .success("account-backup", "Backup imported successfully")
                            .await;
                        return Ok(());
                    }
                    Ok(Err(msg)) | Err(msg) => {
                        self.operational
                            .emit_error(TerminalError::Operation(msg.clone()))
                            .await;
                        return Err(msg);
                    }
                }
            }
            _ => {}
        }

        // Build command context from current state for proper ID resolution.
        let cmd_ctx = self.snapshots.command_context();

        // Intent path (journaled).
        if let Some(intent) = command_to_intent(&command, &cmd_ctx) {
            let mut core = self.app_core.write().await;
            match core.dispatch(intent) {
                Ok(_fact_id) => {
                    if let Err(e) = core.commit_pending_facts_and_emit().await {
                        tracing::warn!("Failed to commit facts or emit signals: {}", e);
                    }
                    Ok(())
                }
                Err(e) => {
                    let msg = format!("Intent dispatch failed: {}", e);
                    self.operational
                        .emit_error(TerminalError::Operation(msg.clone()))
                        .await;
                    Err(msg)
                }
            }
        } else if let Some(result) = self.operational.execute_with_errors(&command).await {
            // Operational path.
            match result {
                Ok(response) => self.handle_op_response(response).await,
                Err(e) => Err(e.to_string()),
            }
        } else {
            // Unknown command (neither intent nor operational).
            tracing::warn!(
                "Unknown command not handled by Intent or Operational: {:?}",
                command
            );
            let msg = format!("Unknown command: {:?}", command);
            self.operational
                .emit_error(TerminalError::Operation(msg.clone()))
                .await;
            Err(msg)
        }
    }

    /// Legacy compatibility wrapper.
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
            OpResponse::NicknameUpdated { name } => {
                *self.display_name.write().await = name;
                Ok(())
            }
            OpResponse::MfaPolicyUpdated { require_mfa } => {
                use crate::tui::types::MfaPolicy;
                *self.mfa_policy.write().await = if require_mfa {
                    MfaPolicy::SensitiveOnly
                } else {
                    MfaPolicy::Disabled
                };
                Ok(())
            }
            OpResponse::InvitationImported {
                sender_id,
                invitation_type: _,
                message,
                ..
            } => {
                self.add_contact_from_invitation(&sender_id, message.as_deref())
                    .await;
                Ok(())
            }
            OpResponse::InvitationCode { id: _, code } => {
                self.toasts
                    .success("invitation-code", format!("Invitation code: {}", code))
                    .await;
                Ok(())
            }
            OpResponse::Ok | OpResponse::Data(_) | OpResponse::List(_) => Ok(()),
        }
    }

    /// Best-effort authorization gate for Admin-level commands.
    ///
    /// This does not replace Biscuit enforcement (guard chain); it provides
    /// immediate UX feedback and avoids attempting admin ops outside a block.
    fn check_authorization(&self, command: &EffectCommand) -> Result<(), String> {
        use crate::tui::effects::CommandAuthorizationLevel;
        use aura_app::views::block::ResidentRole;

        let level = command.authorization_level();
        match level {
            CommandAuthorizationLevel::Public
            | CommandAuthorizationLevel::Basic
            | CommandAuthorizationLevel::Sensitive => Ok(()),
            CommandAuthorizationLevel::Admin => {
                let snapshot = self.snapshots.try_state_snapshot();
                let role = snapshot.map(|s| s.blocks.current_block().unwrap_or(&s.block).my_role);
                match role {
                    Some(ResidentRole::Admin | ResidentRole::Owner) => Ok(()),
                    Some(ResidentRole::Resident) => Err(format!(
                        "Permission denied: {} requires administrator privileges",
                        command_name(command)
                    )),
                    None => Err(format!(
                        "Permission denied: {} requires a block context",
                        command_name(command)
                    )),
                }
            }
        }
    }

    async fn add_contact_from_invitation(&self, sender_id: &str, message: Option<&str>) {
        // Parse sender_id to AuthorityId
        let authority_id = match sender_id.parse::<AuthorityId>() {
            Ok(id) => id,
            Err(_) => {
                tracing::warn!("Failed to parse sender_id as AuthorityId: {}", sender_id);
                return;
            }
        };

        let suggested_name = message.and_then(|msg| {
            if msg.contains("from ") {
                msg.split("from ")
                    .nth(1)
                    .and_then(|s| s.split(' ').next())
                    .map(|s| s.to_string())
            } else {
                None
            }
        });

        let contact = ViewContact {
            id: authority_id.clone(),
            nickname: suggested_name.clone().unwrap_or_default(),
            suggested_name,
            is_guardian: false,
            is_resident: false,
            last_interaction: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            ),
            is_online: true,
        };

        let core = self.app_core.read().await;
        if let Ok(mut contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
            if !contacts_state.contacts.iter().any(|c| c.id == authority_id) {
                contacts_state.contacts.push(contact);
                if let Err(e) = core.emit(&*CONTACTS_SIGNAL, contacts_state).await {
                    tracing::warn!("Failed to update contacts signal: {}", e);
                } else {
                    tracing::info!("Added contact from invitation: {}", sender_id);
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
mod tests {
    use std::sync::Arc;

    use async_lock::RwLock;
    use aura_app::{signal_defs::ERROR_SIGNAL, AppConfig, AppCore};
    use aura_core::effects::reactive::ReactiveEffects;

    use crate::handlers::tui::TuiMode;
    use crate::tui::context::IoContext;
    use crate::tui::effects::EffectCommand;

    async fn wait_for_error(app_core: &Arc<RwLock<AppCore>>) -> aura_app::signal_defs::AppError {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            {
                let core = app_core.read().await;
                if let Ok(Some(err)) = core.read(&*ERROR_SIGNAL).await {
                    return err;
                }
            }

            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for ERROR_SIGNAL to become Some");
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn unknown_command_emits_error_signal() {
        let app_core = AppCore::new(AppConfig::default()).expect("Failed to create test AppCore");
        app_core
            .init_signals()
            .await
            .expect("Failed to init signals");
        let app_core = Arc::new(RwLock::new(app_core));

        let dir = tempfile::tempdir().expect("Failed to create temp dir");
        let ctx = IoContext::with_account_status(
            app_core.clone(),
            false,
            dir.path().to_path_buf(),
            "test-device".to_string(),
            TuiMode::Production,
        );

        let _ = ctx.dispatch(EffectCommand::UnknownCommandForTest).await;
        let err = wait_for_error(&app_core).await;
        assert_eq!(err.code, "OPERATION_FAILED");
        assert!(err.message.contains("Unknown command"));
    }
}
