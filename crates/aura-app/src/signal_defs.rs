//! # Application Signal Definitions
//!
//! This module defines the typed signals for the application's reactive state.
//! These signals integrate with the `ReactiveEffects` trait from `aura-core`,
//! providing algebraic effect-based access to application state.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                         AppCore                             │
//! │                            │                                │
//! │                   ReactiveEffects impl                      │
//! │                            │                                │
//! │              ┌─────────────┼─────────────┐                  │
//! │              ▼             ▼             ▼                  │
//! │       CHAT_SIGNAL   RECOVERY_SIGNAL   OTHER_SIGNALS         │
//! │              │             │             │                  │
//! │              └─────────────┼─────────────┘                  │
//! │                            ▼                                │
//! │                      ViewState                              │
//! │                  (futures_signals)                          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_app::signal_defs::{CHAT_SIGNAL, RECOVERY_SIGNAL};
//! use aura_core::effects::ReactiveEffects;
//!
//! // Read current state
//! let chat_state = app.read(&CHAT_SIGNAL).await?;
//!
//! // Subscribe to changes
//! let mut stream = app.subscribe(&CHAT_SIGNAL);
//! while let Ok(state) = stream.recv().await {
//!     println!("Chat updated: {} channels", state.channels.len());
//! }
//! ```

use aura_core::effects::reactive::Signal;
use std::sync::LazyLock;

use crate::queries::{
    BlocksQuery, BoundSignal, ChatQuery, ContactsQuery, GuardiansQuery, InvitationsQuery,
    NeighborhoodQuery, RecoveryQuery,
};
use crate::budget::BlockFlowBudget;
use crate::views::{
    BlockState, BlocksState, ChatState, ContactsState, InvitationsState, NeighborhoodState,
    RecoveryState,
};

// ─────────────────────────────────────────────────────────────────────────────
// Application Signal Definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Signal for chat state (channels, messages, selected channel)
pub static CHAT_SIGNAL: LazyLock<Signal<ChatState>> = LazyLock::new(|| Signal::new("app:chat"));

/// Signal for recovery state (guardians, recovery status, threshold)
pub static RECOVERY_SIGNAL: LazyLock<Signal<RecoveryState>> =
    LazyLock::new(|| Signal::new("app:recovery"));

/// Signal for invitations state (sent/received invitations)
pub static INVITATIONS_SIGNAL: LazyLock<Signal<InvitationsState>> =
    LazyLock::new(|| Signal::new("app:invitations"));

/// Signal for contacts state (contacts, petnames, display names)
pub static CONTACTS_SIGNAL: LazyLock<Signal<ContactsState>> =
    LazyLock::new(|| Signal::new("app:contacts"));

/// Signal for current block state (backwards compatibility)
pub static BLOCK_SIGNAL: LazyLock<Signal<BlockState>> = LazyLock::new(|| Signal::new("app:block"));

/// Signal for multi-block state (all blocks the user has created/joined)
pub static BLOCKS_SIGNAL: LazyLock<Signal<BlocksState>> =
    LazyLock::new(|| Signal::new("app:blocks"));

/// Signal for neighborhood state (nearby peers, relay info)
pub static NEIGHBORHOOD_SIGNAL: LazyLock<Signal<NeighborhoodState>> =
    LazyLock::new(|| Signal::new("app:neighborhood"));

/// Signal for block storage budget (resident/neighborhood/pinned allocations)
pub static BUDGET_SIGNAL: LazyLock<Signal<BlockFlowBudget>> =
    LazyLock::new(|| Signal::new("app:budget"));

// ─────────────────────────────────────────────────────────────────────────────
// Query-Bound Signals
// ─────────────────────────────────────────────────────────────────────────────
//
// These signals are bound to queries and automatically update when underlying
// facts change. Use `create_bound_signals()` to instantiate them.

/// Create bound signal for contacts (updates when contact facts change)
pub fn create_contacts_bound() -> BoundSignal<ContactsQuery> {
    BoundSignal::with_name("app:contacts:bound", ContactsQuery::default())
}

/// Create bound signal for guardians (updates when guardian facts change)
pub fn create_guardians_bound() -> BoundSignal<GuardiansQuery> {
    BoundSignal::with_name("app:guardians:bound", GuardiansQuery::default())
}

