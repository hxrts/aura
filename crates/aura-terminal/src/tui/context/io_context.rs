//! # iocraft Context
//!
//! Self-contained context for iocraft TUI components.
//!
//! This type intentionally keeps UI state (toasts, local preferences) separate
//! from Aura application state (signals in `AppCore`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_lock::RwLock;
use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL,
    SYNC_STATUS_SIGNAL,
};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;

use crate::error::TerminalError;
use crate::tui::context::{AccountFilesHelper, DispatchHelper, SnapshotHelper, ToastHelper};
use crate::tui::effects::{EffectCommand, OpResponse, OperationalHandler};
use crate::tui::types::ChannelMode;

use crate::tui::hooks::{
    BlockSnapshot, ChatSnapshot, ContactsSnapshot, DevicesSnapshot, GuardiansSnapshot,
    InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};

/// iocraft-friendly context.
#[derive(Clone)]
pub struct IoContext {
    app_core: Arc<RwLock<AppCore>>,
    operational: Arc<OperationalHandler>,

    // Focused helpers
    dispatch: DispatchHelper,
    snapshots: SnapshotHelper,
    toasts: ToastHelper,
    account_files: AccountFilesHelper,

    // UI-only state
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,
    invited_lan_peers: Arc<RwLock<HashSet<String>>>,
    display_name: Arc<RwLock<String>>,
    mfa_policy: Arc<RwLock<crate::tui::types::MfaPolicy>>,
    current_context: Arc<RwLock<Option<String>>>,
    channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
}

impl IoContext {
    pub fn new(
        app_core: Arc<RwLock<AppCore>>,
        base_path: std::path::PathBuf,
        device_id_str: String,
        mode: crate::handlers::tui::TuiMode,
    ) -> Self {
        Self::with_account_status(app_core, true, base_path, device_id_str, mode)
    }

    pub fn with_account_status(
        app_core: Arc<RwLock<AppCore>>,
        has_existing_account: bool,
        base_path: std::path::PathBuf,
        device_id_str: String,
        mode: crate::handlers::tui::TuiMode,
    ) -> Self {
        let operational = Arc::new(OperationalHandler::new(app_core.clone()));
        let snapshots = SnapshotHelper::new(app_core.clone(), device_id_str.clone());
        let toasts = ToastHelper::new();

        let has_existing_account =
            Arc::new(std::sync::atomic::AtomicBool::new(has_existing_account));
        let account_files =
            AccountFilesHelper::new(base_path, device_id_str, mode, has_existing_account.clone());

        let invited_lan_peers = Arc::new(RwLock::new(HashSet::new()));
        let display_name = Arc::new(RwLock::new(String::new()));
        let mfa_policy = Arc::new(RwLock::new(crate::tui::types::MfaPolicy::default()));
        let current_context = Arc::new(RwLock::new(None));
        let channel_modes = Arc::new(RwLock::new(HashMap::new()));

        let dispatch = DispatchHelper::new(
            app_core.clone(),
            operational.clone(),
            snapshots.clone(),
            toasts.clone(),
            account_files.clone(),
            invited_lan_peers.clone(),
            display_name.clone(),
            mfa_policy.clone(),
            current_context.clone(),
            channel_modes.clone(),
        );

        Self {
            app_core,
            operational,
            dispatch,
            snapshots,
            toasts,
            account_files,
            #[cfg(feature = "development")]
            demo_hints: None,
            invited_lan_peers,
            display_name,
            mfa_policy,
            current_context,
            channel_modes,
        }
    }

    #[cfg(feature = "development")]
    pub fn with_demo_hints(
        app_core: Arc<RwLock<AppCore>>,
        hints: crate::demo::DemoHints,
        has_existing_account: bool,
        base_path: std::path::PathBuf,
        device_id_str: String,
        mode: crate::handlers::tui::TuiMode,
    ) -> Self {
        let mut ctx = Self::with_account_status(
            app_core,
            has_existing_account,
            base_path,
            device_id_str,
            mode,
        );
        ctx.demo_hints = Some(hints);
        ctx
    }

    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    pub fn with_defaults() -> Self {
        let app_core =
            AppCore::new(aura_app::AppConfig::default()).expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        Self::with_account_status(
            app_core,
            true,
            std::path::PathBuf::from("./aura-data"),
            "default-device".to_string(),
            crate::handlers::tui::TuiMode::Production,
        )
    }

