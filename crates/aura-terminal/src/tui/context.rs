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

use std::sync::Arc;

use aura_app::AppCore;
use tokio::sync::RwLock;

use crate::tui::effects::{AuraEvent, EffectBridge, EffectCommand, EventFilter, EventSubscription};
use crate::tui::hooks::{
    BlockSnapshot, ChatSnapshot, ContactsSnapshot, GuardiansSnapshot, InvitationsSnapshot,
    NeighborhoodSnapshot, RecoverySnapshot,
};
use crate::tui::types::{
    BlockBudget, Channel, Contact, Guardian, Invitation, Message, RecoveryStatus, Resident,
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
    /// Effect bridge for command dispatch
    bridge: Arc<EffectBridge>,

    /// AppCore for intent-based state management
    /// This is the portable application core from aura-app
    /// Always available - demo mode uses AppCore without agent
    app_core: Arc<RwLock<AppCore>>,

    /// Whether an actual account exists (vs placeholder IDs for pre-setup state)
    /// When false, the account setup modal should be shown
    has_existing_account: bool,

    /// Demo mode hints (None in production mode)
    #[cfg(feature = "development")]
    demo_hints: Option<crate::demo::DemoHints>,
}

impl IoContext {
    /// Create a new IoContext with an effect bridge (demo mode with default AppCore)
    ///
    /// Creates an AppCore without an agent, which provides:
    /// - Full ViewState signal infrastructure
    /// - Local-only intent dispatch
    /// - No network/sync capabilities
    ///
    /// Note: The provided bridge should be created with `EffectBridge::with_app_core()`
    /// to ensure signal-based state updates work correctly.
    pub fn new(bridge: EffectBridge, app_core: Arc<RwLock<AppCore>>) -> Self {
        Self {
            bridge: Arc::new(bridge),
            app_core,
            has_existing_account: true, // Default to true for backwards compatibility
            #[cfg(feature = "development")]
            demo_hints: None,
        }
    }

    /// Create a new IoContext with AppCore integration
    ///
    /// This is the production constructor that enables the full intent-based
    /// state management flow from aura-app. The bridge should be created with
    /// `EffectBridge::with_app_core()` to enable signal-based state updates.
    pub fn with_app_core(bridge: EffectBridge, app_core: Arc<RwLock<AppCore>>) -> Self {
        Self {
            bridge: Arc::new(bridge),
            app_core,
            has_existing_account: true, // Default to true for backwards compatibility
            #[cfg(feature = "development")]
            demo_hints: None,
        }
    }

    /// Create a new IoContext with explicit account existence flag
    ///
    /// Use this constructor when you need to control whether the account setup
    /// modal should be shown. Pass `has_existing_account: false` to show the modal.
    pub fn with_account_status(
        bridge: EffectBridge,
        app_core: Arc<RwLock<AppCore>>,
        has_existing_account: bool,
    ) -> Self {
        Self {
            bridge: Arc::new(bridge),
            app_core,
            has_existing_account,
            #[cfg(feature = "development")]
            demo_hints: None,
        }
    }