/// Create bound signal for invitations (updates when invitation facts change)
pub fn create_invitations_bound() -> BoundSignal<InvitationsQuery> {
    BoundSignal::with_name("app:invitations:bound", InvitationsQuery::default())
}

/// Create bound signal for recovery state (updates when recovery facts change)
pub fn create_recovery_bound() -> BoundSignal<RecoveryQuery> {
    BoundSignal::with_name("app:recovery:bound", RecoveryQuery)
}

/// Create bound signal for chat state (updates when channel/message facts change)
pub fn create_chat_bound() -> BoundSignal<ChatQuery> {
    BoundSignal::with_name("app:chat:bound", ChatQuery::default())
}

/// Create bound signal for blocks state (updates when block facts change)
pub fn create_blocks_bound() -> BoundSignal<BlocksQuery> {
    BoundSignal::with_name("app:blocks:bound", BlocksQuery::default())
}

/// Create bound signal for neighborhood state (updates when neighbor facts change)
pub fn create_neighborhood_bound() -> BoundSignal<NeighborhoodQuery> {
    BoundSignal::with_name("app:neighborhood:bound", NeighborhoodQuery::default())
}

// ─────────────────────────────────────────────────────────────────────────────
// Derived Signal Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Signal for connection status (online/offline)
pub static CONNECTION_STATUS_SIGNAL: LazyLock<Signal<ConnectionStatus>> =
    LazyLock::new(|| Signal::new("app:connection_status"));

/// Signal for sync status (syncing/synced)
pub static SYNC_STATUS_SIGNAL: LazyLock<Signal<SyncStatus>> =
    LazyLock::new(|| Signal::new("app:sync_status"));

/// Signal for error notifications
pub static ERROR_SIGNAL: LazyLock<Signal<Option<AppError>>> =
    LazyLock::new(|| Signal::new("app:error"));

/// Signal for unread message count (derived from chat state)
pub static UNREAD_COUNT_SIGNAL: LazyLock<Signal<usize>> =
    LazyLock::new(|| Signal::new("app:unread_count"));

/// Signal for discovered peers (rendezvous + LAN)
pub static DISCOVERED_PEERS_SIGNAL: LazyLock<Signal<DiscoveredPeersState>> =
    LazyLock::new(|| Signal::new("app:discovered_peers"));

/// Signal for account settings and profile
pub static SETTINGS_SIGNAL: LazyLock<Signal<SettingsState>> =
    LazyLock::new(|| Signal::new("app:settings"));

// ─────────────────────────────────────────────────────────────────────────────
// Signal Value Types
// ─────────────────────────────────────────────────────────────────────────────

/// Connection status for the app
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    /// Not connected to any peers
    #[default]
    Offline,
    /// Attempting to connect
    Connecting,
    /// Connected to peers
    Online {
        /// Number of connected peers
        peer_count: usize,
    },
}

/// Sync status for the app
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SyncStatus {
    /// Not syncing
    #[default]
    Idle,
    /// Currently syncing
    Syncing {
        /// Progress percentage (0-100)
        progress: u8,
    },
    /// Sync completed
    Synced,
    /// Sync failed with error
    Failed {
        /// Error message
        message: String,
    },
}

/// Application error for error signal
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    /// Error code for categorization
    pub code: String,
    /// Human-readable message
    pub message: String,
    /// Whether the error is recoverable
    pub recoverable: bool,
}

impl AppError {
    /// Create a new application error
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable: true,
        }
    }

    /// Create a non-recoverable error
    pub fn fatal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable: false,
        }
    }
}

/// Discovered peer information
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// Authority ID of the peer
    pub authority_id: String,
    /// Network address (empty for rendezvous, IP:port for LAN)
    pub address: String,
    /// Discovery method ("rendezvous" or "LAN")
    pub method: String,
    /// Whether this peer has been invited already
    pub invited: bool,
}

/// State of discovered peers for the signal
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiscoveredPeersState {
    /// List of discovered peers
    pub peers: Vec<DiscoveredPeer>,
    /// Timestamp of last update (ms since epoch)
    pub last_updated_ms: u64,
}

