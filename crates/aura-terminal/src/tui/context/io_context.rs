//! # iocraft Context
//!
//! Self-contained context for iocraft TUI components.
//!
//! This type intentionally keeps UI state (toasts, local preferences) separate
//! from Aura application state (signals in `AppCore`).
//!
//! ## Builder Pattern
//!
//! Use `IoContext::builder()` for flexible construction:
//!
//! ```rust,ignore
//! let ctx = IoContext::builder()
//!     .with_app_core(app_core)
//!     .with_base_path(PathBuf::from("./data"))
//!     .with_device_id("device-1".to_string())
//!     .with_mode(TuiMode::Production)
//!     .build()?;
//! ```

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_lock::RwLock;
use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL,
    SETTINGS_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_app::AppCore;
use aura_core::effects::reactive::ReactiveEffects;

use crate::error::TerminalError;
use crate::handlers::tui::TuiMode;
use crate::tui::context::{
    AccountFilesHelper, DispatchHelper, InitializedAppCore, SnapshotHelper, ToastHelper,
};
use crate::tui::effects::{EffectCommand, OpResponse, OperationalHandler};
use crate::tui::types::ChannelMode;

use crate::tui::hooks::{
    BlockSnapshot, ChatSnapshot, ContactsSnapshot, DevicesSnapshot, GuardiansSnapshot,
    InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};

// ============================================================================
// Builder
// ============================================================================

/// Error returned when IoContextBuilder cannot build an IoContext.
#[derive(Debug, Clone)]
pub enum ContextBuildError {
    /// Required field was not set
    MissingField(&'static str),
}

impl std::fmt::Display for ContextBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextBuildError::MissingField(field) => {
                write!(f, "IoContextBuilder: missing required field '{}'", field)
            }
        }
    }
}

impl std::error::Error for ContextBuildError {}

/// Builder for constructing IoContext with flexible configuration.
///
/// # Example
///
/// ```rust,ignore
/// let ctx = IoContext::builder()
///     .with_app_core(app_core)
///     .with_base_path(PathBuf::from("./data"))
///     .with_device_id("device-1".to_string())
///     .with_mode(TuiMode::Production)
///     .build()?;
/// ```
#[derive(Default)]
pub struct IoContextBuilder {
    app_core: Option<InitializedAppCore>,
    base_path: Option<PathBuf>,
    device_id: Option<String>,
    mode: Option<TuiMode>,
    has_existing_account: bool,
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,
    #[cfg(feature = "development")]
    demo_bridge: Option<Arc<crate::demo::SimulatedBridge>>,
}

impl IoContextBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initialized AppCore (required).
    pub fn with_app_core(mut self, app_core: InitializedAppCore) -> Self {
        self.app_core = Some(app_core);
        self
    }

    /// Set the base path for account files (required).
    pub fn with_base_path(mut self, path: PathBuf) -> Self {
        self.base_path = Some(path);
        self
    }

    /// Set the device ID string (required).
    pub fn with_device_id(mut self, id: String) -> Self {
        self.device_id = Some(id);
        self
    }

    /// Set the TUI mode (required).
    pub fn with_mode(mut self, mode: TuiMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set whether an existing account is present (default: false).
    pub fn with_existing_account(mut self, exists: bool) -> Self {
        self.has_existing_account = exists;
        self
    }

    /// Set demo hints for development mode.
    #[cfg(feature = "development")]
    pub fn with_demo_hints(mut self, hints: crate::demo::DemoHints) -> Self {
        self.demo_hints = Some(hints);
        self
    }

    /// Set the demo bridge for routing commands to simulated agents.
    #[cfg(feature = "development")]
    pub fn with_demo_bridge(mut self, bridge: Arc<crate::demo::SimulatedBridge>) -> Self {
        self.demo_bridge = Some(bridge);
        self
    }

    /// Build the IoContext, returning an error if required fields are missing.
    pub fn build(self) -> Result<IoContext, ContextBuildError> {
        let app_core = self
            .app_core
            .ok_or(ContextBuildError::MissingField("app_core"))?;
        let base_path = self
            .base_path
            .ok_or(ContextBuildError::MissingField("base_path"))?;
        let device_id = self
            .device_id
            .ok_or(ContextBuildError::MissingField("device_id"))?;
        let mode = self
            .mode
            .ok_or(ContextBuildError::MissingField("mode"))?;

        let operational = Arc::new(OperationalHandler::new(app_core.raw().clone()));
        let snapshots = SnapshotHelper::new(app_core.raw().clone(), device_id.clone());
        let toasts = ToastHelper::new();

        let has_existing_account =
            Arc::new(std::sync::atomic::AtomicBool::new(self.has_existing_account));
        let account_files =
            AccountFilesHelper::new(base_path, device_id, mode, has_existing_account.clone());

        let invited_lan_peers = Arc::new(RwLock::new(HashSet::new()));
        let current_context = Arc::new(RwLock::new(None));
        let channel_modes = Arc::new(RwLock::new(HashMap::new()));

        let dispatch = DispatchHelper::new(
            app_core.raw().clone(),
            operational.clone(),
            snapshots.clone(),
            toasts.clone(),
            account_files.clone(),
            invited_lan_peers.clone(),
            current_context.clone(),
            channel_modes.clone(),
        );

        Ok(IoContext {
            app_core,
            operational,
            dispatch,
            snapshots,
            toasts,
            account_files,
            #[cfg(feature = "development")]
            demo_hints: self.demo_hints,
            #[cfg(feature = "development")]
            demo_bridge: self.demo_bridge,
            invited_lan_peers,
            current_context,
            channel_modes,
        })
    }
}