    /// Create a new IoContext for demo mode with hints
    ///
    /// This constructor includes demo hints that provide contextual guidance
    /// and pre-generated invite codes for Alice and Charlie.
    #[cfg(feature = "development")]
    pub fn with_demo_hints(
        bridge: EffectBridge,
        app_core: Arc<RwLock<AppCore>>,
        hints: crate::demo::DemoHints,
        has_existing_account: bool,
    ) -> Self {
        Self {
            bridge: Arc::new(bridge),
            app_core,
            has_existing_account,
            demo_hints: Some(hints),
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

    /// Get Charlie's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub fn demo_charlie_code(&self) -> String {
        self.demo_hints
            .as_ref()
            .map(|h| h.charlie_invite_code.clone())
            .unwrap_or_default()
    }

    /// Get Alice's invite code (empty without development feature)
    #[cfg(not(feature = "development"))]
    pub fn demo_alice_code(&self) -> String {
        String::new()
    }

    /// Get Charlie's invite code (empty without development feature)
    #[cfg(not(feature = "development"))]
    pub fn demo_charlie_code(&self) -> String {
        String::new()
    }

    /// Create with default bridge configuration (demo mode with AppCore)
    ///
    /// Creates an AppCore without an agent, which provides:
    /// - Full ViewState signal infrastructure
    /// - Local-only intent dispatch
    /// - No network/sync capabilities
    ///
    /// The bridge is automatically configured with the AppCore for signal-based updates.
    #[allow(clippy::expect_used)] // Panic on initialization failure is intentional
    pub fn with_defaults() -> Self {
        let app_core =
            AppCore::new(aura_app::AppConfig::default()).expect("Failed to create default AppCore");
        let app_core = Arc::new(RwLock::new(app_core));
        let bridge = EffectBridge::with_app_core(app_core.clone());
        Self {
            bridge: Arc::new(bridge),
            app_core,
            has_existing_account: true, // Defaults assume account exists
            #[cfg(feature = "development")]
            demo_hints: None,
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
    }

    /// Mark that an account has been created
    ///
    /// Called after the user completes the account setup modal.
    /// This updates the internal flag so `has_account()` returns true.
    pub fn set_account_created(&mut self) {
        self.has_existing_account = true;
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
                guardians: snapshot
                    .recovery
                    .guardians
                    .iter()
                    .map(|g| crate::tui::reactive::queries::Guardian {
                        authority_id: g.id.clone(),
                        name: g.name.clone(),
                        status: convert_guardian_status(&g.status),
                        added_at: g.added_at,
                        last_seen: g.last_seen,
                        share_index: None,
                    })
                    .collect(),
                threshold: Some(crate::tui::reactive::views::ThresholdConfig {
                    threshold: snapshot.recovery.threshold,
                    total: snapshot.recovery.guardian_count,
                }),
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
                status: convert_recovery_status(&snapshot.recovery),
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
                .map(|i| convert_invitation(i))
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
            // Map aura-app BlockState to TUI BlockSnapshot
            let block_info = if !snapshot.block.name.is_empty() {
                Some(crate::tui::reactive::views::BlockInfo {
                    id: snapshot.block.id.clone(),
                    name: Some(snapshot.block.name.clone()),
                    description: None,
                    created_at: 0, // Not tracked in aura-app BlockState
                })
            } else {
                None
            };

            // Map aura-app Resident (has role) to TUI Resident (has role enum)
            let convert_role = |role: &aura_app::views::block::ResidentRole| match role {
                aura_app::views::block::ResidentRole::Admin
                | aura_app::views::block::ResidentRole::Owner => {
                    crate::tui::reactive::views::ResidentRole::Steward
                }
                aura_app::views::block::ResidentRole::Resident => {
                    crate::tui::reactive::views::ResidentRole::Resident
                }
            };

            return BlockSnapshot {
                block: block_info,
                residents: snapshot
                    .block
                    .residents
                    .iter()
                    .map(|r| crate::tui::reactive::views::Resident {
                        authority_id: r.id.clone(),
                        name: r.name.clone(),
                        is_self: false, // Not tracked in aura-app Resident
                        is_online: r.is_online,
                        role: convert_role(&r.role),
                    })
                    .collect(),
                storage: crate::tui::reactive::views::StorageInfo {
                    used_bytes: snapshot.block.storage.used_bytes,
                    total_bytes: snapshot.block.storage.total_bytes,
                },
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
                contacts: snapshot
                    .contacts
                    .contacts
                    .iter()
                    .map(|c| crate::tui::reactive::views::Contact {
                        authority_id: c.id.clone(),
                        petname: c.petname.clone(),
                        suggested_name: c.suggested_name.clone(),
                        is_online: Some(c.is_online),
                        added_at: 0, // Not tracked in aura-app Contact
                        last_seen: c.last_interaction,
                        has_pending_suggestion: false, // Not tracked in aura-app Contact
                    })
                    .collect(),
                policy: crate::tui::reactive::views::SuggestionPolicy::default(),
            };
        }
        ContactsSnapshot::default()
    }

    /// Get a snapshot of neighborhood data
    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        // Try AppCore first
        if let Some(snapshot) = self.app_core_snapshot() {
            // Map aura-app NeighborhoodState to TUI NeighborhoodSnapshot
            let home_block_id = &snapshot.neighborhood.home_block_id;
            let current_block_id = snapshot
                .neighborhood
                .position
                .as_ref()
                .map(|p| p.current_block_id.clone());

            return NeighborhoodSnapshot {
                neighborhood_id: Some(snapshot.neighborhood.home_block_id.clone()),
                neighborhood_name: Some(snapshot.neighborhood.home_block_name.clone()),
                blocks: snapshot
                    .neighborhood
                    .neighbors
                    .iter()
                    .map(|b| crate::tui::reactive::views::NeighborhoodBlock {
                        id: b.id.clone(),
                        name: Some(b.name.clone()),
                        resident_count: b.resident_count.unwrap_or(0) as u8,
                        max_residents: 8, // Default max residents
                        is_home: &b.id == home_block_id,
                        can_enter: b.can_traverse,
                        is_current: current_block_id.as_ref() == Some(&b.id),
                    })
                    .collect(),
                position: crate::tui::reactive::views::TraversalPosition {
                    neighborhood_id: Some(snapshot.neighborhood.home_block_id.clone()),
                    block_id: current_block_id,
                    depth: crate::tui::reactive::views::TraversalDepth::Street, // Default depth
                    entered_at: 0, // Not tracked in aura-app
                },
            };
        }
        NeighborhoodSnapshot::default()
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
        snap.residents.iter().map(|r| r.into()).collect()
    }