/// Device information for settings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Device ID
    pub id: String,
    /// Device name/label
    pub name: String,
    /// Whether this is the current device
    pub is_current: bool,
    /// Last seen timestamp (ms since epoch)
    pub last_seen: Option<u64>,
}

/// Account settings and profile state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsState {
    /// Display name/nickname
    pub display_name: String,
    /// Threshold k (minimum signers required)
    pub threshold_k: u8,
    /// Threshold n (total guardians)
    pub threshold_n: u8,
    /// MFA policy setting
    pub mfa_policy: String,
    /// List of devices
    pub devices: Vec<DeviceInfo>,
    /// Number of contacts
    pub contact_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Signal Registration Helper
// ─────────────────────────────────────────────────────────────────────────────

use aura_core::effects::reactive::{ReactiveEffects, ReactiveError};

/// Register all application signals with a reactive handler.
///
/// This should be called during app initialization to set up the signal graph.
///
/// # Example
///
/// ```rust,ignore
/// use aura_effects::ReactiveHandler;
/// use aura_app::signal_defs::register_app_signals;
///
/// let handler = ReactiveHandler::new();
/// register_app_signals(&handler).await?;
/// ```
pub async fn register_app_signals<R: ReactiveEffects>(handler: &R) -> Result<(), ReactiveError> {
    // Register domain signals with default values
    handler
        .register(&*CHAT_SIGNAL, ChatState::default())
        .await?;
    handler
        .register(&*RECOVERY_SIGNAL, RecoveryState::default())
        .await?;
    handler
        .register(&*INVITATIONS_SIGNAL, InvitationsState::default())
        .await?;
    handler
        .register(&*CONTACTS_SIGNAL, ContactsState::default())
        .await?;
    handler
        .register(&*BLOCK_SIGNAL, BlockState::default())
        .await?;
    handler
        .register(&*BLOCKS_SIGNAL, BlocksState::default())
        .await?;
    handler
        .register(&*NEIGHBORHOOD_SIGNAL, NeighborhoodState::default())
        .await?;

    // Register derived/status signals
    handler
        .register(&*CONNECTION_STATUS_SIGNAL, ConnectionStatus::default())
        .await?;
    handler
        .register(&*SYNC_STATUS_SIGNAL, SyncStatus::default())
        .await?;
    handler.register(&*ERROR_SIGNAL, None).await?;
    handler.register(&*UNREAD_COUNT_SIGNAL, 0).await?;
    handler
        .register(&*DISCOVERED_PEERS_SIGNAL, DiscoveredPeersState::default())
        .await?;
    handler
        .register(&*SETTINGS_SIGNAL, SettingsState::default())
        .await?;

    Ok(())
}

/// Register application signals with query bindings for automatic updates.
///
/// Unlike `register_app_signals` which registers signals with static default values,
/// this function binds signals to queries. When facts matching the query's dependencies
/// change, the signals are automatically invalidated and re-evaluated.
///
/// # Example
///
/// ```rust,ignore
/// use aura_effects::ReactiveHandler;
/// use aura_app::signal_defs::register_app_signals_with_queries;
///
/// let handler = ReactiveHandler::new();
/// register_app_signals_with_queries(&handler).await?;
///
/// // Signals now automatically update when facts change
/// ```
pub async fn register_app_signals_with_queries<R: ReactiveEffects>(
    handler: &R,
) -> Result<(), ReactiveError> {
    use crate::queries::{
        BlocksQuery, ChatQuery, ContactsQuery, InvitationsQuery, NeighborhoodQuery, RecoveryQuery,
    };

    // Register domain signals bound to queries
    // When facts change, the queries re-evaluate and signals update

    // Chat signal - bound to ChatQuery for automatic channel/message updates
    handler
        .register_query(&*CHAT_SIGNAL, ChatQuery::default())
        .await?;

    // Recovery signal - bound to RecoveryQuery for threshold/guardian updates
    handler
        .register_query(&*RECOVERY_SIGNAL, RecoveryQuery)
        .await?;

    // Invitations signal - bound to InvitationsQuery for invitation list updates
    handler
        .register_query(&*INVITATIONS_SIGNAL, InvitationsQuery::default())
        .await?;

    // Contacts signal - bound to ContactsQuery for contact list updates
    handler
        .register_query(&*CONTACTS_SIGNAL, ContactsQuery::default())
        .await?;

    // Block signal - single block state (backwards compatibility)
    // Note: For multi-block state, use BLOCKS_SIGNAL with BlocksQuery
    handler
        .register(&*BLOCK_SIGNAL, BlockState::default())
        .await?;

    // Blocks signal - bound to BlocksQuery for multi-block updates
    handler
        .register_query(&*BLOCKS_SIGNAL, BlocksQuery::default())
        .await?;

    // Neighborhood signal - bound to NeighborhoodQuery for neighbor updates
    handler
        .register_query(&*NEIGHBORHOOD_SIGNAL, NeighborhoodQuery::default())
        .await?;

    // Register derived/status signals (not query-bound, updated manually)
    handler
        .register(&*CONNECTION_STATUS_SIGNAL, ConnectionStatus::default())
        .await?;
    handler
        .register(&*SYNC_STATUS_SIGNAL, SyncStatus::default())
        .await?;
    handler.register(&*ERROR_SIGNAL, None).await?;
    handler.register(&*UNREAD_COUNT_SIGNAL, 0).await?;

    Ok(())
}

