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
use cfg_if::cfg_if;
cfg_if! {
    if #[cfg(feature = "development")] {
        use aura_agent::AuraAgent;
    }
}
use aura_agent::handlers::{InvitationType as AgentInvitationType, ShareableInvitation};
use aura_app::ui::prelude::*;
use aura_app::ui::signals::{
    ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL,
    SETTINGS_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_app::ui::types::{BootstrapRuntimeIdentity, InvitationBridgeType};
use aura_app::ui::workflows::invitation::import_invitation_details;
use aura_app::ui::workflows::{
    context as context_workflows, settings as settings_workflows, system as system_workflows,
};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::Epoch;
use aura_core::AuthorityId;

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::tui::{resolve_storage_path, TuiMode};
use crate::tui::context::{
    AccountFilesHelper, DispatchHelper, InitializedAppCore, SnapshotHelper, ToastHelper,
};
use crate::tui::effects::{EffectCommand, OpFailureCode, OpResponse, OperationalHandler};
use crate::tui::tasks::UiTaskRegistry;
use crate::tui::types::ChannelMode;

use crate::tui::hooks::{
    ChatSnapshot, ContactsSnapshot, DevicesSnapshot, GuardiansSnapshot, HomeSnapshot,
    InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};

#[derive(Clone, Debug)]
pub struct AuthoritySwitchRequest {
    pub authority_id: aura_core::types::identifiers::AuthorityId,
    pub nickname_suggestion: Option<String>,
}

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
                write!(f, "IoContextBuilder: missing required field '{field}'")
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
    pending_runtime_bootstrap: bool,
    #[cfg_attr(feature = "development", doc = "Demo configuration fields")]
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,
    #[cfg(feature = "development")]
    demo_bridge: Option<Arc<crate::demo::SimulatedBridge>>,
    #[cfg(feature = "development")]
    demo_mobile_agent: Option<Arc<AuraAgent>>,
    #[cfg(feature = "development")]
    demo_mobile_device_id: Option<String>,
    #[cfg(feature = "development")]
    demo_mobile_authority_id: Option<String>,
}

impl IoContextBuilder {
    /// Create a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initialized AppCore (required).
    #[must_use]
    pub fn with_app_core(mut self, app_core: InitializedAppCore) -> Self {
        self.app_core = Some(app_core);
        self
    }

    /// Set the base path for account files (required).
    #[must_use]
    pub fn with_base_path(mut self, path: PathBuf) -> Self {
        self.base_path = Some(path);
        self
    }

    /// Set the device ID string (required).
    #[must_use]
    pub fn with_device_id(mut self, id: String) -> Self {
        self.device_id = Some(id);
        self
    }