// ============================================================================
// IoContext
// ============================================================================

/// iocraft-friendly context.
#[derive(Clone)]
pub struct IoContext {
    app_core: InitializedAppCore,
    operational: Arc<OperationalHandler>,

    // Focused helpers
    dispatch: DispatchHelper,
    snapshots: SnapshotHelper,
    toasts: ToastHelper,
    account_files: AccountFilesHelper,

    // UI-only state
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,
    #[cfg(feature = "development")]
    demo_bridge: Option<Arc<crate::demo::SimulatedBridge>>,
    invited_lan_peers: Arc<RwLock<HashSet<String>>>,
    current_context: Arc<RwLock<Option<String>>>,
    channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
}

impl IoContext {
    /// Create a new IoContextBuilder for flexible construction.
    ///
    /// This is the preferred way to construct IoContext.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ctx = IoContext::builder()
    ///     .with_app_core(app_core)
    ///     .with_base_path(PathBuf::from("./data"))
    ///     .with_device_id("device-1".to_string())
    ///     .with_mode(TuiMode::Production)
    ///     .with_existing_account(true)
    ///     .build()?;
    /// ```
    pub fn builder() -> IoContextBuilder {
        IoContextBuilder::new()
    }

    /// Create a new IoContext with the given parameters.
    ///
    /// # Deprecated
    ///
    /// Use `IoContext::builder()` instead for more flexible construction.
    #[deprecated(since = "0.1.0", note = "Use IoContext::builder() instead")]
    pub fn new(
        app_core: InitializedAppCore,
        base_path: PathBuf,
        device_id_str: String,
        mode: TuiMode,
    ) -> Self {
        #[allow(deprecated)]
        Self::with_account_status(app_core, true, base_path, device_id_str, mode)
    }

    /// Create a new IoContext with explicit account status.
    ///
    /// # Deprecated
    ///
    /// Use `IoContext::builder()` instead for more flexible construction.
    #[deprecated(since = "0.1.0", note = "Use IoContext::builder() instead")]
    pub fn with_account_status(
        app_core: InitializedAppCore,
        has_existing_account: bool,
        base_path: PathBuf,
        device_id_str: String,
        mode: TuiMode,
    ) -> Self {
        IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(base_path)
            .with_device_id(device_id_str)
            .with_mode(mode)
            .with_existing_account(has_existing_account)
            .build()
            .expect("IoContext::with_account_status: all required fields provided")
    }

    /// Create a new IoContext with demo hints for development mode.
    ///
    /// # Deprecated
    ///
    /// Use `IoContext::builder()` instead for more flexible construction.
    #[cfg(feature = "development")]
    #[deprecated(since = "0.1.0", note = "Use IoContext::builder() instead")]
    pub fn with_demo_hints(
        app_core: InitializedAppCore,
        hints: crate::demo::DemoHints,
        has_existing_account: bool,
        base_path: PathBuf,
        device_id_str: String,
        mode: TuiMode,
    ) -> Self {
        IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(base_path)
            .with_device_id(device_id_str)
            .with_mode(mode)
            .with_existing_account(has_existing_account)
            .with_demo_hints(hints)
            .build()
            .expect("IoContext::with_demo_hints: all required fields provided")
    }

    /// Create an IoContext with default configuration (for testing).
    ///
    /// **Note**: This method cannot be called inside a tokio runtime.
    /// Use `with_defaults_async()` instead.
    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    pub fn with_defaults() -> Self {
        if tokio::runtime::Handle::try_current().is_ok() {
            panic!("IoContext::with_defaults() cannot be called inside a tokio runtime; use IoContext::with_defaults_async().await instead");
        }

        let app_core =
            AppCore::new(aura_app::AppConfig::default()).expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let app_core = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime for IoContext::with_defaults")
            .block_on(InitializedAppCore::new(app_core))
            .expect("Failed to init signals for IoContext::with_defaults");

        IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(PathBuf::from("./aura-data"))
            .with_device_id("default-device".to_string())
            .with_mode(TuiMode::Production)
            .with_existing_account(true)
            .build()
            .expect("IoContext::with_defaults: all required fields provided")
    }