/// Get all bound signals for the application.
///
/// Returns the pre-configured bound signals that can be used for
/// reactive state management. Each bound signal pairs a signal ID
/// with its source query for automatic invalidation.
///
/// # Example
///
/// ```rust,ignore
/// let bound_signals = get_bound_signals();
/// for signal in bound_signals.contacts {
///     println!("Contact signal: {:?}", signal.signal().id());
/// }
/// ```
pub struct BoundSignals {
    /// Contacts bound signal
    pub contacts: BoundSignal<ContactsQuery>,
    /// Guardians bound signal
    pub guardians: BoundSignal<GuardiansQuery>,
    /// Invitations bound signal
    pub invitations: BoundSignal<InvitationsQuery>,
    /// Recovery bound signal
    pub recovery: BoundSignal<RecoveryQuery>,
    /// Chat bound signal
    pub chat: BoundSignal<ChatQuery>,
    /// Blocks bound signal
    pub blocks: BoundSignal<BlocksQuery>,
    /// Neighborhood bound signal
    pub neighborhood: BoundSignal<NeighborhoodQuery>,
}

impl BoundSignals {
    /// Create a new set of bound signals with default queries.
    pub fn new() -> Self {
        Self {
            contacts: create_contacts_bound(),
            guardians: create_guardians_bound(),
            invitations: create_invitations_bound(),
            recovery: create_recovery_bound(),
            chat: create_chat_bound(),
            blocks: create_blocks_bound(),
            neighborhood: create_neighborhood_bound(),
        }
    }
}

impl Default for BoundSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_ids_are_unique() {
        // Verify all signal IDs are unique
        let ids = vec![
            CHAT_SIGNAL.id().to_string(),
            RECOVERY_SIGNAL.id().to_string(),
            INVITATIONS_SIGNAL.id().to_string(),
            CONTACTS_SIGNAL.id().to_string(),
            BLOCK_SIGNAL.id().to_string(),
            BLOCKS_SIGNAL.id().to_string(),
            NEIGHBORHOOD_SIGNAL.id().to_string(),
            CONNECTION_STATUS_SIGNAL.id().to_string(),
            SYNC_STATUS_SIGNAL.id().to_string(),
            ERROR_SIGNAL.id().to_string(),
            UNREAD_COUNT_SIGNAL.id().to_string(),
        ];

        let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
        assert_eq!(ids.len(), unique_count, "All signal IDs must be unique");
    }

    #[test]
    fn test_connection_status() {
        let status = ConnectionStatus::Online { peer_count: 3 };
        assert!(matches!(status, ConnectionStatus::Online { peer_count: 3 }));
    }

    #[test]
    fn test_sync_status() {
        let status = SyncStatus::Syncing { progress: 50 };
        assert!(matches!(status, SyncStatus::Syncing { progress: 50 }));
    }

    #[test]
    fn test_app_error() {
        let error = AppError::new("NETWORK_ERROR", "Connection failed");
        assert!(error.recoverable);

        let fatal = AppError::fatal("DATA_CORRUPTION", "Database corrupted");
        assert!(!fatal.recoverable);
    }
}