    /// Set the TUI mode (required).
    #[must_use]
    pub fn with_mode(mut self, mode: TuiMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set whether an existing account is present (default: false).
    #[must_use]
    pub fn with_existing_account(mut self, exists: bool) -> Self {
        self.has_existing_account = exists;
        self
    }

    #[must_use]
    pub fn with_pending_runtime_bootstrap(mut self, pending: bool) -> Self {
        self.pending_runtime_bootstrap = pending;
        self
    }

    cfg_if! {
        if #[cfg(feature = "development")] {
            /// Set demo hints for development mode.
            pub fn with_demo_hints(mut self, hints: crate::demo::DemoHints) -> Self {
                self.demo_hints = Some(hints);
                self
            }

            /// Set the demo bridge for routing commands to simulated agents.
            pub fn with_demo_bridge(mut self, bridge: Arc<crate::demo::SimulatedBridge>) -> Self {
                self.demo_bridge = Some(bridge);
                self
            }

            /// Set the demo Mobile agent for device enrollment flows.
            pub fn with_demo_mobile_agent(mut self, agent: Arc<AuraAgent>) -> Self {
                self.demo_mobile_agent = Some(agent);
                self
            }

            /// Set the demo Mobile device id for MFA shortcuts.
            pub fn with_demo_mobile_device_id(mut self, device_id: String) -> Self {
                self.demo_mobile_device_id = Some(device_id);
                self
            }

            pub fn with_demo_mobile_authority_id(mut self, authority_id: String) -> Self {
                self.demo_mobile_authority_id = Some(authority_id);
                self
            }
        }
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
        // Mode is required in the builder but no longer used here - mode isolation
        // is achieved via mode-specific base_path directories. Keep the check to
        // maintain the API contract.
        let _mode = self.mode.ok_or(ContextBuildError::MissingField("mode"))?;

        let tasks = Arc::new(UiTaskRegistry::new());
        let operational = Arc::new(OperationalHandler::new(
            app_core.raw().clone(),
            tasks.clone(),
        ));
        let snapshots = SnapshotHelper::new(app_core.raw().clone(), device_id.clone());
        let toasts = ToastHelper::new();

        let has_existing_account = Arc::new(std::sync::atomic::AtomicBool::new(
            self.has_existing_account,
        ));
        let account_files = AccountFilesHelper::new(base_path, device_id, has_existing_account);

        let invited_lan_peers = Arc::new(RwLock::new(HashSet::new()));
        let current_context = Arc::new(RwLock::new(None));
        let channel_modes = Arc::new(RwLock::new(HashMap::new()));
        let requested_authority_switch = Arc::new(std::sync::Mutex::new(None));

        let dispatch = DispatchHelper::new(
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
            #[cfg(feature = "development")]
            demo_mobile_agent: self.demo_mobile_agent,
            #[cfg(feature = "development")]
            demo_mobile_device_id: self.demo_mobile_device_id,
            #[cfg(feature = "development")]
            demo_mobile_authority_id: self.demo_mobile_authority_id,
            invited_lan_peers,
            current_context,
            channel_modes,
            tasks,
            pending_runtime_bootstrap: self.pending_runtime_bootstrap,
            requested_authority_switch,
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
    #[cfg(feature = "development")]
    demo_mobile_agent: Option<Arc<AuraAgent>>,
    #[cfg(feature = "development")]
    demo_mobile_device_id: Option<String>,
    #[cfg(feature = "development")]
    demo_mobile_authority_id: Option<String>,
    invited_lan_peers: Arc<RwLock<HashSet<AuthorityId>>>,
    current_context: Arc<RwLock<Option<String>>>,
    channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,
    tasks: Arc<UiTaskRegistry>,
    pending_runtime_bootstrap: bool,
    requested_authority_switch: Arc<std::sync::Mutex<Option<AuthoritySwitchRequest>>>,
}

/// Lightweight TUI-facing result of starting device enrollment.
#[derive(Debug, Clone)]
pub struct DeviceEnrollmentStartInfo {
    pub ceremony_id: String,
    pub enrollment_code: String,
    pub pending_epoch: Epoch,
    pub device_id: String,
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
    #[must_use]
    pub fn builder() -> IoContextBuilder {
        IoContextBuilder::new()
    }

    #[must_use]
    pub fn tasks(&self) -> Arc<UiTaskRegistry> {
        self.tasks.clone()
    }

    pub fn authority_switch_request_handle(
        &self,
    ) -> Arc<std::sync::Mutex<Option<AuthoritySwitchRequest>>> {
        self.requested_authority_switch.clone()
    }

    pub fn request_authority_switch(
        &self,
        authority_id: aura_core::types::identifiers::AuthorityId,
        nickname_suggestion: Option<String>,
    ) {
        if let Ok(mut guard) = self.requested_authority_switch.lock() {
            *guard = Some(AuthoritySwitchRequest {
                authority_id,
                nickname_suggestion,
            });
        }
    }

    /// Create a new IoContext with explicit account status.
    ///
    /// # Deprecated
    ///
    /// Use `IoContext::builder()` instead for more flexible construction.
    #[doc(hidden)]
    #[deprecated(since = "0.1.0", note = "Use IoContext::builder() instead")]
    #[must_use]
    pub fn with_account_status(
        app_core: InitializedAppCore,
        has_existing_account: bool,
        base_path: PathBuf,
        device_id_str: String,
        mode: TuiMode,
    ) -> Self {
        match IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(base_path)
            .with_device_id(device_id_str)
            .with_mode(mode)
            .with_existing_account(has_existing_account)
            .build()
        {
            Ok(ctx) => ctx,
            Err(err) => panic!("IoContext::with_account_status: {err}"),
        }
    }

    cfg_if! {
        if #[cfg(feature = "development")] {
            /// Create a new IoContext with demo hints for development mode.
            ///
            /// # Deprecated
            ///
            /// Use `IoContext::builder()` instead for more flexible construction.
            #[doc(hidden)]
            #[deprecated(since = "0.1.0", note = "Use IoContext::builder() instead")]
            pub fn with_demo_hints(
                app_core: InitializedAppCore,
                hints: crate::demo::DemoHints,
                has_existing_account: bool,
                base_path: PathBuf,
                device_id_str: String,
                mode: TuiMode,
            ) -> Self {
                match IoContext::builder()
                    .with_app_core(app_core)
                    .with_base_path(base_path)
                    .with_device_id(device_id_str)
                    .with_mode(mode)
                    .with_existing_account(has_existing_account)
                    .with_demo_hints(hints)
                    .build()
                {
                    Ok(ctx) => ctx,
                    Err(err) => panic!("IoContext::with_demo_hints: {err}"),
                }
            }
        }
    }

    /// Create an IoContext with default configuration (for testing).
    ///
    /// **Note**: This method cannot be called inside a tokio runtime.
    /// Use `with_defaults_async()` instead.
    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    #[must_use]
    pub fn with_defaults() -> Self {
        if tokio::runtime::Handle::try_current().is_ok() {
            panic!("IoContext::with_defaults() cannot be called inside a tokio runtime; use IoContext::with_defaults_async().await instead");
        }

        let app_core = AppCore::new(aura_app::ui::types::AppConfig::default())
            .expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let app_core = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime for IoContext::with_defaults")
            .block_on(InitializedAppCore::new(app_core))
            .expect("Failed to init signals for IoContext::with_defaults");

        let mode = TuiMode::Production;
        IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(resolve_storage_path(None, mode))
            .with_device_id("default-device".to_string())
            .with_mode(mode)
            .with_existing_account(true)
            .build()
            .expect("IoContext::with_defaults: all required fields provided")
    }

    /// Create an IoContext with default configuration (async version).
    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    pub async fn with_defaults_async() -> Self {
        let app_core = AppCore::new(aura_app::ui::types::AppConfig::default())
            .expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let app_core = InitializedAppCore::new(app_core)
            .await
            .expect("Failed to init signals for IoContext::with_defaults_async");

        let mode = TuiMode::Production;
        IoContext::builder()
            .with_app_core(app_core)
            .with_base_path(resolve_storage_path(None, mode))
            .with_device_id("default-device".to_string())
            .with_mode(mode)
            .with_existing_account(true)
            .build()
            .expect("IoContext::with_defaults_async: all required fields provided")
    }

    #[inline]
    #[must_use]
    pub fn has_app_core(&self) -> bool {
        true
    }

    #[must_use]
    pub fn app_core(&self) -> &InitializedAppCore {
        &self.app_core
    }

    #[must_use]
    pub fn app_core_raw(&self) -> &Arc<RwLock<AppCore>> {
        self.app_core.raw()
    }

    #[must_use]
    pub fn has_account(&self) -> bool {
        self.account_files.has_account()
    }

    pub fn set_account_created(&self) {
        self.account_files.set_account_created();
    }

    // =========================================================================
    // Demo helpers
    // =========================================================================

    cfg_if! {
        if #[cfg(feature = "development")] {
            pub fn demo_hints(&self) -> Option<&crate::demo::DemoHints> {
                self.demo_hints.as_ref()
            }

            pub fn is_demo_mode(&self) -> bool {
                self.demo_hints.is_some()
            }

            pub fn demo_alice_code(&self) -> String {
                self.demo_hints
                    .as_ref()
                    .map(|h| h.alice_invite_code.clone())
                    .unwrap_or_default()
            }

            pub fn demo_carol_code(&self) -> String {
                self.demo_hints
                    .as_ref()
                    .map(|h| h.carol_invite_code.clone())
                    .unwrap_or_default()
            }

            pub fn demo_mobile_device_id(&self) -> String {
                self.demo_mobile_device_id.clone().unwrap_or_default()
            }

            pub fn demo_mobile_authority_id(&self) -> String {
                self.demo_mobile_authority_id.clone().unwrap_or_default()
            }

            /// Set the demo bridge for routing commands to simulated agents.
            ///
            /// When set, commands dispatched through this context will also be routed
            /// to the SimulatedBridge, allowing demo agents (Alice/Carol) to respond
            /// to guardian invitations and other interactions.
            pub fn set_demo_bridge(&mut self, bridge: Arc<crate::demo::SimulatedBridge>) {
                self.demo_bridge = Some(bridge);
            }

            /// Get the demo bridge if set.
            pub fn demo_bridge(&self) -> Option<&Arc<crate::demo::SimulatedBridge>> {
                self.demo_bridge.as_ref()
            }

            /// Import an invite code on the demo Mobile device and accept it.
            pub async fn import_invitation_on_mobile(&self, code: &str) -> TerminalResult<()> {
                let agent = self
                    .demo_mobile_agent
                    .as_ref()
                    .ok_or_else(|| TerminalError::NotFound("Demo Mobile agent unavailable".to_string()))?;
                let invitations = agent
                    .invitations()
                    .map_err(|e| {
                        TerminalError::structured_operation(
                            OpFailureCode::ImportInvitation.as_str(),
                            format!("Invitation service unavailable: {e}"),
                        )
                    })?;
                let invitation = invitations
                    .import_and_cache(code)
                    .await
                    .map_err(|e| {
                        TerminalError::structured_operation(
                            OpFailureCode::ImportInvitation.as_str(),
                            format!("Failed to import invitation: {e}"),
                        )
                    })?;
                invitations
                    .accept(&invitation.invitation_id)
                    .await
                    .map_err(|e| {
                        TerminalError::structured_operation(
                            OpFailureCode::AcceptInvitation.as_str(),
                            format!("Failed to accept invitation: {e}"),
                        )
                    })?;
                Ok(())
            }

            /// Drive ceremony processing on the demo Mobile device.
            ///
            /// This is used by demo-mode harness flows to ensure device-threshold
            /// ceremony packages are consumed by the simulated secondary device.
            pub async fn process_demo_mobile_ceremony_acceptances(&self) -> TerminalResult<()> {
                let agent = self
                    .demo_mobile_agent
                    .as_ref()
                    .ok_or_else(|| TerminalError::NotFound("Demo Mobile agent unavailable".to_string()))?;
                agent
                    .process_ceremony_acceptances()
                    .await
                    .map(|_| ())
                    .map_err(|e| TerminalError::Operation(e.to_string()))
            }
        } else {
            #[must_use]
            pub fn is_demo_mode(&self) -> bool {
                false
            }

            #[must_use]
            pub fn demo_alice_code(&self) -> String {
                String::new()
            }

            #[must_use]
            pub fn demo_carol_code(&self) -> String {
                String::new()
            }

            #[must_use]
            pub fn demo_mobile_device_id(&self) -> String {
                String::new()
            }

            pub fn demo_mobile_authority_id(&self) -> String {
                String::new()
            }
        }
    }

