//! # Application Signal Definitions
//!
//! This module defines the typed signals for the application's reactive state.
//! These signals integrate with the `ReactiveEffects` trait from `aura-core`,
//! providing algebraic effect-based access to application state.
//!
//! # Architecture

#![allow(missing_docs)] // Signal statics and state types are self-documenting
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

use aura_core::effects::query::QuerySignalEffects;
use aura_core::effects::reactive::Signal;
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use std::sync::LazyLock;

use crate::errors::AppError;
use crate::queries::{
    BoundSignal, ChatQuery, ContactsQuery, GuardiansQuery, HomesQuery, InvitationsQuery,
    NeighborhoodQuery, RecoveryQuery,
};
use crate::views::{
    ChatState, ContactsState, HomesState, InvitationsState, NeighborhoodState, RecoveryState,
};
use crate::workflows::budget::HomeFlowBudget;

// ─────────────────────────────────────────────────────────────────────────────
// Application Signal Definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Signal for chat state (channels, messages, selected channel)
pub const CHAT_SIGNAL_NAME: &str = "CHAT_SIGNAL";
pub static CHAT_SIGNAL: LazyLock<Signal<ChatState>> = LazyLock::new(|| Signal::new("app:chat"));

/// Signal for recovery state (guardians, recovery status, threshold)
pub const RECOVERY_SIGNAL_NAME: &str = "RECOVERY_SIGNAL";
pub static RECOVERY_SIGNAL: LazyLock<Signal<RecoveryState>> =
    LazyLock::new(|| Signal::new("app:recovery"));

/// Signal for invitations state (sent/received invitations)
pub const INVITATIONS_SIGNAL_NAME: &str = "INVITATIONS_SIGNAL";
pub static INVITATIONS_SIGNAL: LazyLock<Signal<InvitationsState>> =
    LazyLock::new(|| Signal::new("app:invitations"));

/// Signal for contacts state (contacts, nicknames, display names)
pub const CONTACTS_SIGNAL_NAME: &str = "CONTACTS_SIGNAL";
pub static CONTACTS_SIGNAL: LazyLock<Signal<ContactsState>> =
    LazyLock::new(|| Signal::new("app:contacts"));

/// Signal for multi-home state (all homes the user has created/joined)
pub const HOMES_SIGNAL_NAME: &str = "HOMES_SIGNAL";
pub static HOMES_SIGNAL: LazyLock<Signal<HomesState>> = LazyLock::new(|| Signal::new("app:homes"));

/// Signal for neighborhood state (nearby peers, relay info)
pub const NEIGHBORHOOD_SIGNAL_NAME: &str = "NEIGHBORHOOD_SIGNAL";
pub static NEIGHBORHOOD_SIGNAL: LazyLock<Signal<NeighborhoodState>> =
    LazyLock::new(|| Signal::new("app:neighborhood"));

/// Signal for home storage budget (member/neighborhood/pinned allocations)
pub const BUDGET_SIGNAL_NAME: &str = "BUDGET_SIGNAL";
pub static BUDGET_SIGNAL: LazyLock<Signal<HomeFlowBudget>> =
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

/// Create bound signal for homes state (updates when home facts change)
pub fn create_homes_bound() -> BoundSignal<HomesQuery> {
    BoundSignal::with_name("app:homes:bound", HomesQuery::default())
}

/// Create bound signal for neighborhood state (updates when neighbor facts change)
pub fn create_neighborhood_bound() -> BoundSignal<NeighborhoodQuery> {
    BoundSignal::with_name("app:neighborhood:bound", NeighborhoodQuery::default())
}

// ─────────────────────────────────────────────────────────────────────────────
// Derived Signal Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Signal for connection status (online/offline)
pub const CONNECTION_STATUS_SIGNAL_NAME: &str = "CONNECTION_STATUS_SIGNAL";
pub static CONNECTION_STATUS_SIGNAL: LazyLock<Signal<ConnectionStatus>> =
    LazyLock::new(|| Signal::new("app:connection_status"));

/// Signal for sync status (syncing/synced)
pub const SYNC_STATUS_SIGNAL_NAME: &str = "SYNC_STATUS_SIGNAL";
pub static SYNC_STATUS_SIGNAL: LazyLock<Signal<SyncStatus>> =
    LazyLock::new(|| Signal::new("app:sync_status"));

/// Signal for unified network status (combines transport and sync state)
pub const NETWORK_STATUS_SIGNAL_NAME: &str = "NETWORK_STATUS_SIGNAL";
pub static NETWORK_STATUS_SIGNAL: LazyLock<Signal<NetworkStatus>> =
    LazyLock::new(|| Signal::new("app:network_status"));