    #[inline]
    pub fn has_app_core(&self) -> bool {
        true
    }

    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    pub fn has_account(&self) -> bool {
        self.account_files.has_account()
    }

    pub fn set_account_created(&self) {
        self.account_files.set_account_created();
    }

    // =========================================================================
    // Demo helpers
    // =========================================================================

    #[cfg(feature = "development")]
    pub fn demo_hints(&self) -> Option<&crate::demo::DemoHints> {
        self.demo_hints.as_ref()
    }

    #[cfg(feature = "development")]
    pub fn is_demo_mode(&self) -> bool {
        self.demo_hints.is_some()
    }

    #[cfg(not(feature = "development"))]
    pub fn is_demo_mode(&self) -> bool {
        false
    }

    #[cfg(feature = "development")]
    pub fn demo_alice_code(&self) -> String {
        self.demo_hints
            .as_ref()
            .map(|h| h.alice_invite_code.clone())
            .unwrap_or_default()
    }

    #[cfg(feature = "development")]
    pub fn demo_carol_code(&self) -> String {
        self.demo_hints
            .as_ref()
            .map(|h| h.carol_invite_code.clone())
            .unwrap_or_default()
    }

    #[cfg(not(feature = "development"))]
    pub fn demo_alice_code(&self) -> String {
        String::new()
    }

    #[cfg(not(feature = "development"))]
    pub fn demo_carol_code(&self) -> String {
        String::new()
    }

    // =========================================================================
    // Account file operations (isolated, async)
    // =========================================================================

    pub fn create_account(&self, display_name: &str) -> Result<(), String> {
        self.account_files.create_account(display_name)
    }