    // =========================================================================
    // Account file operations (isolated, async)
    // =========================================================================

    pub async fn create_account(&self, nickname_suggestion: &str) -> TerminalResult<()> {
        tracing::info!(
            nickname = nickname_suggestion,
            "io_context create_account begin"
        );
        let app_core = self.app_core_raw().clone();
        let (authority_id, _context_id) = self
            .account_files
            .create_account(nickname_suggestion)
            .await?;
        tracing::info!(%authority_id, "io_context create_account persisted account");
        {
            let mut core = app_core.write().await;
            core.set_authority(authority_id);
        }
        tracing::info!(
            %authority_id,
            "io_context create_account staged local authority; runtime bootstrap will occur after restart"
        );
        Ok(())
    }

    pub async fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::types::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::types::identifiers::ContextId>,
    ) -> TerminalResult<()> {
        self.account_files
            .restore_recovered_account(recovered_authority_id, recovered_context_id)
            .await?;

        let app_core = self.app_core_raw().clone();
        {
            let mut core = app_core.write().await;
            core.set_authority(recovered_authority_id);
        }

        let bootstrap = async move {
            let _ =
                context_workflows::create_home(&app_core, Some("Recovered Home".to_string()), None)
                    .await;
            let _ = settings_workflows::refresh_settings_from_runtime(&app_core).await;
            let _ = system_workflows::refresh_account(&app_core).await;
        };
        self.tasks.spawn(bootstrap);
        Ok(())
    }

    pub async fn export_account_backup(&self) -> TerminalResult<String> {
        self.account_files.export_account_backup().await
    }

    pub async fn import_account_backup(&self, backup_code: &str) -> TerminalResult<()> {
        self.account_files.import_account_backup(backup_code).await
    }

    // =========================================================================
    // View snapshots (synchronous, best-effort)
    // =========================================================================

    #[must_use]
    pub fn snapshot_chat(&self) -> ChatSnapshot {
        self.snapshots.snapshot_chat()
    }

    #[must_use]
    pub fn snapshot_contacts(&self) -> ContactsSnapshot {
        self.snapshots.snapshot_contacts()
    }

    #[must_use]
    pub fn snapshot_recovery(&self) -> RecoverySnapshot {
        self.snapshots.snapshot_recovery()
    }

    #[must_use]
    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        self.snapshots.snapshot_neighborhood()
    }

    #[must_use]
    pub fn snapshot_home(&self) -> HomeSnapshot {
        self.snapshots.snapshot_home()
    }

    #[must_use]
    pub fn snapshot_invitations(&self) -> InvitationsSnapshot {
        self.snapshots.snapshot_invitations()
    }

    #[must_use]
    pub fn snapshot_devices(&self) -> DevicesSnapshot {
        self.snapshots.snapshot_devices()
    }

    #[must_use]
    pub fn snapshot_guardians(&self) -> GuardiansSnapshot {
        self.snapshots.snapshot_guardians()
    }

    #[must_use]
    pub fn pending_runtime_bootstrap(&self) -> bool {
        self.pending_runtime_bootstrap
    }

    // =========================================================================
    // Command dispatch
    // =========================================================================

    pub async fn dispatch(&self, command: EffectCommand) -> TerminalResult<()> {
        // In demo mode, also route commands through the SimulatedBridge
        // so that simulated agents (Alice/Carol) can respond to them
        cfg_if! {
            if #[cfg(feature = "development")] {
                if let Some(bridge) = &self.demo_bridge {
                    bridge.route_command(&command).await;
                }
            }
        }

        self.dispatch.dispatch(command).await
    }

    pub async fn dispatch_with_response(
        &self,
        command: EffectCommand,
    ) -> TerminalResult<OpResponse> {
        cfg_if! {
            if #[cfg(feature = "development")] {
                if let Some(bridge) = &self.demo_bridge {
                    bridge.route_command(&command).await;
                }
            }
        }

        self.dispatch.dispatch_with_response(command).await
    }

    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> TerminalResult<()> {
        // In demo mode, also route commands through the SimulatedBridge
        cfg_if! {
            if #[cfg(feature = "development")] {
                if let Some(bridge) = &self.demo_bridge {
                    bridge.route_command(&command).await;
                }
            }
        }

        self.dispatch.dispatch_and_wait(command).await
    }

    pub async fn export_invitation_code(&self, invitation_id: &str) -> TerminalResult<String> {
        match self
            .operational
            .execute(&EffectCommand::ExportInvitation {
                invitation_id: invitation_id.to_string(),
            })
            .await
        {
            Some(Ok(OpResponse::InvitationCode { code, .. })) => Ok(code),
            Some(Ok(other)) => Err(TerminalError::structured_operation(
                OpFailureCode::ExportInvitation.as_str(),
                format!("Unexpected response: {other:?}"),
            )),
            Some(Err(err)) => {
                let terr: TerminalError = err.clone().into();
                self.operational.emit_error(terr).await;
                Err(err.into())
            }
            None => Err(TerminalError::NotImplemented(
                "ExportInvitation not handled".to_string(),
            )),
        }
    }

    pub async fn create_invitation_code(
        &self,
        receiver_id: AuthorityId,
        invitation_type: &str,
        message: Option<String>,
        ttl_secs: Option<u64>,
    ) -> TerminalResult<String> {
        match self
            .operational
            .execute(&EffectCommand::CreateInvitation {
                receiver_id,
                invitation_type: invitation_type.to_string(),
                message,
                ttl_secs,
            })
            .await
        {
            Some(Ok(OpResponse::InvitationCode { code, .. })) => Ok(code),
            Some(Ok(other)) => Err(TerminalError::structured_operation(
                OpFailureCode::CreateInvitation.as_str(),
                format!("Unexpected response: {other:?}"),
            )),
            Some(Err(err)) => {
                let terr: TerminalError = err.clone().into();
                self.operational.emit_error(terr).await;
                Err(err.into())
            }
            None => Err(TerminalError::NotImplemented(
                "CreateInvitation not handled".to_string(),
            )),
        }
    }

    pub async fn start_device_enrollment(
        &self,
        nickname_suggestion: &str,
        invitee_authority_id: Option<AuthorityId>,
    ) -> TerminalResult<DeviceEnrollmentStartInfo> {
        match self
            .operational
            .execute(&EffectCommand::AddDevice {
                nickname_suggestion: nickname_suggestion.to_string(),
                invitee_authority_id,
            })
            .await
        {
            Some(Ok(OpResponse::DeviceEnrollmentStarted {
                ceremony_id,
                enrollment_code,
                pending_epoch,
                device_id,
            })) => Ok(DeviceEnrollmentStartInfo {
                ceremony_id,
                enrollment_code,
                pending_epoch,
                device_id,
            }),
            Some(Ok(other)) => Err(TerminalError::structured_operation(
                OpFailureCode::StartDeviceEnrollment.as_str(),
                format!("Unexpected response: {other:?}"),
            )),
            Some(Err(err)) => {
                let terr: TerminalError = err.clone().into();
                self.operational.emit_error(terr).await;
                Err(err.into())
            }
            None => Err(TerminalError::NotImplemented(
                "AddDevice not handled".to_string(),
            )),
        }
    }

    pub async fn start_device_removal(&self, device_id: &str) -> TerminalResult<String> {
        match self
            .operational
            .execute(&EffectCommand::RemoveDevice {
                device_id: device_id.to_string(),
            })
            .await
        {
            Some(Ok(OpResponse::DeviceRemovalStarted { ceremony_id })) => Ok(ceremony_id),
            Some(Ok(other)) => Err(TerminalError::structured_operation(
                OpFailureCode::RemoveDevice.as_str(),
                format!("Unexpected response: {other:?}"),
            )),
            Some(Err(err)) => {
                let terr: TerminalError = err.clone().into();
                self.operational.emit_error(terr).await;
                Err(err.into())
            }
            None => Err(TerminalError::NotImplemented(
                "RemoveDevice not handled".to_string(),
            )),
        }
    }

    pub async fn import_device_enrollment_code(&self, code: &str) -> TerminalResult<()> {
        cfg_if! {
            if #[cfg(feature = "development")] {
                if self.demo_mobile_agent.is_some() {
                    return self.import_invitation_on_mobile(code).await;
                }
            }
        }

        let app_core = self.app_core_raw();
        let runtime_available = {
            let core = app_core.read().await;
            core.runtime().is_some()
        };
        if !runtime_available {
            let shareable = ShareableInvitation::from_code(code).map_err(|e| {
                TerminalError::structured_operation(
                    OpFailureCode::ImportDeviceEnrollmentCode.as_str(),
                    format!("Failed to parse device enrollment code: {e}"),
                )
            })?;
            let AgentInvitationType::DeviceEnrollment {
                subject_authority,
                device_id,
                nickname_suggestion,
                ..
            } = shareable.invitation_type
            else {
                return Err(TerminalError::Input(
                    "Code is not a device enrollment invitation".to_string(),
                ));
            };
            let nickname_suggestion =
                nickname_suggestion.unwrap_or_else(|| "Imported Device".to_string());
            let runtime_identity = BootstrapRuntimeIdentity::new(subject_authority, device_id);
            let (authority_id, _context_id) = self
                .account_files
                .create_account_with_device_enrollment_runtime_identity(
                    runtime_identity,
                    &nickname_suggestion,
                    code,
                )
                .await?;
            {
                let mut core = app_core.write().await;
                core.set_authority(authority_id);
            }
            return Ok(());
        }

        let invitation = import_invitation_details(app_core, code)
            .await
            .map_err(|e| {
                TerminalError::structured_operation(
                    OpFailureCode::ImportDeviceEnrollmentCode.as_str(),
                    format!("Failed to import invitation: {e}"),
                )
            })?;

        if !matches!(
            invitation.invitation_type,
            InvitationBridgeType::DeviceEnrollment { .. }
        ) {
            return Err(TerminalError::Input(
                "Code is not a device enrollment invitation".to_string(),
            ));
        }

        aura_app::ui::workflows::invitation::accept_device_enrollment_invitation(
            app_core,
            &invitation,
        )
        .await
        .map_err(|e| {
            TerminalError::structured_operation(
                OpFailureCode::ImportDeviceEnrollmentCode.as_str(),
                format!("Failed to accept device enrollment invitation: {e}"),
            )
        })?;

        Ok(())
    }

    pub async fn dispatch_send_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> TerminalResult<()> {
        self.dispatch(EffectCommand::SendMessage {
            channel: channel_id.to_string(),
            content: content.to_string(),
        })
        .await
    }

    pub async fn dispatch_join_channel(&self, channel_id: &str) -> TerminalResult<()> {
        self.dispatch(EffectCommand::JoinChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    pub async fn dispatch_leave_channel(&self, channel_id: &str) -> TerminalResult<()> {
        self.dispatch(EffectCommand::LeaveChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    pub async fn dispatch_start_recovery(&self) -> TerminalResult<()> {
        self.dispatch(EffectCommand::StartRecovery).await
    }

    pub async fn dispatch_submit_guardian_approval(&self, guardian_id: &str) -> TerminalResult<()> {
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
            return runtime
                .try_get_sync_status()
                .await
                .ok()
                .and_then(|status| status.last_sync_ms);
        }
        None
    }

    pub async fn known_peers_count(&self) -> usize {
        let (runtime, reactive) = match self.app_core.raw().try_read() {
            Some(core) => (core.runtime().cloned(), core.reactive().clone()),
            None => return 0,
        };

        if let Some(runtime) = runtime {
            if let Ok(status) = runtime.try_get_sync_status().await {
                if status.connected_peers > 0 {
                    return status.connected_peers;
                }
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
                .map(|p| (p.authority_id.to_string(), p.address.clone()))
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

        let Ok(lan_peers) = runtime.try_get_lan_peers().await else {
            return vec![];
        };
        lan_peers
            .iter()
            .map(|peer| (peer.authority_id.to_string(), peer.address.clone()))
            .collect()
    }

    // =========================================================================
    // LAN invitation tracking (UI-only)
    // =========================================================================

    pub async fn mark_peer_invited(&self, authority_id: &str) {
        match authority_id.parse::<AuthorityId>() {
            Ok(parsed) => {
                self.invited_lan_peers.write().await.insert(parsed);
            }
            Err(error) => {
                tracing::warn!(
                    "Ignoring non-AuthorityId LAN invitation marker '{}': {}",
                    authority_id,
                    error
                );
            }
        }
    }

    pub async fn is_peer_invited(&self, authority_id: &str) -> bool {
        let Ok(parsed) = authority_id.parse::<AuthorityId>() else {
            return false;
        };
        self.invited_lan_peers.read().await.contains(&parsed)
    }

    pub async fn get_invited_peer_ids(&self) -> HashSet<String> {
        self.invited_lan_peers
            .read()
            .await
            .iter()
            .map(ToString::to_string)
            .collect()
    }
    // =========================================================================
    // Settings helpers (via SETTINGS_SIGNAL)
    // =========================================================================

    pub async fn get_nickname_suggestion(&self) -> String {
        let core = self.app_core.raw().read().await;
        core.read(&*SETTINGS_SIGNAL)
            .await
            .unwrap_or_default()
            .nickname_suggestion
    }

    // Note: set_nickname_suggestion and set_mfa_policy were removed because they bypassed
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

    #[must_use]
    pub fn get_current_role(&self) -> Option<aura_app::ui::types::home::HomeRole> {
        let snapshot = self.snapshots.try_state_snapshot()?;
        let home_state = snapshot.homes.current_home()?;
        Some(home_state.my_role)
    }

    /// Check if the current user can execute a command based on its authorization level.
    ///
    /// This is a best-effort, UX-focused pre-check. Biscuit/guard-chain enforcement
    /// remains the source of truth.
    pub fn check_authorization(&self, command: &EffectCommand) -> TerminalResult<()> {
        // Delegate to portable authorization logic in aura-app
        let level = command.authorization_level();
        if matches!(level, crate::tui::effects::CommandAuthorizationLevel::Admin)
            && has_explicit_admin_scope(command)
        {
            return Ok(());
        }
        aura_app::ui::authorization::check_authorization_level(
            level,
            self.get_current_role(),
            command_name(command),
        )
        .map_err(TerminalError::Capability)
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