    /// Get block budget as iocraft type
    pub fn get_block_budget(&self) -> BlockBudget {
        let snap = self.snapshot_block();
        let mut budget: BlockBudget = (&snap.storage).into();
        budget.resident_count = snap.residents.len() as u8;
        budget
    }

    // ─── Effect Dispatch ────────────────────────────────────────────────────

    /// Dispatch a command (fire and forget)
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch(command).await
    }

    /// Dispatch a command and wait for completion
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch_and_wait(command).await
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

    // ─── Event Subscription ─────────────────────────────────────────────────

    /// Subscribe to events with a filter
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        self.bridge.subscribe(filter)
    }

    /// Subscribe to all events
    pub fn subscribe_all(&self) -> EventSubscription {
        self.bridge.subscribe_all()
    }

    /// Emit an event (for testing)
    pub fn emit(&self, event: AuraEvent) {
        self.bridge.emit(event);
    }

    // ─── Connection Status ──────────────────────────────────────────────────

    /// Check if connected to the effect system
    pub async fn is_connected(&self) -> bool {
        self.bridge.is_connected().await
    }

    /// Get last error if any
    pub async fn last_error(&self) -> Option<String> {
        self.bridge.last_error().await
    }

    // ─── Sync Status ───────────────────────────────────────────────────────

    /// Check if a sync operation is currently in progress
    pub async fn is_syncing(&self) -> bool {
        self.bridge.is_syncing().await
    }

    /// Get the timestamp of the last successful sync (ms since epoch)
    pub async fn last_sync_time(&self) -> Option<u64> {
        self.bridge.last_sync_time().await
    }

    /// Get the number of known peers for sync operations
    pub async fn known_peers_count(&self) -> usize {
        self.bridge.known_peers_count().await
    }
}