/// Signal for transport-level peer count (active channels/connections)
pub const TRANSPORT_PEERS_SIGNAL_NAME: &str = "TRANSPORT_PEERS_SIGNAL";
pub static TRANSPORT_PEERS_SIGNAL: LazyLock<Signal<usize>> =
    LazyLock::new(|| Signal::new("app:transport_peers"));

/// Signal for error notifications
pub const ERROR_SIGNAL_NAME: &str = "ERROR_SIGNAL";
pub static ERROR_SIGNAL: LazyLock<Signal<Option<AppError>>> =
    LazyLock::new(|| Signal::new("app:error"));

/// Signal for unread message count (derived from chat state)
pub const UNREAD_COUNT_SIGNAL_NAME: &str = "UNREAD_COUNT_SIGNAL";
pub static UNREAD_COUNT_SIGNAL: LazyLock<Signal<usize>> =
    LazyLock::new(|| Signal::new("app:unread_count"));

/// Signal for discovered peers (rendezvous + bootstrap candidates)
pub const DISCOVERED_PEERS_SIGNAL_NAME: &str = "DISCOVERED_PEERS_SIGNAL";
pub static DISCOVERED_PEERS_SIGNAL: LazyLock<Signal<DiscoveredPeersState>> =
    LazyLock::new(|| Signal::new("app:discovered_peers"));

/// Signal for account settings and profile
pub const SETTINGS_SIGNAL_NAME: &str = "SETTINGS_SIGNAL";
pub static SETTINGS_SIGNAL: LazyLock<Signal<SettingsState>> =
    LazyLock::new(|| Signal::new("app:settings"));

/// Signal for authoritative semantic lifecycle/readiness facts.
pub const AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME: &str = "AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL";
pub static AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL: LazyLock<
    Signal<crate::ui_contract::AuthoritativeSemanticFactsSnapshot>,
> = LazyLock::new(|| Signal::new("app:authoritative_semantic_facts"));

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

/// Unified network status combining transport and sync state.
///
/// This provides a single source of truth for the TUI footer status indicator,
/// combining transport connectivity and journal sync state into 4 user-facing states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkStatus {
    /// No transport connections at all
    #[default]
    Disconnected,

    /// Connected to transports but no peers found
    NoPeers,

    /// Connected with peers, journals catching up
    Syncing,

    /// Caught up with network, receiving real-time updates
    Synced {
        /// Last sync timestamp (ms since epoch)
        last_sync_ms: u64,
    },
}

/// Discovered peer information
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// Authority ID of the peer
    pub authority_id: AuthorityId,
    /// Network address (empty for rendezvous, bootstrap address when available)
    pub address: String,
    /// Discovery method
    pub method: DiscoveredPeerMethod,
    /// Whether this peer has been invited already
    pub invited: bool,
    /// Nickname the peer announced, when known (LAN/broker candidates).
    pub nickname_suggestion: Option<String>,
}

/// Discovery method for peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveredPeerMethod {
    Rendezvous,
    BootstrapCandidate,
}

impl DiscoveredPeerMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rendezvous => "rendezvous",
            Self::BootstrapCandidate => "bootstrap",
        }
    }
}

impl std::fmt::Display for DiscoveredPeerMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Cumulative LAN discovery counters for Observability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LanDiscoveryStats {
    /// Announcements this device broadcast.
    pub announcements_sent: u64,
    /// Discovery packets received.
    pub packets_received: u64,
    /// Received packets rejected as invalid or unauthenticated.
    pub packets_invalid: u64,
    /// Peers discovered from valid packets.
    pub peers_discovered: u64,
}

/// State of discovered peers for the signal
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiscoveredPeersState {
    /// List of discovered peers
    pub peers: Vec<DiscoveredPeer>,
    /// Timestamp of last update (ms since epoch)
    pub last_updated_ms: u64,
    /// LAN discovery counters, when LAN discovery runs.
    pub lan_stats: Option<LanDiscoveryStats>,
}

/// Device information for settings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Device ID
    pub id: DeviceId,
    /// Device name/label
    pub name: String,
    /// Whether this is the current device
    pub is_current: bool,
    /// Last seen timestamp (ms since epoch)
    pub last_seen: Option<u64>,
}

/// Authority information for settings and authority switching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityInfo {
    /// Authority identifier.
    pub id: AuthorityId,
    /// Best-effort display label.
    pub nickname_suggestion: String,
    /// Whether this is the active authority.
    pub is_current: bool,
}

