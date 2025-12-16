//! # iocraft Context
//!
//! Self-contained context for iocraft TUI components.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::tui::context::IoContext;
//!
//! let ctx = IoContext::new(bridge);
//!
//! // Get reactive data snapshots
//! let chat = ctx.snapshot_chat();
//! let guardians = ctx.snapshot_guardians();
//!
//! // Get iocraft-compatible data for screens
//! let channels = ctx.get_channels();
//! let messages = ctx.get_messages();
//!
//! // Dispatch effects
//! ctx.dispatch_send_message("channel-1", "Hello!").await;
//! ```
//!
//! ## Note on Reactive Updates
//!
//! Screen components now subscribe directly to AppCore signals for reactive
//! updates. The snapshot methods in this context provide synchronous access
//! for initial rendering and fallback cases.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aura_app::AppCore;
use tokio::sync::RwLock;

use aura_app::signal_defs::{
    ConnectionStatus, SyncStatus, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL,
    RECOVERY_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_app::views::contacts::Contact as ViewContact;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::effects::{
    command_to_intent, CommandContext, EffectCommand, OpResponse, OperationalHandler,
};
use crate::tui::hooks::{
    BlockSnapshot, ChatSnapshot, ContactsSnapshot, DevicesSnapshot, GuardiansSnapshot,
    InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};
use crate::tui::types::{
    BlockBudget, Channel, ChannelMode, Contact, Guardian, Invitation, Message, RecoveryStatus,
    Resident,
};

/// iocraft-friendly context
///
/// Self-contained context providing snapshot-based access to AppCore ViewState
/// and effect dispatch for iocraft components.
///
/// ## AppCore Integration
///
/// This context delegates to aura-app's ViewState for all data access,
/// enabling the full intent-based state management flow:
///
/// ```text
/// Intent → Journal → Reduce → ViewState → TUI Snapshot
/// ```
///
/// ## Reactive Updates
///
/// Screen components subscribe directly to AppCore signals for push-based
/// reactive updates. This context provides synchronous snapshot access for
/// initial rendering.
#[derive(Clone)]
pub struct IoContext {
    /// Operational handler for non-journaled commands (Ping, sync, etc.)
    operational: Arc<OperationalHandler>,

    /// AppCore for intent-based state management
    /// This is the portable application core from aura-app
    /// Always available - demo mode uses AppCore without agent
    app_core: Arc<RwLock<AppCore>>,

    /// Whether an actual account exists (vs placeholder IDs for pre-setup state)
    /// When false, the account setup modal should be shown
    has_existing_account: Arc<std::sync::atomic::AtomicBool>,

    /// Base path for data storage (needed for account file creation)
    base_path: std::path::PathBuf,

    /// Device ID string (needed for account file creation)
    device_id_str: String,

    /// Demo mode hints (None in production mode)
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,

    /// Tracks authority_ids of peers that have been invited via LAN
    /// Used to display invitation status in the contacts screen
    invited_lan_peers: Arc<RwLock<HashSet<String>>>,

    /// User's display name / nickname
    /// This is the name shown in the Settings screen and shared with contacts
    display_name: Arc<RwLock<String>>,

    /// MFA policy setting
    /// Controls when multi-factor authentication is required
    mfa_policy: Arc<RwLock<crate::tui::types::MfaPolicy>>,

    /// Current active context (block/channel ID)
    /// Tracks the user's current navigation context for command targeting
    current_context: Arc<RwLock<Option<String>>>,

    /// Channel mode settings (channel_id -> mode flags)
    /// Stores local channel mode configuration
    channel_modes: Arc<RwLock<HashMap<String, ChannelMode>>>,

    /// Toast notifications for displaying errors/info in the UI
    /// These are shown temporarily at the top-right of the screen
    toasts: Arc<RwLock<Vec<crate::tui::components::ToastMessage>>>,
}

impl IoContext {
    /// Create a new IoContext with AppCore
    ///
    /// This is the primary constructor. AppCore provides:
    /// - Full ViewState signal infrastructure
    /// - Intent-based state management
    /// - Reactive signal subscriptions for screens
    pub fn new(
        app_core: Arc<RwLock<AppCore>>,
        base_path: std::path::PathBuf,
        device_id_str: String,
    ) -> Self {
        let operational = Arc::new(OperationalHandler::new(app_core.clone()));
        Self {
            operational,
            app_core,
            has_existing_account: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            base_path,
            device_id_str,
            #[cfg(feature = "development")]
            demo_hints: None,
            invited_lan_peers: Arc::new(RwLock::new(HashSet::new())),
            display_name: Arc::new(RwLock::new(String::new())),
            mfa_policy: Arc::new(RwLock::new(crate::tui::types::MfaPolicy::default())),
            current_context: Arc::new(RwLock::new(None)),
            channel_modes: Arc::new(RwLock::new(HashMap::new())),
            toasts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new IoContext with explicit account existence flag
    ///
    /// Use this constructor when you need to control whether the account setup
    /// modal should be shown. Pass `has_existing_account: false` to show the modal.
    pub fn with_account_status(
        app_core: Arc<RwLock<AppCore>>,
        has_existing_account: bool,
        base_path: std::path::PathBuf,
        device_id_str: String,
    ) -> Self {
        let operational = Arc::new(OperationalHandler::new(app_core.clone()));
        Self {
            operational,
            app_core,
            has_existing_account: Arc::new(std::sync::atomic::AtomicBool::new(
                has_existing_account,
            )),
            base_path,
            device_id_str,
            #[cfg(feature = "development")]
            demo_hints: None,
            invited_lan_peers: Arc::new(RwLock::new(HashSet::new())),
            display_name: Arc::new(RwLock::new(String::new())),
            mfa_policy: Arc::new(RwLock::new(crate::tui::types::MfaPolicy::default())),
            current_context: Arc::new(RwLock::new(None)),
            channel_modes: Arc::new(RwLock::new(HashMap::new())),
            toasts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new IoContext for demo mode with hints
    ///
    /// This constructor includes demo hints that provide contextual guidance
    /// and pre-generated invite codes for Alice and Carol.
    #[cfg(feature = "development")]
    pub fn with_demo_hints(
        app_core: Arc<RwLock<AppCore>>,
        hints: crate::demo::DemoHints,
        has_existing_account: bool,
        base_path: std::path::PathBuf,
        device_id_str: String,
    ) -> Self {
        let operational = Arc::new(OperationalHandler::new(app_core.clone()));
        Self {
            operational,
            app_core,
            has_existing_account: Arc::new(std::sync::atomic::AtomicBool::new(
                has_existing_account,
            )),
            base_path,
            device_id_str,
            demo_hints: Some(hints),
            invited_lan_peers: Arc::new(RwLock::new(HashSet::new())),
            display_name: Arc::new(RwLock::new(String::new())),
            mfa_policy: Arc::new(RwLock::new(crate::tui::types::MfaPolicy::default())),
            current_context: Arc::new(RwLock::new(None)),
            channel_modes: Arc::new(RwLock::new(HashMap::new())),
            toasts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get demo hints if in demo mode
    #[cfg(feature = "development")]
    pub fn demo_hints(&self) -> Option<&crate::demo::DemoHints> {
        self.demo_hints.as_ref()
    }

    /// Check if running in demo mode
    #[cfg(feature = "development")]
    pub fn is_demo_mode(&self) -> bool {
        self.demo_hints.is_some()
    }

    /// Check if running in demo mode (always false without development feature)
    #[cfg(not(feature = "development"))]
    pub fn is_demo_mode(&self) -> bool {
        false
    }

    /// Get Alice's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub fn demo_alice_code(&self) -> String {
        self.demo_hints
            .as_ref()
            .map(|h| h.alice_invite_code.clone())
            .unwrap_or_default()
    }

    /// Get Carol's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub fn demo_carol_code(&self) -> String {
        self.demo_hints
            .as_ref()
            .map(|h| h.carol_invite_code.clone())
            .unwrap_or_default()
    }

    /// Get Alice's invite code (empty without development feature)
    #[cfg(not(feature = "development"))]
    pub fn demo_alice_code(&self) -> String {
        String::new()
    }

    /// Get Carol's invite code (empty without development feature)
    #[cfg(not(feature = "development"))]
    pub fn demo_carol_code(&self) -> String {
        String::new()
    }

    /// Create with default configuration (demo mode with AppCore)
    ///
    /// Creates an AppCore without an agent, which provides:
    /// - Full ViewState signal infrastructure
    /// - Local-only intent dispatch
    /// - No network/sync capabilities
    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    pub fn with_defaults() -> Self {
        let app_core =
            AppCore::new(aura_app::AppConfig::default()).expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let operational = Arc::new(OperationalHandler::new(app_core.clone()));
        Self {
            operational,
            app_core,
            has_existing_account: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            base_path: std::path::PathBuf::from("./aura-data"),
            device_id_str: "default-device".to_string(),
            #[cfg(feature = "development")]
            demo_hints: None,
            invited_lan_peers: Arc::new(RwLock::new(HashSet::new())),
            display_name: Arc::new(RwLock::new(String::new())),
            mfa_policy: Arc::new(RwLock::new(crate::tui::types::MfaPolicy::default())),
            current_context: Arc::new(RwLock::new(None)),
            channel_modes: Arc::new(RwLock::new(HashMap::new())),
            toasts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Check if this context has AppCore integration
    ///
    /// Always returns true - AppCore is always available (demo mode uses agent-less AppCore)
    #[inline]
    pub fn has_app_core(&self) -> bool {
        true
    }

    /// Get the AppCore
    pub fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    /// Check if an account (authority) has been set up
    ///
    /// Returns true if an actual account exists (not placeholder IDs).
    /// When false, the account setup modal should be shown.
    pub fn has_account(&self) -> bool {
        self.has_existing_account
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Mark that an account has been created
    ///
    /// Called after the user completes the account setup modal.
    /// This updates the internal flag so `has_account()` returns true.
    pub fn set_account_created(&self) {
        self.has_existing_account
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Create a new account and save to disk
    ///
    /// This is the method that actually creates the account.json file.
    /// It should be called when the user completes the account setup modal.
    ///
    /// Returns Ok(()) on success, Err with message on failure.
    pub fn create_account(&self, _display_name: &str) -> Result<(), String> {
        use crate::handlers::tui::create_account;

        // Create the account file on disk
        match create_account(&self.base_path, &self.device_id_str) {
            Ok((authority_id, context_id)) => {
                // Update the flag to indicate account exists
                self.set_account_created();

                // Log success
                tracing::info!(
                    "Account created: authority={}, context={}",
                    authority_id,
                    context_id
                );

                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to create account: {}", e);
                Err(format!("Failed to create account: {}", e))
            }
        }
    }

    /// Restore an account from guardian-based recovery
    ///
    /// This is called after guardians have reconstructed the ORIGINAL authority_id
    /// via FROST threshold signatures. Unlike `create_account()` which derives
    /// the authority from device_id, this preserves the cryptographically identical
    /// authority from before the catastrophic device loss.
    ///
    /// # Arguments
    /// * `recovered_authority_id` - The ORIGINAL authority reconstructed by guardians
    /// * `recovered_context_id` - Optional context_id (generated if None)
    ///
    /// Returns Ok(()) on success, Err with message on failure.
    pub fn restore_recovered_account(
        &self,
        recovered_authority_id: aura_core::identifiers::AuthorityId,
        recovered_context_id: Option<aura_core::identifiers::ContextId>,
    ) -> Result<(), String> {
        use crate::handlers::tui::restore_recovered_account;

        // Restore the account file on disk with the RECOVERED authority
        match restore_recovered_account(
            &self.base_path,
            recovered_authority_id,
            recovered_context_id,
        ) {
            Ok((authority_id, context_id)) => {
                // Update the flag to indicate account exists
                self.set_account_created();

                // Log success
                tracing::info!(
                    "Account restored from recovery: authority={}, context={}",
                    authority_id,
                    context_id
                );

                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to restore recovered account: {}", e);
                Err(format!("Failed to restore recovered account: {}", e))
            }
        }
    }

    /// Export account to a portable backup code
    ///
    /// The backup code includes:
    /// - Account configuration (authority_id, context_id)
    /// - Journal facts (all state history)
    ///
    /// Format: `aura:backup:v1:<base64>`
    ///
    /// Returns the backup code string on success, error message on failure.
    pub fn export_account_backup(&self) -> Result<String, String> {
        use crate::handlers::tui::export_account_backup;

        if !self.has_account() {
            return Err("No account exists to backup".to_string());
        }

        match export_account_backup(&self.base_path, Some(&self.device_id_str)) {
            Ok(backup_code) => {
                tracing::info!("Account backup exported successfully");
                Ok(backup_code)
            }
            Err(e) => {
                tracing::error!("Failed to export backup: {}", e);
                Err(format!("Failed to export backup: {}", e))
            }
        }
    }

    /// Import and restore account from backup code
    ///
    /// This completely replaces the current account with the backup.
    /// Use with caution - existing data will be overwritten!
    ///
    /// # Arguments
    /// * `backup_code` - The backup code from `export_account_backup`
    ///
    /// Returns Ok(()) on success, error message on failure.
    pub fn import_account_backup(&self, backup_code: &str) -> Result<(), String> {
        use crate::handlers::tui::import_account_backup;

        // Allow overwrite for restoration
        match import_account_backup(&self.base_path, backup_code, true) {
            Ok((authority_id, context_id)) => {
                // Update the flag to indicate account exists
                self.set_account_created();

                tracing::info!(
                    "Account restored from backup: authority={}, context={}",
                    authority_id,
                    context_id
                );

                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to import backup: {}", e);
                Err(format!("Failed to import backup: {}", e))
            }
        }
    }

    /// Get a snapshot from AppCore (blocking read lock)
    ///
    /// Returns None if the lock is busy.
    fn app_core_snapshot(&self) -> Option<aura_app::StateSnapshot> {
        // Use try_read to avoid blocking indefinitely
        if let Ok(core) = self.app_core.try_read() {
            return Some(core.snapshot());
        }
        None
    }

    /// Build a CommandContext from current AppCore state.
    ///
    /// This extracts the current block ID and recovery context ID from the
    /// AppCore snapshot for use in command-to-intent mapping.
    fn build_command_context(&self) -> CommandContext {
        if let Some(snapshot) = self.app_core_snapshot() {
            CommandContext::from_snapshot(&snapshot)
        } else {
            // If we can't get a snapshot, use empty context (nil_context fallback)
            CommandContext::empty()
        }
    }

    // ─── Snapshot Accessors ─────────────────────────────────────────────────
    //
    // These methods read from AppCore's ViewState and provide snapshots
    // for initial rendering. Screens subscribe to AppCore signals directly
    // for reactive updates.

    /// Get a snapshot of chat data (channels and messages)
    pub fn snapshot_chat(&self) -> ChatSnapshot {
        if let Some(snapshot) = self.app_core_snapshot() {
            ChatSnapshot {
                channels: snapshot.chat.channels,
                selected_channel: snapshot.chat.selected_channel_id,
                messages: snapshot.chat.messages,
            }
        } else {
            ChatSnapshot::default()
        }
    }

    /// Get a snapshot of guardians data
    pub fn snapshot_guardians(&self) -> GuardiansSnapshot {
        if let Some(snapshot) = self.app_core_snapshot() {
            GuardiansSnapshot {
                guardians: snapshot.recovery.guardians.clone(),
                threshold: aura_core::threshold::ThresholdConfig::new(
                    snapshot.recovery.threshold as u16,
                    snapshot.recovery.guardian_count as u16,
                )
                .ok(),
            }
        } else {
            GuardiansSnapshot::default()
        }
    }

    /// Get a snapshot of recovery data
    pub fn snapshot_recovery(&self) -> RecoverySnapshot {
        // Try AppCore first
        if let Some(snapshot) = self.app_core_snapshot() {
            // Compute progress and in_progress status
            let is_in_progress = snapshot.recovery.active_recovery.is_some();
            let progress_percent = if let Some(ref process) = snapshot.recovery.active_recovery {
                if process.approvals_required > 0 {
                    ((process.approvals_received as f32 / process.approvals_required as f32)
                        * 100.0) as u32
                } else {
                    0
                }
            } else {
                0
            };

            return RecoverySnapshot {
                status: snapshot.recovery.clone(),
                progress_percent,
                is_in_progress,
            };
        }
        RecoverySnapshot::default()
    }

    /// Get a snapshot of invitations data
    pub fn snapshot_invitations(&self) -> InvitationsSnapshot {
        // Try AppCore first
        if let Some(snapshot) = self.app_core_snapshot() {
            // Combine pending, sent, and history into one list
            let invitations: Vec<_> = snapshot
                .invitations
                .pending
                .iter()
                .chain(snapshot.invitations.sent.iter())
                .chain(snapshot.invitations.history.iter())
                .cloned()
                .collect();
            let pending_count = snapshot.invitations.pending_count as usize;
            return InvitationsSnapshot {
                invitations,
                pending_count,
            };
        }
        InvitationsSnapshot::default()
    }

    /// Get a snapshot of block data
    pub fn snapshot_block(&self) -> BlockSnapshot {
        // Try AppCore first
        if let Some(snapshot) = self.app_core_snapshot() {
            let block = if !snapshot.block.name.is_empty() {
                Some(snapshot.block.clone())
            } else {
                None
            };

            return BlockSnapshot {
                block,
                is_resident: snapshot.block.resident_count > 0,
                is_steward: snapshot.block.is_admin(),
            };
        }
        BlockSnapshot::default()
    }

    /// Get a snapshot of contacts data
    pub fn snapshot_contacts(&self) -> ContactsSnapshot {
        // Try AppCore first
        if let Some(snapshot) = self.app_core_snapshot() {
            return ContactsSnapshot {
                contacts: snapshot.contacts.contacts.clone(),
                policy: aura_app::views::contacts::SuggestionPolicy::default(),
            };
        }
        ContactsSnapshot::default()
    }

    /// Get a snapshot of neighborhood data
    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        // Try AppCore first
        if let Some(snapshot) = self.app_core_snapshot() {
            return NeighborhoodSnapshot {
                neighborhood_id: Some(snapshot.neighborhood.home_block_id.clone()),
                neighborhood_name: Some(snapshot.neighborhood.home_block_name.clone()),
                blocks: snapshot.neighborhood.neighbors.clone(),
                position: snapshot
                    .neighborhood
                    .position
                    .clone()
                    .unwrap_or_default(),
            };
        }
        NeighborhoodSnapshot::default()
    }

    /// Get a snapshot of devices data
    ///
    /// Returns the list of devices registered for this account.
    /// Currently derives the current device from context; future versions
    /// will read additional devices from the commitment tree.
    pub fn snapshot_devices(&self) -> DevicesSnapshot {
        use crate::tui::types::Device;

        let current_device_id = self.device_id_str.clone();

        // Build device list - start with current device
        let devices = vec![Device::new(&current_device_id, "Current Device").current()];

        // Future: Read additional devices from TreeEffects::get_current_state()
        // The commitment tree stores devices as LeafNode entries with role=Device
        // For now, we only show the current device

        DevicesSnapshot {
            devices,
            current_device_id: Some(current_device_id),
        }
    }

    // Backwards-compatible view helpers for tests and harnesses.
    pub fn chat_view(&self) -> ChatSnapshot {
        self.snapshot_chat()
    }

    pub fn guardians_view(&self) -> GuardiansSnapshot {
        self.snapshot_guardians()
    }

    pub fn recovery_view(&self) -> RecoverySnapshot {
        self.snapshot_recovery()
    }

    pub fn invitations_view(&self) -> InvitationsSnapshot {
        self.snapshot_invitations()
    }

    pub fn block_view(&self) -> BlockSnapshot {
        self.snapshot_block()
    }

    pub fn devices_view(&self) -> DevicesSnapshot {
        self.snapshot_devices()
    }

    // ─── iocraft-Compatible Getters ────────────────────────────────────────

    /// Get channels as iocraft types for ChatScreen
    pub fn get_channels(&self) -> Vec<Channel> {
        let chat = self.snapshot_chat();
        let selected_id = chat.selected_channel.as_deref();
        chat.channels
            .iter()
            .map(|c| Channel::from_app(c, selected_id == Some(c.id.as_str())))
            .collect()
    }

    /// Get messages for current channel as iocraft types
    pub fn get_messages(&self) -> Vec<Message> {
        let chat = self.snapshot_chat();
        chat.messages.iter().map(|m| m.into()).collect()
    }

    /// Get selected channel ID
    pub fn get_selected_channel(&self) -> Option<String> {
        self.snapshot_chat().selected_channel
    }

    /// Get guardians as iocraft types
    pub fn get_guardians(&self) -> Vec<Guardian> {
        let snap = self.snapshot_guardians();
        snap.guardians.iter().map(|g| g.into()).collect()
    }

    /// Get recovery status as iocraft type
    pub fn get_recovery_status(&self) -> RecoveryStatus {
        let snap = self.snapshot_recovery();
        (&snap.status).into()
    }

    /// Get invitations as iocraft types
    pub fn get_invitations(&self) -> Vec<Invitation> {
        let snap = self.snapshot_invitations();
        snap.invitations.iter().map(|i| i.into()).collect()
    }

    /// Get contacts as iocraft types
    pub fn get_contacts(&self) -> Vec<Contact> {
        let snap = self.snapshot_contacts();
        snap.contacts.iter().map(|c| c.into()).collect()
    }

    /// Get block residents as iocraft types
    pub fn get_residents(&self) -> Vec<Resident> {
        let snap = self.snapshot_block();
        snap.residents().iter().map(|r| r.into()).collect()
    }

    /// Get block budget as iocraft type
    pub fn get_block_budget(&self) -> BlockBudget {
        let snap = self.snapshot_block();
        let mut budget: BlockBudget = (&snap.storage()).into();
        budget.resident_count = snap.residents().len() as u8;
        budget
    }

    /// Get devices as iocraft types
    pub fn get_devices(&self) -> Vec<crate::tui::types::Device> {
        self.snapshot_devices().devices
    }

    // ─── Effect Dispatch ────────────────────────────────────────────────────
    //
    // Dispatch strategy:
    // 1. If command maps to an Intent → dispatch through AppCore (journaled)
    // 2. If command is operational (no Intent) → dispatch through OperationalHandler
    //
    // All commands are handled by one of these two paths. The unified approach
    // enables intent-based state management with signals for UI updates.

    /// Dispatch a command (fire and forget)
    ///
    /// Dispatch strategy:
    /// 0. Check authorization - Admin commands require Steward role
    /// 1. Handle backup commands directly (need IoContext access)
    /// 2. If command maps to an Intent → dispatch through AppCore (journaled)
    /// 3. If command is operational → dispatch through OperationalHandler
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        // Check authorization before dispatching
        self.check_authorization(&command)?;

        // Handle backup commands directly (they need IoContext access for base_path)
        match &command {
            EffectCommand::ExportAccountBackup => {
                // Export is handled specially - returns the code
                // but for dispatch() we just verify it works
                return self.export_account_backup().map(|_| ());
            }
            EffectCommand::ImportAccountBackup { backup_code } => {
                return self.import_account_backup(backup_code);
            }
            _ => {} // Continue with normal dispatch
        }

        // Build command context from current state for proper ID resolution
        let cmd_ctx = self.build_command_context();

        // Try to map command to intent for unified dispatch
        if let Some(intent) = command_to_intent(&command, &cmd_ctx) {
            // Dispatch through AppCore (journaled operation)
            let mut core = self.app_core.write().await;
            match core.dispatch(intent) {
                Ok(_fact_id) => {
                    // Commit pending facts and emit to reactive signals
                    // This is critical: dispatch() only queues facts, we must commit and emit
                    // to notify UI subscribers (ChatScreen, ContactsScreen, etc.)
                    if let Err(e) = core.commit_pending_facts_and_emit().await {
                        tracing::warn!("Failed to commit facts or emit signals: {}", e);
                    }
                    Ok(())
                }
                Err(e) => Err(format!("Intent dispatch failed: {}", e)),
            }
        } else if let Some(result) = self.operational.execute(&command).await {
            // Handle operational command, checking for special responses
            match result {
                Ok(OpResponse::ContextChanged { context_id }) => {
                    // Update the current context in IoContext
                    self.set_current_context(context_id).await;
                    Ok(())
                }
                Ok(OpResponse::ChannelModeSet { channel_id, flags }) => {
                    // Update channel mode in IoContext
                    self.set_channel_mode(&channel_id, &flags).await;
                    Ok(())
                }
                Ok(OpResponse::NicknameUpdated { name }) => {
                    // Update display name in IoContext
                    self.set_display_name(&name).await;
                    Ok(())
                }
                Ok(OpResponse::MfaPolicyUpdated { require_mfa }) => {
                    // Update MFA policy in IoContext
                    use crate::tui::types::MfaPolicy;
                    let policy = if require_mfa {
                        MfaPolicy::SensitiveOnly
                    } else {
                        MfaPolicy::Disabled
                    };
                    self.set_mfa_policy(policy).await;
                    Ok(())
                }
                Ok(OpResponse::InvitationImported {
                    sender_id,
                    invitation_type,
                    message,
                    ..
                }) => {
                    // Add the sender as a contact
                    self.add_contact_from_invitation(
                        &sender_id,
                        &invitation_type,
                        message.as_deref(),
                    )
                    .await;
                    Ok(())
                }
                Ok(OpResponse::Ok) => {
                    // Command succeeded with no data - intentionally no-op
                    Ok(())
                }
                Ok(OpResponse::Data(data)) => {
                    // Log the returned data for debugging
                    tracing::info!("Command returned data: {}", data);
                    Ok(())
                }
                Ok(OpResponse::List(items)) => {
                    // Log the returned list with {} items: {:?}", items.len(), items);
                    tracing::info!("Command returned list with {} items", items.len());
                    Ok(())
                }
                Ok(OpResponse::InvitationCode { id, code }) => {
                    // Show the generated invitation code to the user
                    tracing::info!("Generated invitation code for {}: {}", id, code);
                    self.add_success_toast("invitation-code", format!("Invitation code: {}", code))
                        .await;
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            // Unknown command - log warning and return error
            tracing::warn!(
                "Unknown command not handled by Intent or Operational: {:?}",
                command
            );
            Err(format!("Unknown command: {:?}", command))
        }
    }

    /// Export an invitation code and return the generated code
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
            Some(Err(err)) => Err(err.to_string()),
            None => Err("ExportInvitation not handled".to_string()),
        }
    }

    /// Dispatch a command and wait for completion
    ///
    /// Dispatch strategy:
    /// 0. Check authorization - Admin commands require Steward role
    /// 1. Handle backup commands directly (need IoContext access)
    /// 2. If command maps to an Intent → dispatch through AppCore (journaled)
    /// 3. If command is operational → dispatch through OperationalHandler
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        // Check authorization before dispatching
        self.check_authorization(&command)?;

        // Handle backup commands directly (they need IoContext access for base_path)
        match &command {
            EffectCommand::ExportAccountBackup => {
                return self.export_account_backup().map(|_| ());
            }
            EffectCommand::ImportAccountBackup { backup_code } => {
                return self.import_account_backup(backup_code);
            }
            _ => {} // Continue with normal dispatch
        }

        // Build command context from current state for proper ID resolution
        let cmd_ctx = self.build_command_context();

        // Try to map command to intent for unified dispatch
        if let Some(intent) = command_to_intent(&command, &cmd_ctx) {
            // Dispatch through AppCore (journaled operation)
            let mut core = self.app_core.write().await;
            match core.dispatch(intent) {
                Ok(_fact_id) => {
                    // Commit pending facts and emit to reactive signals
                    // This is critical: dispatch() only queues facts, we must commit and emit
                    // to notify UI subscribers (ChatScreen, ContactsScreen, etc.)
                    if let Err(e) = core.commit_pending_facts_and_emit().await {
                        tracing::warn!("Failed to commit facts or emit signals: {}", e);
                    }
                    Ok(())
                }
                Err(e) => Err(format!("Intent dispatch failed: {}", e)),
            }
        } else if let Some(result) = self.operational.execute(&command).await {
            // Handle operational command, checking for special responses
            match result {
                Ok(OpResponse::ContextChanged { context_id }) => {
                    // Update the current context in IoContext
                    self.set_current_context(context_id).await;
                    Ok(())
                }
                Ok(OpResponse::ChannelModeSet { channel_id, flags }) => {
                    // Update channel mode in IoContext
                    self.set_channel_mode(&channel_id, &flags).await;
                    Ok(())
                }
                Ok(OpResponse::NicknameUpdated { name }) => {
                    // Update display name in IoContext
                    self.set_display_name(&name).await;
                    Ok(())
                }
                Ok(OpResponse::MfaPolicyUpdated { require_mfa }) => {
                    // Update MFA policy in IoContext
                    use crate::tui::types::MfaPolicy;
                    let policy = if require_mfa {
                        MfaPolicy::SensitiveOnly
                    } else {
                        MfaPolicy::Disabled
                    };
                    self.set_mfa_policy(policy).await;
                    Ok(())
                }
                Ok(OpResponse::InvitationImported {
                    sender_id,
                    invitation_type,
                    message,
                    ..
                }) => {
                    // Add the sender as a contact
                    self.add_contact_from_invitation(
                        &sender_id,
                        &invitation_type,
                        message.as_deref(),
                    )
                    .await;
                    Ok(())
                }
                Ok(OpResponse::Ok) => {
                    // Command succeeded with no data - intentionally no-op
                    Ok(())
                }
                Ok(OpResponse::Data(data)) => {
                    // Log the returned data for debugging
                    tracing::info!("Command returned data: {}", data);
                    Ok(())
                }
                Ok(OpResponse::List(items)) => {
                    // Log the returned list with {} items: {:?}", items.len(), items);
                    tracing::info!("Command returned list with {} items", items.len());
                    Ok(())
                }
                Ok(OpResponse::InvitationCode { id, code }) => {
                    // Show the generated invitation code to the user
                    tracing::info!("Generated invitation code for {}: {}", id, code);
                    self.add_success_toast("invitation-code", format!("Invitation code: {}", code))
                        .await;
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            // Unknown command - log warning and return error
            tracing::warn!(
                "Unknown command not handled by Intent or Operational: {:?}",
                command
            );
            Err(format!("Unknown command: {:?}", command))
        }
    }

    /// Send a message to a channel
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

    /// Join a channel
    pub async fn dispatch_join_channel(&self, channel_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::JoinChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    /// Leave a channel
    pub async fn dispatch_leave_channel(&self, channel_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::LeaveChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    /// Start recovery
    pub async fn dispatch_start_recovery(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::StartRecovery).await
    }

    /// Submit guardian approval for recovery
    pub async fn dispatch_submit_guardian_approval(&self, guardian_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::SubmitGuardianApproval {
            guardian_id: guardian_id.to_string(),
        })
        .await
    }

    // ─── Connection Status (via Signals) ───────────────────────────────────

    /// Check if connected to the effect system
    ///
    /// Reads from the CONNECTION_STATUS_SIGNAL to determine connection state.
    pub async fn is_connected(&self) -> bool {
        if let Ok(core) = self.app_core.try_read() {
            if let Ok(status) = core.read(&*CONNECTION_STATUS_SIGNAL).await {
                return matches!(status, ConnectionStatus::Online { .. });
            }
        }
        false
    }

    /// Get last error if any
    ///
    /// Reads from the ERROR_SIGNAL to get the most recent error.
    pub async fn last_error(&self) -> Option<String> {
        if let Ok(core) = self.app_core.try_read() {
            if let Ok(error) = core.read(&*ERROR_SIGNAL).await {
                return error.map(|e| e.message);
            }
        }
        None
    }

    // ─── Sync Status (via Signals) ─────────────────────────────────────────

    /// Check if a sync operation is currently in progress
    ///
    /// Reads from the SYNC_STATUS_SIGNAL to determine sync state.
    pub async fn is_syncing(&self) -> bool {
        if let Ok(core) = self.app_core.try_read() {
            if let Ok(status) = core.read(&*SYNC_STATUS_SIGNAL).await {
                return matches!(status, SyncStatus::Syncing { .. });
            }
        }
        false
    }

    /// Get the timestamp of the last successful sync (ms since epoch)
    pub async fn last_sync_time(&self) -> Option<u64> {
        if let Ok(core) = self.app_core.try_read() {
            if let Some(status) = core.sync_status().await {
                if let Some(ts) = status.last_sync_ms {
                    return Some(ts);
                }
            }
        }

        None
    }

    /// Get the number of known peers for sync operations
    pub async fn known_peers_count(&self) -> usize {
        if let Ok(core) = self.app_core.try_read() {
            if let Some(status) = core.sync_status().await {
                if status.connected_peers > 0 {
                    return status.connected_peers;
                }
            }

            if let Ok(ConnectionStatus::Online { peer_count }) =
                core.read(&*CONNECTION_STATUS_SIGNAL).await
            {
                return peer_count;
            }
        }

        0
    }

    /// Get discovered peers from rendezvous service
    ///
    /// Returns a list of (authority_id, address) pairs for discovered peers.
    /// Returns empty list if no runtime is available.
    pub async fn get_discovered_peers(&self) -> Vec<(String, String)> {
        let core = self.app_core.read().await;

        // Get discovered peers from rendezvous
        match core.discover_peers().await {
            Ok(peers) => peers
                .iter()
                .map(|a| (a.to_string(), String::new())) // No address available from this API
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Mark a LAN peer as having been invited
    ///
    /// Call this after successfully dispatching an InviteLanPeer command.
    /// The contacts screen will show these peers with "pending" status.
    pub async fn mark_peer_invited(&self, authority_id: &str) {
        let mut invited = self.invited_lan_peers.write().await;
        invited.insert(authority_id.to_string());
    }

    /// Check if a LAN peer has been invited
    ///
    /// Returns true if `mark_peer_invited` was called for this authority_id.
    pub async fn is_peer_invited(&self, authority_id: &str) -> bool {
        let invited = self.invited_lan_peers.read().await;
        invited.contains(authority_id)
    }

    /// Get all invited peer authority_ids
    ///
    /// Returns a set of authority_ids for peers that have been invited.
    pub async fn get_invited_peer_ids(&self) -> HashSet<String> {
        let invited = self.invited_lan_peers.read().await;
        invited.clone()
    }

    // =========================================================================
    // Display Name / Nickname Methods
    // =========================================================================

    /// Get the current display name
    ///
    /// Returns the user's display name, or empty string if not set.
    pub async fn get_display_name(&self) -> String {
        let name = self.display_name.read().await;
        name.clone()
    }

    /// Set the user's display name
    ///
    /// Updates the display name in memory. In the future, this should also
    /// persist to account.json or a settings file.
    pub async fn set_display_name(&self, name: &str) {
        let mut display_name = self.display_name.write().await;
        *display_name = name.to_string();
        tracing::info!("Display name updated to: {}", name);
    }

    // =========================================================================
    // MFA Policy Methods
    // =========================================================================

    /// Get the current MFA policy
    ///
    /// Returns the user's MFA policy setting.
    pub async fn get_mfa_policy(&self) -> crate::tui::types::MfaPolicy {
        let policy = self.mfa_policy.read().await;
        *policy
    }

    /// Set the MFA policy
    ///
    /// Updates the MFA policy in memory. In the future, this should also
    /// persist to account.json or a settings file.
    pub async fn set_mfa_policy(&self, policy: crate::tui::types::MfaPolicy) {
        let mut mfa_policy = self.mfa_policy.write().await;
        *mfa_policy = policy;
        tracing::info!("MFA policy updated to: {:?}", policy);
    }

    // =========================================================================
    // Current Context Methods
    // =========================================================================

    /// Get the current active context (block/channel ID)
    ///
    /// Returns the ID of the currently active context for navigation and
    /// command targeting. Returns None if no context is set.
    pub async fn get_current_context(&self) -> Option<String> {
        let context = self.current_context.read().await;
        context.clone()
    }

    /// Set the current active context (block/channel ID)
    ///
    /// Updates the active context ID for navigation and command targeting.
    /// Pass None to clear the context.
    pub async fn set_current_context(&self, context_id: Option<String>) {
        let mut current_context = self.current_context.write().await;
        *current_context = context_id.clone();
        tracing::debug!("Current context updated to: {:?}", context_id);
    }

    /// Get channel mode for a specific channel
    ///
    /// Returns the mode flags for a channel, or default if not set.
    pub async fn get_channel_mode(&self, channel_id: &str) -> ChannelMode {
        let modes = self.channel_modes.read().await;
        modes.get(channel_id).cloned().unwrap_or_default()
    }

    /// Set channel mode flags for a channel
    ///
    /// Parses IRC-style mode string (e.g., "+mpt" or "-i") and updates the channel's mode.
    pub async fn set_channel_mode(&self, channel_id: &str, flags: &str) {
        let mut modes = self.channel_modes.write().await;
        let mode = modes.entry(channel_id.to_string()).or_default();
        mode.parse_flags(flags);
        tracing::debug!(
            "Channel {} mode updated to: {}",
            channel_id,
            mode.to_string()
        );
    }

    /// Add a contact from an imported invitation
    ///
    /// Called when an invitation is successfully imported. Adds the sender
    /// as a contact in the CONTACTS_SIGNAL.
    ///
    /// **Important**: Importing an invitation does NOT make someone a guardian.
    /// The `is_guardian` flag indicates "this person is MY guardian", which
    /// requires a complete guardian acceptance flow:
    /// 1. User creates guardian invitation and shares with contact
    /// 2. Contact imports invitation (they become a contact, NOT a guardian)
    /// 3. Contact accepts the invitation
    /// 4. Contact becomes the user's guardian
    ///
    /// When WE import someone else's guardian invitation, they want US to be
    /// THEIR guardian - they don't become OUR guardian.
    pub async fn add_contact_from_invitation(
        &self,
        sender_id: &str,
        _invitation_type: &str,
        message: Option<&str>,
    ) {
        // Extract name from message if available (demo invitations include name)
        // Format: "Guardian invitation from Alice (demo)"
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

        // Create contact entry
        // NOTE: is_guardian is always false when importing - guardian status
        // is only established after completing the guardian acceptance flow
        let contact = ViewContact {
            id: sender_id.to_string(),
            petname: suggested_name.clone().unwrap_or_default(),
            suggested_name,
            is_guardian: false, // Never set on import - requires acceptance flow
            is_resident: false,
            last_interaction: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            ),
            is_online: true, // Demo agents are "online"
        };

        // Update CONTACTS_SIGNAL
        // Note: Use .read().await (not try_read()) to properly wait for the lock.
        // try_read() would silently fail if the lock is held during TUI rendering.
        let core = self.app_core.read().await;
        if let Ok(mut contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
            // Check if contact already exists
            if !contacts_state.contacts.iter().any(|c| c.id == sender_id) {
                contacts_state.contacts.push(contact);
                if let Err(e) = core.emit(&*CONTACTS_SIGNAL, contacts_state).await {
                    tracing::warn!("Failed to update contacts signal: {}", e);
                } else {
                    tracing::info!("Added contact from invitation: {}", sender_id);
                }
            } else {
                tracing::debug!("Contact {} already exists, skipping", sender_id);
            }
        }
    }

    /// Toggle guardian status for a contact (demo mode)
    ///
    /// In demo mode, when user selects a contact as guardian, we immediately
    /// mark them as a guardian (simulating instant acceptance).
    ///
    /// This updates both:
    /// - CONTACTS_SIGNAL: Sets `is_guardian` flag on the contact
    /// - RECOVERY_SIGNAL: Adds/removes the contact from guardian list
    pub async fn toggle_contact_guardian(&self, contact_id: &str, is_guardian: bool) {
        tracing::info!(
            "Toggling guardian status for contact {}: {}",
            contact_id,
            is_guardian
        );

        let core = self.app_core.read().await;

        // 1. Update CONTACTS_SIGNAL
        if let Ok(mut contacts_state) = core.read(&*CONTACTS_SIGNAL).await {
            if let Some(contact) = contacts_state
                .contacts
                .iter_mut()
                .find(|c| c.id == contact_id)
            {
                contact.is_guardian = is_guardian;
                if let Err(e) = core.emit(&*CONTACTS_SIGNAL, contacts_state.clone()).await {
                    tracing::warn!("Failed to update contacts signal: {}", e);
                }
            }
        }

        // 2. Update RECOVERY_SIGNAL
        if let Ok(mut recovery_state) = core.read(&*RECOVERY_SIGNAL).await {
            recovery_state.toggle_guardian(contact_id.to_string(), is_guardian);
            if let Err(e) = core.emit(&*RECOVERY_SIGNAL, recovery_state).await {
                tracing::warn!("Failed to update recovery signal: {}", e);
            } else {
                tracing::info!(
                    "Guardian status toggled for {}: is_guardian={}",
                    contact_id,
                    is_guardian
                );
            }
        }
    }

    // =========================================================================
    // Toast Notifications
    // =========================================================================

    /// Add a toast notification
    ///
    /// Toasts are displayed temporarily at the top-right of the screen.
    /// Use for errors, success messages, and other notifications.
    pub async fn add_toast(&self, toast: crate::tui::components::ToastMessage) {
        let mut toasts = self.toasts.write().await;
        // Deduplicate: if a toast with the same ID exists, don't add another
        if toasts.iter().any(|t| t.id == toast.id) {
            return;
        }
        // Limit to 5 toasts max to avoid UI clutter
        if toasts.len() >= 5 {
            toasts.remove(0);
        }
        toasts.push(toast);
    }

    /// Add an error toast notification
    ///
    /// Convenience method for error messages.
    pub async fn add_error_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        use crate::tui::components::ToastMessage;
        self.add_toast(ToastMessage::error(id, message)).await;
    }

    /// Add a success toast notification
    ///
    /// Convenience method for success messages.
    pub async fn add_success_toast(&self, id: impl Into<String>, message: impl Into<String>) {
        use crate::tui::components::ToastMessage;
        self.add_toast(ToastMessage::success(id, message)).await;
    }

    /// Get all current toast notifications
    pub async fn get_toasts(&self) -> Vec<crate::tui::components::ToastMessage> {
        let toasts = self.toasts.read().await;
        toasts.clone()
    }

    /// Clear a specific toast by id
    pub async fn clear_toast(&self, id: &str) {
        let mut toasts = self.toasts.write().await;
        toasts.retain(|t| t.id != id);
    }

    /// Clear all toast notifications
    pub async fn clear_toasts(&self) {
        let mut toasts = self.toasts.write().await;
        toasts.clear();
    }

    // =========================================================================
    // Capability Checking
    // =========================================================================

    /// Get the current user's role in the current block context
    ///
    /// Returns `None` if:
    /// - There is no current context (not in a block)
    /// - The AppCore lock is busy
    pub fn get_current_role(&self) -> Option<aura_app::views::block::ResidentRole> {
        // Get a snapshot from AppCore
        let snapshot = self.app_core_snapshot()?;

        // Get the current block from BlocksState (multi-block support)
        // Fall back to the legacy singular block field for backwards compatibility
        let block = snapshot.blocks.current_block().unwrap_or(&snapshot.block);

        // BlockState has a `my_role` field that tracks the current user's role
        Some(block.my_role)
    }

    /// Check if the current user has a specific capability
    ///
    /// Capability mapping:
    /// - `None` capability: Always allowed
    /// - User-level capabilities: Any resident
    /// - Moderator/Admin capabilities: Admin or Owner only
    pub fn has_capability(&self, capability: &crate::tui::commands::CommandCapability) -> bool {
        use aura_app::views::block::ResidentRole;
        use crate::tui::commands::CommandCapability;

        // None capability is always allowed
        if matches!(capability, CommandCapability::None) {
            return true;
        }

        // Get current role
        let role = match self.get_current_role() {
            Some(r) => r,
            None => {
                // Not in a block context - only allow basic user commands
                // that don't require block membership
                return matches!(
                    capability,
                    CommandCapability::SendDm | CommandCapability::UpdateContact
                );
            }
        };

        // Check capability against role
        match capability {
            // Always allowed
            CommandCapability::None => true,

            // User-level capabilities - any resident can do these
            CommandCapability::SendDm
            | CommandCapability::SendMessage
            | CommandCapability::UpdateContact
            | CommandCapability::ViewMembers
            | CommandCapability::JoinChannel
            | CommandCapability::LeaveContext => true,

            // Moderator capabilities - require Admin or Owner role
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

    /// Check capability and return a detailed error if unauthorized
    pub fn check_capability(
        &self,
        capability: &crate::tui::commands::CommandCapability,
    ) -> Result<(), crate::tui::effects::DispatchError> {
        if self.has_capability(capability) {
            Ok(())
        } else {
            Err(crate::tui::effects::DispatchError::PermissionDenied {
                required: capability.clone(),
            })
        }
    }

    /// Check if the current user can execute a command based on its authorization level
    ///
    /// Authorization level mapping:
    /// - `Public` - Always allowed (read-only, status queries)
    /// - `Basic` - Always allowed (user token assumed in TUI)
    /// - `Sensitive` - Always allowed (account owner operations)
    /// - `Admin` - Requires Admin or Owner role in current block
    pub fn check_authorization(&self, command: &EffectCommand) -> Result<(), String> {
        use aura_app::views::block::ResidentRole;
        use crate::tui::effects::CommandAuthorizationLevel;

        let level = command.authorization_level();

        match level {
            // Public, Basic, Sensitive are always allowed
            CommandAuthorizationLevel::Public
            | CommandAuthorizationLevel::Basic
            | CommandAuthorizationLevel::Sensitive => Ok(()),

            // Admin requires Admin or Owner role
            CommandAuthorizationLevel::Admin => {
                let role = self.get_current_role();
                match role {
                    Some(ResidentRole::Admin | ResidentRole::Owner) => Ok(()),
                    Some(ResidentRole::Resident) => Err(format!(
                        "Permission denied: {} requires administrator privileges",
                        Self::command_name(command)
                    )),
                    None => {
                        // Not in a block context - some admin commands might still be
                        // allowed depending on the operation, but by default deny
                        Err(format!(
                            "Permission denied: {} requires a block context",
                            Self::command_name(command)
                        ))
                    }
                }
            }
        }
    }

    /// Get a human-readable name for a command (for error messages)
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
}

impl Default for IoContext {
    fn default() -> Self {
        Self::with_defaults()
    }
}
/// Trait for iocraft props that need context access
///
/// Implement this to enable context injection into components.
pub trait HasContext {
    /// Set the context reference
    fn set_context(&mut self, ctx: Arc<IoContext>);

    /// Get the context reference
    fn context(&self) -> Option<&Arc<IoContext>>;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::disallowed_methods)] // Test code uses std::fs for setup/teardown
    use super::*;

    #[tokio::test]
    async fn test_io_context_creation() {
        let ctx = IoContext::with_defaults();

        // Verify snapshots work
        let chat = ctx.snapshot_chat();
        assert!(chat.channels.is_empty());

        let guardians = ctx.snapshot_guardians();
        assert!(guardians.guardians.is_empty());
    }

    #[tokio::test]
    async fn test_io_context_dispatch() {
        let ctx = IoContext::with_defaults();

        // Dispatch should succeed (command is processed)
        let result = ctx.dispatch(EffectCommand::Ping).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_export_invitation_code_returns_code() {
        let ctx = IoContext::with_defaults();
        let code = ctx
            .export_invitation_code("inv-123")
            .await
            .expect("expected code");

        // Now generates proper shareable invitation codes in aura:v1: format
        assert!(
            code.starts_with("aura:v1:"),
            "Expected aura:v1: prefix, got: {}",
            code
        );
    }

    #[tokio::test]
    async fn test_create_account_writes_file() {
        // Set up isolated test directory
        let test_dir = std::env::temp_dir().join(format!("aura-ctx-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("Failed to create test dir");

        // Create AppCore
        let app_core =
            AppCore::new(aura_app::AppConfig::default()).expect("Failed to create AppCore");
        let app_core = Arc::new(RwLock::new(app_core));

        // Create IoContext with the test directory
        let ctx = IoContext::with_account_status(
            app_core,
            false, // No existing account
            test_dir.clone(),
            "test-device".to_string(),
        );

        // Verify no account exists initially
        assert!(!ctx.has_account(), "Should not have account initially");

        // Account file should not exist
        let account_file = test_dir.join("account.json");
        assert!(
            !account_file.exists(),
            "account.json should not exist before creation"
        );

        // Create account
        let result = ctx.create_account("Test User");
        assert!(
            result.is_ok(),
            "create_account should succeed: {:?}",
            result
        );

        // Verify flag updated
        assert!(ctx.has_account(), "Should have account after creation");

        // CRITICAL: Verify the account.json file was written
        assert!(
            account_file.exists(),
            "account.json should exist after creation"
        );

        // Verify file content
        let content = std::fs::read_to_string(&account_file).expect("Failed to read account.json");
        assert!(
            content.contains("authority_id"),
            "Should contain authority_id"
        );
        assert!(content.contains("context_id"), "Should contain context_id");

        // Cleanup
        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