    pub fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::identifiers::ContextId>,
    ) -> Result<(), String> {
        self.account_files
            .restore_recovered_account(recovered_authority_id, recovered_context_id)
    }

    pub fn export_account_backup(&self) -> Result<String, String> {
        self.account_files.export_account_backup()
    }

    pub fn import_account_backup(&self, backup_code: &str) -> Result<(), String> {
        self.account_files.import_account_backup(backup_code)
    }

    // =========================================================================
    // View snapshots (synchronous, best-effort)
    // =========================================================================

    pub fn snapshot_chat(&self) -> ChatSnapshot {
        self.snapshots.snapshot_chat()
    }

    pub fn snapshot_contacts(&self) -> ContactsSnapshot {
        self.snapshots.snapshot_contacts()
    }

    pub fn snapshot_recovery(&self) -> RecoverySnapshot {
        self.snapshots.snapshot_recovery()
    }

    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        self.snapshots.snapshot_neighborhood()
    }

    pub fn snapshot_block(&self) -> BlockSnapshot {
        self.snapshots.snapshot_block()
    }

    pub fn snapshot_invitations(&self) -> InvitationsSnapshot {
        self.snapshots.snapshot_invitations()
    }

    pub fn snapshot_devices(&self) -> DevicesSnapshot {
        self.snapshots.snapshot_devices()
    }

    pub fn snapshot_guardians(&self) -> GuardiansSnapshot {
        self.snapshots.snapshot_guardians()
    }

    // =========================================================================
    // Command dispatch
    // =========================================================================

    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        self.dispatch.dispatch(command).await
    }

    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        self.dispatch.dispatch_and_wait(command).await
    }

    pub async fn export_invitation_code(&self, invitation_id: &str) -> Result<String, String> {
        match self
            .operational
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: invitation_id.to_string(),
            })
            .await
        {
            Some(Ok(OpResponse::InvitationCode { code, .. })) => Ok(code),
            Some(Ok(other)) => Err(format!("Unexpected response: {:?}", other)),
            Some(Err(err)) => {
                let terr: TerminalError = err.clone().into();
                self.operational.emit_error(terr).await;
                Err(err.to_string())
            }
            None => Err("ExportInvitation not handled".to_string()),
        }
    }

    pub async fn dispatch_send_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<(), String> {
        self.dispatch(EffectCommand::SendMessage {
            channel: channel_id.to_string(),
            content: content.to_string(),
        })
        .await
    }

    pub async fn dispatch_join_channel(&self, channel_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::JoinChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    pub async fn dispatch_leave_channel(&self, channel_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::LeaveChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    pub async fn dispatch_start_recovery(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::StartRecovery).await
    }

    pub async fn dispatch_submit_guardian_approval(&self, guardian_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::SubmitGuardianApproval {
            guardian_id: guardian_id.to_string(),
        })
        .await
    }

    // =========================================================================
    // Connection / sync status (via signals)
    // =========================================================================

    pub async fn is_connected(&self) -> bool {
        let reactive = match self.app_core.try_read() {
            Some(core) => core.reactive().clone(),
            None => return false,
        };

        if let Ok(status) = reactive.read(&*CONNECTION_STATUS_SIGNAL).await {
            return matches!(status, ConnectionStatus::Online { .. });
        }
        false
    }

    pub async fn last_error(&self) -> Option<String> {
        let reactive = match self.app_core.try_read() {
            Some(core) => core.reactive().clone(),
            None => return None,
        };

        if let Ok(error) = reactive.read(&*ERROR_SIGNAL).await {
            return error.map(|e| e.message);
        }
        None
    }

    pub async fn is_syncing(&self) -> bool {
        let reactive = match self.app_core.try_read() {
            Some(core) => core.reactive().clone(),
            None => return false,
        };

        if let Ok(status) = reactive.read(&*SYNC_STATUS_SIGNAL).await {
            return matches!(status, SyncStatus::Syncing { .. });
        }
        false
    }

    pub async fn last_sync_time(&self) -> Option<u64> {
        let runtime = self
            .app_core
            .try_read()
            .and_then(|core| core.runtime().cloned());

        if let Some(runtime) = runtime {
            return runtime.get_sync_status().await.last_sync_ms;
        }
        None
    }

    pub async fn known_peers_count(&self) -> usize {
        let (runtime, reactive) = match self.app_core.try_read() {
            Some(core) => (core.runtime().cloned(), core.reactive().clone()),
            None => return 0,
        };

        if let Some(runtime) = runtime {
            let status = runtime.get_sync_status().await;
            if status.connected_peers > 0 {
                return status.connected_peers;
            }
        }

        if let Ok(ConnectionStatus::Online { peer_count }) =
            reactive.read(&*CONNECTION_STATUS_SIGNAL).await
        {
            return peer_count;
        }
        0
    }

    pub async fn get_discovered_peers(&self) -> Vec<(String, String)> {
        let reactive = match self.app_core.try_read() {
            Some(core) => core.reactive().clone(),
            None => return vec![],
        };

        if let Ok(state) = reactive.read(&*DISCOVERED_PEERS_SIGNAL).await {
            return state
                .peers
                .iter()
                .map(|p| (p.authority_id.clone(), p.address.clone()))
                .collect();
        }
        vec![]
    }

    pub async fn get_lan_peers(&self) -> Vec<(String, String)> {
        let runtime = self
            .app_core
            .try_read()
            .and_then(|core| core.runtime().cloned());

        let Some(runtime) = runtime else {
            return vec![];
        };

        let lan_peers = runtime.get_lan_peers().await;
        lan_peers
            .iter()
            .map(|peer| (peer.authority_id.to_string(), peer.address.clone()))
            .collect()
    }

    // =========================================================================
    // LAN invitation tracking (UI-only)
    // =========================================================================

    pub async fn mark_peer_invited(&self, authority_id: &str) {
        self.invited_lan_peers
            .write()
            .await
            .insert(authority_id.to_string());
    }

    pub async fn is_peer_invited(&self, authority_id: &str) -> bool {
        self.invited_lan_peers.read().await.contains(authority_id)
    }

    pub async fn get_invited_peer_ids(&self) -> HashSet<String> {
        self.invited_lan_peers.read().await.clone()
    }

    // =========================================================================
    // Display name / MFA policy / context / channel modes (UI-only)
    // =========================================================================

    pub async fn get_display_name(&self) -> String {
        self.display_name.read().await.clone()
    }

    pub async fn set_display_name(&self, name: &str) {
        *self.display_name.write().await = name.to_string();
    }

    pub async fn get_mfa_policy(&self) -> crate::tui::types::MfaPolicy {
        *self.mfa_policy.read().await
    }

    pub async fn set_mfa_policy(&self, policy: crate::tui::types::MfaPolicy) {
        *self.mfa_policy.write().await = policy;
    }

    pub async fn get_current_context(&self) -> Option<String> {
        self.current_context.read().await.clone()
    }

    pub async fn set_current_context(&self, context_id: Option<String>) {
        *self.current_context.write().await = context_id;
    }

    pub async fn get_channel_mode(&self, channel_id: &str) -> ChannelMode {
        self.channel_modes
            .read()
            .await
            .get(channel_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn set_channel_mode(&self, channel_id: &str, flags: &str) {
        let mut modes = self.channel_modes.write().await;
        let mode = modes.entry(channel_id.to_string()).or_default();
        mode.parse_flags(flags);
    }

    // =========================================================================
    // Toast notifications
    // =========================================================================

    pub async fn add_toast(&self, toast: crate::tui::components::ToastMessage) {
        self.toasts.add(toast).await;
    }

    pub async fn add_error_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        self.toasts.error(id, message).await;
    }

    pub async fn add_success_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        self.toasts.success(id, message).await;
    }

    pub async fn add_info_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        self.toasts.info(id, message).await;
    }

    pub async fn get_toasts(&self) -> Vec<crate::tui::components::ToastMessage> {
        self.toasts.get_all().await
    }

    pub async fn clear_toast(&self, id: &str) {
        self.toasts.clear(id).await;
    }

    pub async fn clear_toasts(&self) {
        self.toasts.clear_all().await;
    }

    // =========================================================================
    // Capability checking (best-effort, snapshot-based)
    // =========================================================================

    pub fn get_current_role(&self) -> Option<aura_app::views::block::ResidentRole> {
        // Prefer multi-block state if available; fall back to legacy singular.
        let snapshot = self.snapshots.try_state_snapshot()?;
        let block = snapshot.blocks.current_block().unwrap_or(&snapshot.block);
        Some(block.my_role)
    }

    pub fn has_capability(&self, capability: &crate::tui::commands::CommandCapability) -> bool {
        use crate::tui::commands::CommandCapability;
        use aura_app::views::block::ResidentRole;

        if matches!(capability, CommandCapability::None) {
            return true;
        }

        let role = match self.get_current_role() {
            Some(r) => r,
            None => {
                return matches!(
                    capability,
                    CommandCapability::SendDm | CommandCapability::UpdateContact
                );
            }
        };

        match capability {
            CommandCapability::None => true,
            CommandCapability::SendDm
            | CommandCapability::SendMessage
            | CommandCapability::UpdateContact
            | CommandCapability::ViewMembers
            | CommandCapability::JoinChannel
            | CommandCapability::LeaveContext => true,
            CommandCapability::ModerateKick
            | CommandCapability::ModerateBan
            | CommandCapability::ModerateMute
            | CommandCapability::Invite
            | CommandCapability::ManageChannel
            | CommandCapability::PinContent
            | CommandCapability::GrantSteward => {
                matches!(role, ResidentRole::Admin | ResidentRole::Owner)
            }
        }
    }

    /// Check if the current user can execute a command based on its authorization level.
    ///
    /// This is a best-effort, UX-focused pre-check. Biscuit/guard-chain enforcement
    /// remains the source of truth.
    pub fn check_authorization(&self, command: &EffectCommand) -> Result<(), String> {
        use crate::tui::effects::CommandAuthorizationLevel;
        use aura_app::views::block::ResidentRole;

        let level = command.authorization_level();
        match level {
            CommandAuthorizationLevel::Public
            | CommandAuthorizationLevel::Basic
            | CommandAuthorizationLevel::Sensitive => Ok(()),
            CommandAuthorizationLevel::Admin => {
                let role = self.get_current_role();
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

impl Default for IoContext {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Trait for iocraft props that need context access.
pub trait HasContext {
    fn set_context(&mut self, ctx: Arc<IoContext>);
    fn context(&self) -> Option<&Arc<IoContext>>;
}