    /// Create an IoContext with default configuration (async version).
    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    pub async fn with_defaults_async() -> Self {
        let app_core =
            AppCore::new(aura_app::AppConfig::default()).expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let app_core = InitializedAppCore::new(app_core)
            .await
            .expect("Failed to init signals for IoContext::with_defaults_async");

        IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(PathBuf::from("./aura-data"))
            .with_device_id("default-device".to_string())
            .with_mode(TuiMode::Production)
            .with_existing_account(true)
            .build()
            .expect("IoContext::with_defaults_async: all required fields provided")
    }

    #[inline]
    pub fn has_app_core(&self) -> bool {
        true
    }

    pub fn app_core(&self) -> &InitializedAppCore {
        &self.app_core
    }

    pub fn app_core_raw(&self) -> &Arc<RwLock<AppCore>> {
        self.app_core.raw()
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

    /// Set the demo bridge for routing commands to simulated agents.
    ///
    /// When set, commands dispatched through this context will also be routed
    /// to the SimulatedBridge, allowing demo agents (Alice/Carol) to respond
    /// to guardian invitations and other interactions.
    #[cfg(feature = "development")]
    pub fn set_demo_bridge(&mut self, bridge: Arc<crate::demo::SimulatedBridge>) {
        self.demo_bridge = Some(bridge);
    }

    /// Get the demo bridge if set.
    #[cfg(feature = "development")]
    pub fn demo_bridge(&self) -> Option<&Arc<crate::demo::SimulatedBridge>> {
        self.demo_bridge.as_ref()
    }

    // =========================================================================
    // Account file operations (isolated, async)
    // =========================================================================

    pub async fn create_account(&self, display_name: &str) -> Result<(), String> {
        let (authority_id, _context_id) = self.account_files.create_account(display_name).await?;

        {
            let mut core = self.app_core_raw().write().await;
            core.set_authority(authority_id);
        }

        Ok(())
    }

    pub async fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::identifiers::ContextId>,
    ) -> Result<(), String> {
        self.account_files
            .restore_recovered_account(recovered_authority_id, recovered_context_id)
            .await
    }

    pub async fn export_account_backup(&self) -> Result<String, String> {
        self.account_files.export_account_backup().await
    }

    pub async fn import_account_backup(&self, backup_code: &str) -> Result<(), String> {
        self.account_files.import_account_backup(backup_code).await
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
        // In demo mode, also route commands through the SimulatedBridge
        // so that simulated agents (Alice/Carol) can respond to them
        #[cfg(feature = "development")]
        if let Some(bridge) = &self.demo_bridge {
            bridge.route_command(&command).await;
        }

        self.dispatch.dispatch(command).await
    }

    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        // In demo mode, also route commands through the SimulatedBridge
        #[cfg(feature = "development")]
        if let Some(bridge) = &self.demo_bridge {
            bridge.route_command(&command).await;
        }

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
        let reactive = match self.app_core.raw().try_read() {
            Some(core) => core.reactive().clone(),
            None => return false,
        };

        if let Ok(status) = reactive.read(&*CONNECTION_STATUS_SIGNAL).await {
            return matches!(status, ConnectionStatus::Online { .. });
        }
        false
    }

    pub async fn last_error(&self) -> Option<String> {
        let reactive = match self.app_core.raw().try_read() {
            Some(core) => core.reactive().clone(),
            None => return None,
        };

        if let Ok(error) = reactive.read(&*ERROR_SIGNAL).await {
            return error.map(|e| e.to_string());
        }
        None
    }

    pub async fn is_syncing(&self) -> bool {
        let reactive = match self.app_core.raw().try_read() {
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
            .raw()
            .try_read()
            .and_then(|core| core.runtime().cloned());

        if let Some(runtime) = runtime {
            return runtime.get_sync_status().await.last_sync_ms;
        }
        None
    }

    pub async fn known_peers_count(&self) -> usize {
        let (runtime, reactive) = match self.app_core.raw().try_read() {
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
        let reactive = match self.app_core.raw().try_read() {
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
            .raw()
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
    // Settings helpers (via SETTINGS_SIGNAL)
    // =========================================================================

    pub async fn get_display_name(&self) -> String {
        let core = self.app_core.raw().read().await;
        core.read(&*SETTINGS_SIGNAL)
            .await
            .unwrap_or_default()
            .display_name
    }

    // Note: set_display_name and set_mfa_policy were removed because they bypassed
    // the proper command dispatch pattern. Use EffectCommand::UpdateNickname and
    // EffectCommand::UpdateMfaPolicy instead, which go through the proper workflow
    // and persist to storage.

    pub async fn get_mfa_policy(&self) -> crate::tui::types::MfaPolicy {
        use crate::tui::types::MfaPolicy;

        let core = self.app_core.raw().read().await;
        let state = core.read(&*SETTINGS_SIGNAL).await.unwrap_or_default();
        match state.mfa_policy.as_str() {
            "Disabled" => MfaPolicy::Disabled,
            "SensitiveOnly" | "" => MfaPolicy::SensitiveOnly,
            "AlwaysRequired" => MfaPolicy::AlwaysRequired,
            _ => MfaPolicy::SensitiveOnly,
        }
    }

    // =========================================================================
    // Context / channel modes (UI-only)
    // =========================================================================

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