/// Account settings and profile state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsState {
    /// Nickname suggestion (what the user wants to be called)
    pub nickname_suggestion: String,
    /// Threshold k (minimum signers required)
    pub threshold_k: u8,
    /// Threshold n (total guardians)
    pub threshold_n: u8,
    /// MFA policy setting
    pub mfa_policy: String,
    /// This device's consent policy for co-signing another device's request
    /// (device-local, never replicated).
    pub signing_consent: crate::runtime_bridge::DeviceSigningConsent,
    /// Quorum signing requests from other devices awaiting a decision here.
    pub pending_signing_requests: Vec<crate::runtime_bridge::PendingSigningRequest>,
    /// List of devices
    pub devices: Vec<DeviceInfo>,
    /// Number of contacts
    pub contact_count: usize,
    /// Current authority ID (hex string)
    pub authority_id: String,
    /// Authority nickname suggestion
    pub authority_nickname: String,
    /// Known authorities for this device/runtime
    pub authorities: Vec<AuthorityInfo>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Signal Registration Helper
// ─────────────────────────────────────────────────────────────────────────────

use aura_core::effects::reactive::{ReactiveEffects, ReactiveError};

/// Ensure all application signals exist without resetting live values.
///
/// This should be called during app initialization to set up the signal graph.
///
/// # Example
///
/// ```rust,ignore
/// use aura_app::ReactiveHandler;
/// use aura_app::signal_defs::register_app_signals;
///
/// let handler = ReactiveHandler::new();
/// register_app_signals(&handler).await?;
/// ```
pub async fn register_app_signals<R: ReactiveEffects>(handler: &R) -> Result<(), ReactiveError> {
    // Register domain signals with default values
    handler
        .ensure_registered(&*CHAT_SIGNAL, ChatState::default())
        .await?;
    handler
        .ensure_registered(&*RECOVERY_SIGNAL, RecoveryState::default())
        .await?;
    handler
        .ensure_registered(&*INVITATIONS_SIGNAL, InvitationsState::default())
        .await?;
    handler
        .ensure_registered(&*CONTACTS_SIGNAL, ContactsState::default())
        .await?;
    handler
        .ensure_registered(&*HOMES_SIGNAL, HomesState::default())
        .await?;
    handler
        .ensure_registered(&*NEIGHBORHOOD_SIGNAL, NeighborhoodState::default())
        .await?;

    // Register derived/status signals
    handler
        .ensure_registered(&*CONNECTION_STATUS_SIGNAL, ConnectionStatus::default())
        .await?;
    handler
        .ensure_registered(&*SYNC_STATUS_SIGNAL, SyncStatus::default())
        .await?;
    handler
        .ensure_registered(&*NETWORK_STATUS_SIGNAL, NetworkStatus::default())
        .await?;
    handler
        .ensure_registered(&*TRANSPORT_PEERS_SIGNAL, 0usize)
        .await?;
    handler.ensure_registered(&*ERROR_SIGNAL, None).await?;
    handler.ensure_registered(&*UNREAD_COUNT_SIGNAL, 0).await?;
    handler
        .ensure_registered(&*DISCOVERED_PEERS_SIGNAL, DiscoveredPeersState::default())
        .await?;
    handler
        .ensure_registered(&*SETTINGS_SIGNAL, SettingsState::default())
        .await?;
    handler
        .ensure_registered(
            &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
            crate::ui_contract::AuthoritativeSemanticFactsSnapshot::default(),
        )
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
/// use aura_app::ReactiveHandler;
/// use aura_app::signal_defs::register_app_signals_with_queries;
///
/// let handler = ReactiveHandler::new();
/// register_app_signals_with_queries(&handler).await?;
///
/// // Signals now automatically update when facts change
/// ```
pub async fn register_app_signals_with_queries<R: QuerySignalEffects>(
    handler: &R,
) -> Result<(), ReactiveError> {
    use crate::queries::{
        ChatQuery, ContactsQuery, HomesQuery, InvitationsQuery, NeighborhoodQuery, RecoveryQuery,
    };

    // Register domain signals bound to queries
    // When facts change, the queries re-evaluate and signals update

    // Chat signal - bound to ChatQuery for automatic channel/message updates
    handler
        .register_query_signal(&*CHAT_SIGNAL, ChatQuery::default())
        .await?;

    // Recovery signal - bound to RecoveryQuery for threshold/guardian updates
    handler
        .register_query_signal(&*RECOVERY_SIGNAL, RecoveryQuery)
        .await?;

    // Invitations signal - bound to InvitationsQuery for invitation list updates
    handler
        .register_query_signal(&*INVITATIONS_SIGNAL, InvitationsQuery::default())
        .await?;

    // Contacts signal - bound to ContactsQuery for contact list updates
    handler
        .register_query_signal(&*CONTACTS_SIGNAL, ContactsQuery::default())
        .await?;

    // Homes signal - bound to HomesQuery for multi-home updates
    handler
        .register_query_signal(&*HOMES_SIGNAL, HomesQuery::default())
        .await?;

    // Neighborhood signal - bound to NeighborhoodQuery for neighbor updates
    handler
        .register_query_signal(&*NEIGHBORHOOD_SIGNAL, NeighborhoodQuery::default())
        .await?;

    // Ensure derived/status signals (not query-bound, updated manually).
    handler
        .ensure_registered(&*CONNECTION_STATUS_SIGNAL, ConnectionStatus::default())
        .await?;
    handler
        .ensure_registered(&*SYNC_STATUS_SIGNAL, SyncStatus::default())
        .await?;
    handler
        .ensure_registered(&*NETWORK_STATUS_SIGNAL, NetworkStatus::default())
        .await?;
    handler
        .ensure_registered(&*TRANSPORT_PEERS_SIGNAL, 0usize)
        .await?;
    handler.ensure_registered(&*ERROR_SIGNAL, None).await?;
    handler.ensure_registered(&*UNREAD_COUNT_SIGNAL, 0).await?;
    handler
        .ensure_registered(&*DISCOVERED_PEERS_SIGNAL, DiscoveredPeersState::default())
        .await?;
    handler
        .ensure_registered(&*SETTINGS_SIGNAL, SettingsState::default())
        .await?;
    handler
        .ensure_registered(
            &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
            crate::ui_contract::AuthoritativeSemanticFactsSnapshot::default(),
        )
        .await?;

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
    /// Homes bound signal
    pub homes: BoundSignal<HomesQuery>,
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
            homes: create_homes_bound(),
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
    use aura_core::effects::reactive::SignalStream;
    use aura_core::query::{FactPredicate, Query};
    use aura_effects::ReactiveHandler;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailOneRegistration<'a> {
        inner: &'a ReactiveHandler,
        fail_at: usize,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ReactiveEffects for FailOneRegistration<'_> {
        async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
        where
            T: Clone + Send + Sync + 'static,
        {
            self.inner.read(signal).await
        }

        async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
        where
            T: Clone + Send + Sync + 'static,
        {
            self.inner.emit(signal, value).await
        }

        fn subscribe<T>(&self, signal: &Signal<T>) -> Result<SignalStream<T>, ReactiveError>
        where
            T: Clone + Send + Sync + 'static,
        {
            self.inner.subscribe(signal)
        }

        async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
        where
            T: Clone + Send + Sync + 'static,
        {
            self.inner.register(signal, initial).await
        }

        async fn ensure_registered<T>(
            &self,
            signal: &Signal<T>,
            initial: T,
        ) -> Result<(), ReactiveError>
        where
            T: Clone + Send + Sync + 'static,
        {
            if self.calls.fetch_add(1, Ordering::SeqCst) == self.fail_at {
                return Err(ReactiveError::Internal {
                    reason: format!("injected registration failure at {}", self.fail_at),
                });
            }
            self.inner.ensure_registered(signal, initial).await
        }

        fn is_registered(&self, signal_id: &aura_core::effects::reactive::SignalId) -> bool {
            self.inner.is_registered(signal_id)
        }

        async fn register_query<Q: Query>(
            &self,
            signal: &Signal<Q::Result>,
            query: Q,
        ) -> Result<(), ReactiveError> {
            self.inner.register_query(signal, query).await
        }

        fn query_dependencies(
            &self,
            signal_id: &aura_core::effects::reactive::SignalId,
        ) -> Option<Vec<FactPredicate>> {
            self.inner.query_dependencies(signal_id)
        }

        async fn invalidate_queries(&self, changed: &FactPredicate) {
            self.inner.invalidate_queries(changed).await;
        }
    }

    #[tokio::test]
    async fn register_app_signals_recovers_from_failure_at_every_step() {
        const REGISTRATION_STEPS: usize = 15;
        for fail_at in 0..REGISTRATION_STEPS {
            let handler = ReactiveHandler::new();
            handler.register(&*TRANSPORT_PEERS_SIGNAL, 0).await.unwrap();
            handler.emit(&*TRANSPORT_PEERS_SIGNAL, 41).await.unwrap();
            let faulty = FailOneRegistration {
                inner: &handler,
                fail_at,
                calls: AtomicUsize::new(0),
            };

            let error = register_app_signals(&faulty).await.unwrap_err();
            assert!(
                matches!(error, ReactiveError::Internal { ref reason } if reason == &format!("injected registration failure at {fail_at}")),
                "registration step {fail_at} did not return its injected failure"
            );
            assert_eq!(faulty.calls.load(Ordering::SeqCst), fail_at + 1);

            register_app_signals(&faulty).await.unwrap();
            assert_eq!(handler.stats().await.signal_count, REGISTRATION_STEPS);
            assert_eq!(handler.read(&*TRANSPORT_PEERS_SIGNAL).await.unwrap(), 41);
        }
    }

    #[tokio::test]
    async fn query_bound_registration_retries_after_binding_failure() {
        let handler = crate::effects::unified_handler::UnifiedHandler::new();
        handler.register(&*TRANSPORT_PEERS_SIGNAL, 0).await.unwrap();
        handler.emit(&*TRANSPORT_PEERS_SIGNAL, 41).await.unwrap();
        let first = register_app_signals_with_queries(&handler).await;
        assert!(matches!(first, Err(ReactiveError::Internal { .. })));
        assert!(handler.is_registered(CHAT_SIGNAL.id()));

        handler.allow_unrestricted_queries().await;
        register_app_signals_with_queries(&handler).await.unwrap();
        register_app_signals_with_queries(&handler).await.unwrap();

        assert_eq!(handler.reactive_handler().stats().await.signal_count, 15);
        assert_eq!(handler.read(&*TRANSPORT_PEERS_SIGNAL).await.unwrap(), 41);
        for signal_id in [
            CHAT_SIGNAL.id(),
            RECOVERY_SIGNAL.id(),
            INVITATIONS_SIGNAL.id(),
            CONTACTS_SIGNAL.id(),
            HOMES_SIGNAL.id(),
            NEIGHBORHOOD_SIGNAL.id(),
        ] {
            assert!(handler.query_dependencies_for(signal_id).is_some());
        }
        for signal_id in [
            CONNECTION_STATUS_SIGNAL.id(),
            SYNC_STATUS_SIGNAL.id(),
            NETWORK_STATUS_SIGNAL.id(),
            TRANSPORT_PEERS_SIGNAL.id(),
            ERROR_SIGNAL.id(),
            UNREAD_COUNT_SIGNAL.id(),
            DISCOVERED_PEERS_SIGNAL.id(),
            SETTINGS_SIGNAL.id(),
            AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL.id(),
        ] {
            assert!(handler.is_registered(signal_id));
        }
    }

    #[tokio::test]
    async fn register_app_signals_retry_preserves_existing_status_and_subscription() {
        let handler = ReactiveHandler::new();
        register_app_signals(&handler).await.unwrap();
        let mut stream = handler
            .subscribe_attached(&*TRANSPORT_PEERS_SIGNAL)
            .await
            .unwrap();
        handler.emit(&*TRANSPORT_PEERS_SIGNAL, 3).await.unwrap();

        register_app_signals(&handler).await.unwrap();

        assert_eq!(handler.read(&*TRANSPORT_PEERS_SIGNAL).await.unwrap(), 3);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.recv())
                .await
                .unwrap()
                .unwrap(),
            3
        );
        assert_eq!(handler.stats().await.signal_count, 15);
    }

    #[tokio::test]
    async fn register_app_signals_completes_partial_registration() {
        let handler = ReactiveHandler::new();
        handler
            .register(&*CHAT_SIGNAL, ChatState::default())
            .await
            .unwrap();
        assert_eq!(handler.stats().await.signal_count, 1);

        register_app_signals(&handler).await.unwrap();

        assert_eq!(handler.stats().await.signal_count, 15);
        assert!(handler.is_registered(TRANSPORT_PEERS_SIGNAL.id()));
        assert!(handler.is_registered(AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL.id()));
    }

    #[test]
    fn test_signal_ids_are_unique() {
        // Verify all signal IDs are unique
        let ids = vec![
            CHAT_SIGNAL.id().to_string(),
            RECOVERY_SIGNAL.id().to_string(),
            INVITATIONS_SIGNAL.id().to_string(),
            CONTACTS_SIGNAL.id().to_string(),
            HOMES_SIGNAL.id().to_string(),
            NEIGHBORHOOD_SIGNAL.id().to_string(),
            CONNECTION_STATUS_SIGNAL.id().to_string(),
            SYNC_STATUS_SIGNAL.id().to_string(),
            NETWORK_STATUS_SIGNAL.id().to_string(),
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
        use crate::errors::NetworkErrorCode;

        let error = AppError::network(NetworkErrorCode::ConnectionRefused, "Connection failed");
        assert!(error.is_recoverable());

        let fatal = AppError::internal("database", "Database corrupted");
        assert!(!fatal.is_recoverable());
    }
}