impl Default for IoContext {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// =============================================================================
// Conversion Helpers (aura-app → TUI types)
// =============================================================================

/// Convert aura-app GuardianStatus to TUI GuardianStatus
fn convert_guardian_status(
    status: &aura_app::views::recovery::GuardianStatus,
) -> crate::tui::reactive::queries::GuardianStatus {
    match status {
        aura_app::views::recovery::GuardianStatus::Active => {
            crate::tui::reactive::queries::GuardianStatus::Active
        }
        aura_app::views::recovery::GuardianStatus::Pending => {
            crate::tui::reactive::queries::GuardianStatus::Pending
        }
        aura_app::views::recovery::GuardianStatus::Offline => {
            crate::tui::reactive::queries::GuardianStatus::Offline
        }
        aura_app::views::recovery::GuardianStatus::Revoked => {
            crate::tui::reactive::queries::GuardianStatus::Removed
        }
    }
}

/// Convert aura-app RecoveryState to TUI RecoveryStatus
fn convert_recovery_status(
    recovery: &aura_app::RecoveryState,
) -> crate::tui::reactive::queries::RecoveryStatus {
    use crate::tui::reactive::queries::{GuardianApproval, RecoveryState, RecoveryStatus};

    // Derive state from active_recovery process status
    let state = if let Some(process) = &recovery.active_recovery {
        match process.status {
            aura_app::views::recovery::RecoveryProcessStatus::Idle => RecoveryState::None,
            aura_app::views::recovery::RecoveryProcessStatus::Initiated => RecoveryState::Initiated,
            aura_app::views::recovery::RecoveryProcessStatus::WaitingForApprovals => {
                RecoveryState::Initiated
            }
            aura_app::views::recovery::RecoveryProcessStatus::Approved => {
                RecoveryState::ThresholdMet
            }
            aura_app::views::recovery::RecoveryProcessStatus::Completed => RecoveryState::Completed,
            aura_app::views::recovery::RecoveryProcessStatus::Failed => RecoveryState::Failed,
        }
    } else {
        RecoveryState::None
    };

    // Convert guardian approvals if there's an active recovery process
    let approvals: Vec<GuardianApproval> = if let Some(process) = &recovery.active_recovery {
        process
            .approved_by
            .iter()
            .map(|guardian_id| GuardianApproval {
                guardian_id: guardian_id.clone(),
                guardian_name: String::new(), // Name not stored in approval
                approved: true,
                timestamp: Some(process.initiated_at),
            })
            .collect()
    } else {
        Vec::new()
    };

    RecoveryStatus {
        session_id: recovery.active_recovery.as_ref().map(|p| p.id.clone()),
        state,
        approvals_received: recovery
            .active_recovery
            .as_ref()
            .map(|p| p.approvals_received)
            .unwrap_or(0),
        threshold: recovery.threshold,
        total_guardians: recovery.guardian_count,
        approvals,
        started_at: recovery.active_recovery.as_ref().map(|p| p.initiated_at),
        expires_at: recovery.active_recovery.as_ref().and_then(|p| p.expires_at),
        error: None,
    }
}

/// Convert aura-app Invitation to TUI Invitation
fn convert_invitation(
    invitation: &aura_app::views::invitations::Invitation,
) -> crate::tui::reactive::queries::Invitation {
    use crate::tui::reactive::queries::{InvitationDirection, InvitationStatus, InvitationType};

    let direction = match invitation.direction {
        aura_app::views::invitations::InvitationDirection::Sent => InvitationDirection::Outbound,
        aura_app::views::invitations::InvitationDirection::Received => InvitationDirection::Inbound,
    };

    let status = match invitation.status {
        aura_app::views::invitations::InvitationStatus::Pending => InvitationStatus::Pending,
        aura_app::views::invitations::InvitationStatus::Accepted => InvitationStatus::Accepted,
        aura_app::views::invitations::InvitationStatus::Rejected => InvitationStatus::Declined,
        aura_app::views::invitations::InvitationStatus::Expired => InvitationStatus::Expired,
        aura_app::views::invitations::InvitationStatus::Revoked => InvitationStatus::Cancelled,
    };

    let invitation_type = match invitation.invitation_type {
        aura_app::views::invitations::InvitationType::Guardian => InvitationType::Guardian,
        aura_app::views::invitations::InvitationType::Chat => InvitationType::Channel,
        aura_app::views::invitations::InvitationType::Block => InvitationType::Channel,
    };

    // Determine other party based on direction
    let (other_party_id, other_party_name) = match direction {
        InvitationDirection::Outbound => (
            invitation.to_id.clone().unwrap_or_default(),
            invitation.to_name.clone().unwrap_or_default(),
        ),
        InvitationDirection::Inbound => (invitation.from_id.clone(), invitation.from_name.clone()),
    };

    crate::tui::reactive::queries::Invitation {
        id: invitation.id.clone(),
        direction,
        other_party_id,
        other_party_name,
        invitation_type,
        status,
        created_at: invitation.created_at,
        expires_at: invitation.expires_at,
        message: invitation.message.clone(),
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
}
