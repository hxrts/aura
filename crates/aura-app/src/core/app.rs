//! # AppCore: The Portable Application Core
//!
//! This is the main entry point for the application. It manages:
//! - Intent dispatch and authorization
//! - View state management
//! - Query execution
//! - Reactive subscriptions
//!
//! ## Flow
//!
//! ```text
//! Intent → Authorize (Biscuit) → Journal → Reduce → View → Sync
//! ```

use super::{Intent, IntentError, StateSnapshot};
use crate::runtime_bridge::{
    BridgeDeviceInfo, LanPeerInfo, RuntimeBridge, SettingsBridgeState,
    SyncStatus as RuntimeSyncStatus,
};
use crate::views::ViewState;

use crate::ReactiveHandler;
use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::reactive::{
    ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream,
};
use aura_core::hash;
use aura_core::identifiers::AuthorityId;
#[cfg(feature = "signals")]
use aura_core::identifiers::ChannelId;
use aura_core::query::{FactPredicate, Query};
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::{Epoch, FrostThreshold};
use aura_core::AccountId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Configuration for creating an AppCore instance
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AppConfig {
    /// Path to the data directory
    pub data_dir: String,

    /// Whether to enable debug logging
    pub debug: bool,

    /// Optional custom journal path
    pub journal_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            data_dir: "./data".to_string(),
            debug: false,
            journal_path: None,
        }
    }
}

/// Unique identifier for a subscription (callbacks feature only)
#[cfg(feature = "callbacks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SubscriptionId {
    /// Internal ID
    pub id: u64,
}

/// The portable application core.
///
/// This struct provides the main API for interacting with Aura:
/// - Dispatch intents (user actions that become facts)
/// - Query current state
/// - Subscribe to state changes
///
/// ## Platform Usage
///
/// ### Native Rust / Terminal
/// ```rust,ignore
/// let app = AppCore::new(config)?;
/// let signal = app.chat_signal(); // futures-signals
/// app.dispatch(Intent::SendMessage { ... })?;
/// ```
///
/// ### iOS (Swift via UniFFI)
/// ```swift
/// let app = try AppCore(config: config)
/// app.subscribe(observer: myObserver)
/// try app.dispatch(.sendMessage(...))
/// ```
///
/// ### Android (Kotlin via UniFFI)
/// ```kotlin
/// val app = AppCore(config)
/// app.subscribe(observer)
/// app.dispatch(Intent.SendMessage(...))
/// ```
pub struct AppCore {
    /// The current authority (user identity)
    authority: Option<AuthorityId>,

    /// The account ID for this AppCore
    account_id: AccountId,

    /// View state manager
    views: ViewState,

    /// Optional RuntimeBridge for runtime operations (sync, signing, network)
    ///
    /// When present, enables:
    /// - Network sync operations
    /// - Threshold signing
    /// - Peer discovery and transport
    ///
    /// When absent (demo/offline mode):
    /// - Local-only state management
    /// - Intent dispatch still works (creates facts)
    /// - No network operations available
    runtime: Option<Arc<dyn RuntimeBridge>>,

    /// Reactive effect handler for FRP-style state management.
    ///
    /// This handler implements ReactiveEffects and manages the signal graph
    /// for reactive state updates. Use `init_signals()` to register application
    /// signals before using reactive operations.
    reactive: ReactiveHandler,

    /// Observer registry for callback-based subscriptions (UniFFI/mobile)
    #[cfg(feature = "callbacks")]
    observer_registry: crate::bridge::callback::ObserverRegistry,

    /// Whether the contacts refresh hook has been installed.
    contacts_refresh_hook_installed: bool,
}

impl AppCore {
    /// Create a new AppCore instance with the given configuration
    pub fn new(config: AppConfig) -> Result<Self, IntentError> {
        // Derive a deterministic account ID from the local config to avoid collisions.
        let config_seed = format!(
            "{}:{}",
            config.data_dir,
            config.journal_path.clone().unwrap_or_default()
        );
        let account_id = AccountId::from_bytes(hash::hash(config_seed.as_bytes()));

        // Create reactive handler for FRP-style state management
        let reactive = ReactiveHandler::new();

        let _ = config; // AppConfig is currently used by frontends; core stores no local journal state.

        Ok(Self {
            authority: None,
            account_id,
            views: ViewState::default(),
            runtime: None,
            reactive,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
            contacts_refresh_hook_installed: false,
        })
    }

    /// Create an AppCore with a RuntimeBridge for full runtime capabilities
    ///
    /// This constructor enables all runtime-backed operations:
    /// - Network sync and peer discovery
    /// - Threshold signing
    /// - Full distributed protocol support
    ///
    /// The runtime's authority ID is automatically set on the AppCore.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// let agent = AgentBuilder::new()
    ///     .with_config(agent_config)
    ///     .with_authority(authority_id)
    ///     .build_production()
    ///     .await?;
    /// let app = AppCore::with_runtime(config, agent.as_runtime_bridge())?;
    /// ```
    pub fn with_runtime(
        config: AppConfig,
        runtime: Arc<dyn RuntimeBridge>,
    ) -> Result<Self, IntentError> {
        let mut app = Self::new(config)?;

        // Set authority from runtime
        let authority_id = runtime.authority_id();
        app.authority = Some(authority_id);

        // Align account_id with the runtime authority to avoid constant IDs.
        app.account_id = AccountId::from_bytes(hash::hash(&authority_id.to_bytes()));

        // Share the runtime-owned reactive signal graph so scheduler-driven updates
        // are visible to the frontend via AppCore::read/subscribe.
        app.reactive = runtime.reactive_handler();

        // Store the runtime
        app.runtime = Some(runtime);

        Ok(app)
    }

    /// Create an AppCore with a specific account ID and authority
    pub fn with_identity(
        account_id: AccountId,
        authority: AuthorityId,
        _group_key_bytes: Vec<u8>,
    ) -> Result<Self, IntentError> {
        // Create reactive handler for FRP-style state management
        let reactive = ReactiveHandler::new();

        Ok(Self {
            authority: Some(authority),
            account_id,
            views: ViewState::default(),
            runtime: None,
            reactive,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
            contacts_refresh_hook_installed: false,
        })
    }

    /// Get the account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    pub(crate) fn mark_contacts_refresh_hook_installed(&mut self) -> bool {
        if self.contacts_refresh_hook_installed {
            false
        } else {
            self.contacts_refresh_hook_installed = true;
            true
        }
    }

    /// Set the authority (user identity) for this AppCore
    pub fn set_authority(&mut self, authority: AuthorityId) {
        self.authority = Some(authority);
    }

    /// Get the current authority (user identity), if set
    pub fn authority(&self) -> Option<&AuthorityId> {
        self.authority.as_ref()
    }

    /// Get a reference to the runtime bridge, if available
    ///
    /// The runtime provides access to:
    /// - Threshold signing operations
    /// - Sync status and peer discovery
    /// - Fact persistence
    pub fn runtime(&self) -> Option<&Arc<dyn RuntimeBridge>> {
        self.runtime.as_ref()
    }

    /// Check if a runtime is available for runtime operations
    ///
    /// Returns `true` if the AppCore was created with `with_runtime()`,
    /// enabling network sync, signing, and distributed protocols.
    ///
    /// Returns `false` for demo/offline mode (created with `new()`).
    pub fn has_runtime(&self) -> bool {
        self.runtime.is_some()
    }

    // ==================== Reactive Effects ====================

    /// Get a reference to the reactive handler.
    ///
    /// This provides direct access to the underlying ReactiveHandler for
    /// advanced reactive operations.
    pub fn reactive(&self) -> &ReactiveHandler {
        &self.reactive
    }

    /// Initialize all application signals with default values.
    ///
    /// This must be called before using reactive operations (read, emit, subscribe).
    /// Typically called once during app startup.
    ///
    /// Note: In `signals` builds, `init_signals()` also starts the internal
    /// ViewState → ReactiveEffects signal forwarder so `views().set_*()` updates
    /// become visible via `read()`/`subscribe()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let app = AppCore::new(config)?;
    /// app.init_signals().await?;
    ///
    /// // Now reactive operations work
    /// let chat = app.read(&CHAT_SIGNAL).await?;
    /// ```
    pub async fn init_signals(&mut self) -> Result<(), IntentError> {
        // Ensure the runtime has an initial threshold configuration if available,
        // even if signals were already registered by a shared runtime handler.
        if let Some(runtime) = self.runtime.as_ref() {
            if runtime.get_threshold_config().await.is_none() {
                let _ = runtime.bootstrap_signing_keys().await.map_err(|e| {
                    IntentError::internal_error(format!("Failed to bootstrap signing keys: {}", e))
                })?;
            }
        }

        // Idempotent init: if signals are already registered, don't re-register.
        let chat_id = (*crate::signal_defs::CHAT_SIGNAL).id();
        if self.reactive.is_registered(chat_id) {
            return Ok(());
        }

        // Register all domain signals with default values
        crate::signal_defs::register_app_signals(&self.reactive)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to initialize signals: {}", e))
            })?;

        Ok(())
    }

    /// Initialize signals and install runtime-backed hooks.
    pub async fn init_signals_with_hooks(
        app_core: &Arc<RwLock<AppCore>>,
    ) -> Result<(), IntentError> {
        {
            let mut core = app_core.write().await;
            core.init_signals().await?;
        }

        crate::workflows::system::install_contacts_refresh_hook(app_core)
            .await
            .map_err(|e| IntentError::internal_error(format!("Failed to install hooks: {e}")))?;

        Ok(())
    }

    // ==================== Threshold Signing Operations ====================

    /// Sign a tree operation and return an attested operation
    ///
    /// This method uses the RuntimeBridge to delegate threshold signing.
    /// The signature proves authorization from the threshold of signers
    /// configured for this authority.
    ///
    /// ## Requirements
    /// - A runtime must be available (`has_runtime()` returns `true`)
    /// - Signing keys must be bootstrapped (`bootstrap_signing_keys()`)
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// let tree_op = TreeOp { ... };
    /// let attested = app.sign_tree_op(&tree_op).await?;
    /// // attested.signature contains the threshold signature
    /// ```
    pub async fn sign_tree_op(&self, op: &TreeOp) -> Result<AttestedOp, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::unauthorized("No runtime available - cannot sign tree operations")
        })?;

        runtime.sign_tree_op(op).await
    }

    /// Bootstrap signing keys for the current authority
    ///
    /// This initializes the threshold signing infrastructure with 1-of-1 keys
    /// for single-device operation. For multi-device setups, additional
    /// devices would participate in a DKG ceremony to create m-of-n keys.
    ///
    /// ## Requirements
    /// - A runtime must be available (`has_runtime()` returns `true`)
    /// - An authority must be set
    ///
    /// ## Returns
    /// The public key package bytes that can be used to verify signatures
    /// from this authority.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// let public_key = app.bootstrap_signing_keys().await?;
    /// // Store or share public_key for signature verification
    /// ```
    pub async fn bootstrap_signing_keys(&self) -> Result<Vec<u8>, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::unauthorized("No runtime available - cannot bootstrap signing keys")
        })?;

        runtime.bootstrap_signing_keys().await
    }

    /// Get the threshold signing configuration for the current authority
    ///
    /// Returns `None` if signing keys haven't been bootstrapped yet.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// if let Some(config) = app.threshold_config().await {
    ///     println!("Threshold: {}-of-{}", config.threshold, config.total);
    /// }
    /// ```
    pub async fn threshold_config(&self) -> Option<aura_core::threshold::ThresholdConfig> {
        let runtime = self.runtime.as_ref()?;
        runtime.get_threshold_config().await
    }

    /// Check if this device has signing capability for the current authority
    ///
    /// Returns `true` if this device holds a key share and can participate
    /// in threshold signing operations.
    pub async fn has_signing_capability(&self) -> bool {
        let Some(runtime) = self.runtime.as_ref() else {
            return false;
        };
        runtime.has_signing_capability().await
    }

    /// Get the public key package for the current authority's signing keys
    ///
    /// Returns the group public key used for signature verification.
    /// This can be used to identify the authority's signing identity
    /// or to include in commitment tree leaf nodes.
    pub async fn threshold_signing_public_key(&self) -> Option<Vec<u8>> {
        let runtime = self.runtime.as_ref()?;
        runtime.get_public_key_package().await
    }

    // ==================== Intent Dispatch ====================

    /// Dispatch an intent (user action that becomes a fact)
    ///
    /// The intent flows through:
    /// 1. Validation - check intent structure and constraints
    /// 2. Authority check - require authority for journaled intents
    /// 3. Conversion to fact - transform intent to journal fact
    /// 4. Journal queue - add to pending facts
    /// 5. View reduction - applied in `dispatch_async()` after commit
    /// 6. Sync to peers - handled by transport layer (see aura-transport)
    ///
    /// Note: For intents that require journaling, use `dispatch_async()` instead
    /// as it provides proper random ordering. This synchronous version uses
    /// deterministic ordering based on intent content.
    ///
    /// Biscuit authorization is available when integrating with aura-agent runtime
    /// (see docs/109_authorization.md for details).
    #[cfg(any(feature = "app-internals", test))]
    pub fn dispatch(&mut self, intent: Intent) -> Result<String, IntentError> {
        self.validate_intent(&intent)?;
        Err(IntentError::service_error(
            "AppCore.dispatch no longer journals legacy string facts; use runtime-backed operations",
        ))
    }

    /// Validate an intent before dispatch
    #[allow(dead_code)]
    fn validate_intent(&self, intent: &Intent) -> Result<(), IntentError> {
        match intent {
            Intent::SendMessage { content, .. } => {
                if content.is_empty() {
                    return Err(IntentError::validation_failed("Message content is empty"));
                }
                if content.len() > 10000 {
                    return Err(IntentError::validation_failed("Message too long"));
                }
            }
            Intent::SetNickname { nickname, .. } => {
                if nickname.is_empty() {
                    return Err(IntentError::validation_failed("Nickname is empty"));
                }
                if nickname.len() > 100 {
                    return Err(IntentError::validation_failed("Nickname too long"));
                }
            }
            Intent::SetHomeName { name, .. } => {
                if name.is_empty() {
                    return Err(IntentError::validation_failed("Home name is empty"));
                }
            }
            Intent::GrantSteward { home_id, target_id } => {
                use aura_core::identifiers::AuthorityId;

                let snapshot = self.snapshot();
                let target = target_id.parse::<AuthorityId>().map_err(|_| {
                    IntentError::validation_failed(format!("Invalid authority ID: {}", target_id))
                })?;

                let home = snapshot
                    .homes
                    .homes
                    .values()
                    .find(|b| b.context_id == Some(*home_id))
                    .ok_or_else(|| IntentError::validation_failed("Home not found"))?;

                if !home.is_admin() {
                    return Err(IntentError::unauthorized(
                        "Only stewards can grant steward role",
                    ));
                }

                let Some(resident) = home.resident(&target) else {
                    return Err(IntentError::validation_failed(format!(
                        "Resident not found: {}",
                        target_id
                    )));
                };

                if matches!(resident.role, crate::views::ResidentRole::Owner) {
                    return Err(IntentError::validation_failed("Cannot modify Owner role"));
                }
            }
            Intent::RevokeSteward { home_id, target_id } => {
                use aura_core::identifiers::AuthorityId;

                let snapshot = self.snapshot();
                let target = target_id.parse::<AuthorityId>().map_err(|_| {
                    IntentError::validation_failed(format!("Invalid authority ID: {}", target_id))
                })?;

                let home = snapshot
                    .homes
                    .homes
                    .values()
                    .find(|b| b.context_id == Some(*home_id))
                    .ok_or_else(|| IntentError::validation_failed("Home not found"))?;

                if !home.is_admin() {
                    return Err(IntentError::unauthorized(
                        "Only stewards can revoke steward role",
                    ));
                }

                let Some(resident) = home.resident(&target) else {
                    return Err(IntentError::validation_failed(format!(
                        "Resident not found: {}",
                        target_id
                    )));
                };

                if !matches!(resident.role, crate::views::ResidentRole::Admin) {
                    return Err(IntentError::validation_failed(
                        "Can only revoke Admin role, not Owner or Resident",
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Get a snapshot of the current state
    ///
    /// This is useful for:
    /// - Initial state retrieval
    /// - Platforms that prefer polling
    /// - Debugging
    pub fn snapshot(&self) -> StateSnapshot {
        self.views.snapshot()
    }

    /// Get access to the view state for reactive subscriptions
    #[cfg(feature = "app-internals")]
    pub fn views(&self) -> &ViewState {
        &self.views
    }

    #[cfg(not(feature = "app-internals"))]
    pub(crate) fn views(&self) -> &ViewState {
        &self.views
    }

    /// Get mutable access to view state (for internal updates)
    #[allow(dead_code)]
    pub(crate) fn views_mut(&mut self) -> &mut ViewState {
        &mut self.views
    }
}

// =============================================================================
// Callback-based subscriptions (for UniFFI/mobile)
// =============================================================================

#[cfg(feature = "callbacks")]
impl AppCore {
    /// Subscribe to state changes via callbacks
    ///
    /// The observer will be called whenever state changes.
    /// Returns a subscription ID that can be used to unsubscribe.
    pub fn subscribe(
        &mut self,
        observer: std::sync::Arc<dyn crate::bridge::callback::StateObserver>,
    ) -> SubscriptionId {
        let id = self.observer_registry.add(observer);
        SubscriptionId { id }
    }

    /// Unsubscribe from state changes
    pub fn unsubscribe(&mut self, id: SubscriptionId) {
        self.observer_registry.remove(id.id);
    }

    /// Notify all observers of the current state
    ///
    /// Called internally after state changes to push updates to mobile clients.
    pub fn notify_observers(&self) {
        let snapshot = self.snapshot();
        self.observer_registry.notify_chat(&snapshot.chat);
        self.observer_registry.notify_recovery(&snapshot.recovery);
        self.observer_registry
            .notify_invitations(&snapshot.invitations);
        self.observer_registry.notify_contacts(&snapshot.contacts);
        self.observer_registry.notify_homes(&snapshot.homes);
        self.observer_registry
            .notify_neighborhood(&snapshot.neighborhood);
    }

    /// Get access to the observer registry (for testing)
    #[cfg(test)]
    pub fn observer_registry(&self) -> &crate::bridge::callback::ObserverRegistry {
        &self.observer_registry
    }
}

// =============================================================================
// Signal-based subscriptions (for native Rust/dominator)
// =============================================================================

#[cfg(feature = "signals")]
impl AppCore {
    /// Get a signal for chat state changes
    pub fn chat_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::ChatState> {
        self.views.chat_signal()
    }

    /// Get a signal for recovery state changes
    pub fn recovery_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::RecoveryState> {
        self.views.recovery_signal()
    }

    /// Get a signal for invitations state changes
    pub fn invitations_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::InvitationsState> {
        self.views.invitations_signal()
    }

    /// Get a signal for contacts state changes
    pub fn contacts_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::ContactsState> {
        self.views.contacts_signal()
    }

    /// Get a signal for neighborhood state changes
    pub fn neighborhood_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::NeighborhoodState> {
        self.views.neighborhood_signal()
    }

    /// Select a channel (UI-only, not journaled)
    ///
    /// This updates the selected channel in ChatState and triggers
    /// the chat signal for UI updates. Channel selection is a UI
    /// concern and doesn't need to be persisted to the journal.
    #[cfg(feature = "signals")]
    pub fn select_channel(&self, channel_id: Option<ChannelId>) {
        self.views.select_channel(channel_id);
    }
}

// =============================================================================
// ViewState Notes
// =============================================================================
// ViewState is now read-only for external consumers. UI updates flow through:
//
//   Facts → ReactiveScheduler → SignalViews → Signals (CONTACTS_SIGNAL, etc.)
//
// This ensures a single source of truth. Code that needs to update what the UI
// displays must either:
// 1. Commit facts through the runtime (production path)
// 2. Emit directly to signals via ReactiveEffects::emit() (demo/test path)
//
// The legacy ViewState mutation methods (add_contact, set_contact_guardian_status,
// add_guardian, add_chat_message, add_recovery_approval) have been removed because
// ViewState changes no longer propagate to signals (signal forwarding was removed
// in work/002.md C2.5).
//
// See work/reactive_unify.md and work/002.md for architectural history.

// Legacy: AppCore async dispatch + local pending-fact commit pipeline removed.
//
// The canonical pipeline is now: runtime typed fact commit → ReactiveScheduler → typed signals.

// =============================================================================
// Agent-backed operations (sync, services, network)
// =============================================================================
// These methods require a RuntimeBridge to be configured via `with_runtime()`.
// They provide high-level access to distributed protocol operations.

impl AppCore {
    // =========================================================================
    // Sync & Network Operations
    // =========================================================================

    /// Check if the sync service is running
    ///
    /// Returns `true` if the runtime has an active sync service.
    pub async fn is_sync_running(&self) -> bool {
        if let Some(runtime) = &self.runtime {
            return runtime.get_sync_status().await.is_running;
        }
        false
    }

    /// Get current sync status from the runtime (if available)
    pub async fn sync_status(&self) -> Option<RuntimeSyncStatus> {
        if let Some(runtime) = &self.runtime {
            return Some(runtime.get_sync_status().await);
        }
        None
    }

    /// Get settings + device list from the runtime (if available).
    pub async fn settings_snapshot(&self) -> Option<(SettingsBridgeState, Vec<BridgeDeviceInfo>)> {
        let runtime = self.runtime.as_ref()?;
        let settings = runtime.get_settings().await;
        let devices = runtime.list_devices().await;
        Some((settings, devices))
    }

    /// Get the list of known sync peers
    ///
    /// Returns device IDs of peers configured for sync.
    pub async fn sync_peers(&self) -> Result<Vec<aura_core::DeviceId>, IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("sync_peers requires a runtime"))?;

        Ok(runtime.get_sync_peers().await)
    }

    /// Discover peers via rendezvous
    ///
    /// Returns a list of discovered peer authority IDs from the rendezvous cache.
    pub async fn discover_peers(&self) -> Result<Vec<AuthorityId>, IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("discover_peers requires a runtime"))?;

        Ok(runtime.get_discovered_peers().await)
    }

    /// Get LAN-discovered peers
    ///
    /// Returns a list of peers discovered via LAN (mDNS/UDP broadcast) with their
    /// network addresses. Returns empty list if no runtime is available.
    pub async fn get_lan_peers(&self) -> Vec<LanPeerInfo> {
        if let Some(runtime) = &self.runtime {
            runtime.get_lan_peers().await
        } else {
            vec![]
        }
    }

    /// Check if the runtime is online (has active sync or rendezvous services)
    pub async fn is_online(&self) -> bool {
        if let Some(runtime) = &self.runtime {
            let status = runtime.get_status().await;
            // Check if either sync or rendezvous is running
            return status.sync.is_running || status.rendezvous.is_running;
        }
        false
    }

    /// Trigger a sync operation with connected peers
    ///
    /// This delegates to the runtime's sync service. If no runtime is configured
    /// (demo/offline mode), returns an error.
    ///
    /// # Errors
    ///
    /// Returns `IntentError::NoAgent` if no runtime is configured or sync
    /// service is not available.
    pub async fn trigger_sync(&self) -> Result<(), IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("trigger_sync requires a runtime"))?;

        runtime.trigger_sync().await
    }

    /// Sync with a specific peer by ID
    ///
    /// Initiates targeted synchronization with the specified peer.
    /// This is useful for requesting state updates from a known good peer.
    ///
    /// # Errors
    ///
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn sync_with_peer(&self, peer_id: &str) -> Result<(), IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("sync_with_peer requires a runtime"))?;

        runtime.sync_with_peer(peer_id).await
    }

    /// Export an invitation code for sharing
    ///
    /// Generates a shareable code that another user can use to establish
    /// a connection. Delegates to the runtime's invitation service.
    ///
    /// # Errors
    ///
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("export_invitation requires a runtime"))?;

        runtime.export_invitation(invitation_id).await
    }

    // Note: Invitation, Recovery, and Authentication service operations
    // have been removed from AppCore. Frontends that need these should
    // access the agent services directly via:
    // - agent.invitations() -> InvitationServiceApi
    // - agent.recovery() -> RecoveryServiceApi
    // - agent.auth() -> AuthServiceApi
    //
    // This maintains clean separation between:
    // - AppCore: Intent dispatch, ViewState, basic runtime status
    // - Agent services: Full distributed protocol operations

    /// Check if the runtime is authenticated
    pub async fn is_authenticated(&self) -> bool {
        if let Some(runtime) = &self.runtime {
            return runtime.is_authenticated().await;
        }
        false
    }

    // =========================================================================
    // Guardian Key Rotation Operations
    // =========================================================================

    /// Rotate guardian keys for a new threshold configuration
    ///
    /// This generates new FROST threshold keys for the given guardian configuration.
    /// The operation creates keys at a new epoch without invalidating the old keys
    /// until `commit_guardian_key_rotation` is called.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum signers required (k), must be >= 2 for FROST
    /// * `total_n` - Total number of guardians (n)
    /// * `guardian_ids` - IDs of contacts who will become guardians
    ///
    /// # Returns
    /// A tuple of (new_epoch, key_packages, public_key_package) on success
    ///
    /// # Errors
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn rotate_guardian_keys(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<(Epoch, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("rotate_guardian_keys requires a runtime"))?;

        runtime
            .rotate_guardian_keys(threshold_k, total_n, guardian_ids)
            .await
    }

    /// Commit a guardian key rotation after successful ceremony
    ///
    /// Called when all guardians have accepted and stored their key shares.
    /// This makes the new epoch authoritative.
    ///
    /// # Arguments
    /// * `new_epoch` - The epoch that should become active
    ///
    /// # Errors
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn commit_guardian_key_rotation(&self, new_epoch: Epoch) -> Result<(), IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("commit_guardian_key_rotation requires a runtime")
        })?;

        runtime.commit_guardian_key_rotation(new_epoch).await
    }

    /// Rollback a guardian key rotation after ceremony failure
    ///
    /// Called when the ceremony fails (guardian declined, user cancelled, or timeout).
    /// This discards the new epoch's keys and keeps the previous configuration active.
    ///
    /// # Arguments
    /// * `failed_epoch` - The epoch that should be discarded
    ///
    /// # Errors
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn rollback_guardian_key_rotation(
        &self,
        failed_epoch: Epoch,
    ) -> Result<(), IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("rollback_guardian_key_rotation requires a runtime")
        })?;

        runtime.rollback_guardian_key_rotation(failed_epoch).await
    }

    /// Initiate a guardian ceremony with full protocol fidelity
    ///
    /// This orchestrates the complete guardian ceremony:
    /// 1. Generates FROST threshold keys at a new epoch
    /// 2. Sends guardian invitations with key packages to each guardian
    /// 3. Returns a ceremony ID for tracking progress
    ///
    /// Guardians process invitations through their full runtimes and respond
    /// via the proper protocol. GuardianBinding facts are committed when
    /// threshold is reached.
    ///
    /// # Arguments
    /// * `threshold_k` - Minimum signers required (k)
    /// * `total_n` - Total number of guardians (n)
    /// * `guardian_ids` - IDs of contacts who will become guardians
    ///
    /// # Returns
    /// A ceremony ID for tracking progress
    ///
    /// # Errors
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn initiate_guardian_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<String, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("initiate_guardian_ceremony requires a runtime")
        })?;

        runtime
            .initiate_guardian_ceremony(threshold_k, total_n, guardian_ids)
            .await
    }

    /// Initiate a device threshold (multifactor) ceremony.
    pub async fn initiate_device_threshold_ceremony(
        &self,
        threshold_k: FrostThreshold,
        total_n: u16,
        device_ids: &[String],
    ) -> Result<String, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("initiate_device_threshold_ceremony requires a runtime")
        })?;

        runtime
            .initiate_device_threshold_ceremony(threshold_k, total_n, device_ids)
            .await
    }

    /// Initiate a device enrollment ("add device") ceremony.
    pub async fn initiate_device_enrollment_ceremony(
        &self,
        device_name: String,
    ) -> Result<crate::runtime_bridge::DeviceEnrollmentStart, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("initiate_device_enrollment_ceremony requires a runtime")
        })?;

        runtime
            .initiate_device_enrollment_ceremony(device_name)
            .await
    }

    /// Initiate a device removal ("remove device") ceremony.
    pub async fn initiate_device_removal_ceremony(
        &self,
        device_id: String,
    ) -> Result<String, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("initiate_device_removal_ceremony requires a runtime")
        })?;

        runtime.initiate_device_removal_ceremony(device_id).await
    }

    /// Get status of a guardian ceremony
    ///
    /// Returns the current state of the ceremony including:
    /// - Number of guardians who have accepted
    /// - Whether threshold has been reached
    /// - Whether ceremony is complete or failed
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony ID returned from initiate_guardian_ceremony
    ///
    /// # Returns
    /// CeremonyStatus with current state
    ///
    /// # Errors
    /// Returns `IntentError::NoAgent` if no runtime is configured.
    pub async fn get_ceremony_status(
        &self,
        ceremony_id: &str,
    ) -> Result<crate::runtime_bridge::CeremonyStatus, IntentError> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| IntentError::no_agent("get_ceremony_status requires a runtime"))?;

        runtime.get_ceremony_status(ceremony_id).await
    }

    /// Get status of a key rotation ceremony (generic form)
    pub async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &str,
    ) -> Result<crate::runtime_bridge::KeyRotationCeremonyStatus, IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("get_key_rotation_ceremony_status requires a runtime")
        })?;

        runtime.get_key_rotation_ceremony_status(ceremony_id).await
    }

    /// Cancel an in-progress key rotation ceremony (best effort)
    pub async fn cancel_key_rotation_ceremony(&self, ceremony_id: &str) -> Result<(), IntentError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            IntentError::no_agent("cancel_key_rotation_ceremony requires a runtime")
        })?;

        runtime.cancel_key_rotation_ceremony(ceremony_id).await
    }
}

// =============================================================================
// ReactiveEffects Implementation
// =============================================================================
// AppCore implements ReactiveEffects by delegating to its internal ReactiveHandler.
// This enables FRP-style state management through the algebraic effect system.

#[async_trait]
impl ReactiveEffects for AppCore {
    async fn read<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.read(signal).await
    }

    async fn emit<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.emit(signal, value).await
    }

    fn subscribe<T>(&self, signal: &Signal<T>) -> SignalStream<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.subscribe(signal)
    }

    async fn register<T>(&self, signal: &Signal<T>, initial: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.register(signal, initial).await
    }

    fn is_registered(&self, signal_id: &SignalId) -> bool {
        self.reactive.is_registered(signal_id)
    }

    async fn register_query<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError> {
        self.reactive.register_query(signal, query).await
    }

    fn query_dependencies(&self, signal_id: &SignalId) -> Option<Vec<FactPredicate>> {
        self.reactive.query_dependencies(signal_id)
    }

    async fn invalidate_queries(&self, changed: &FactPredicate) {
        self.reactive.invalidate_queries(changed).await
    }
}

// =============================================================================
// IntentEffects Implementation
// =============================================================================
// Note: IntentEffects implementation is planned but deferred until AppCore
// uses interior mutability. The trait method `dispatch(&self)` conflicts with
// the existing `dispatch(&mut self)` method. When we refactor AppCore to use
// RwLock/Mutex internally, we can implement IntentEffects properly.
//
// For now, use AppCore::dispatch() directly for intent dispatch.
// The IntentMetadata trait is implemented on Intent (in intent.rs) for
// authorization level checking.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert_eq!(config.data_dir, "./data");
        assert!(!config.debug);
    }

    #[test]
    fn test_app_core_creation() {
        let config = AppConfig::default();
        let app = AppCore::new(config);
        assert!(app.is_ok());
    }

    #[test]
    fn test_validate_empty_message() {
        let config = AppConfig::default();
        let mut app = AppCore::new(config).unwrap();

        let result = app.dispatch(Intent::SendMessage {
            channel_id: aura_core::identifiers::ContextId::new_from_entropy([0u8; 32]),
            content: "".to_string(),
            reply_to: None,
        });

        assert!(matches!(result, Err(IntentError::ValidationFailed { .. })));
    }

    #[test]
    fn test_snapshot_empty() {
        let config = AppConfig::default();
        let app = AppCore::new(config).unwrap();
        let snapshot = app.snapshot();
        assert!(snapshot.is_empty());
    }

    /// E2E test: Intent → dispatch → fact → reduction → ViewState update
    #[cfg(any())]
    #[test]
    fn test_e2e_create_channel_and_send_message() {
        use crate::core::intent::ChannelType;
        use aura_core::identifiers::ContextId;

        let config = AppConfig::default();
        let mut app = AppCore::new(config).unwrap();

        // Set authority (required for journaled intents)
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        app.set_authority(authority);

        // Verify initial state is empty
        let snapshot = app.snapshot();
        assert!(snapshot.chat.channels.is_empty());
        assert!(snapshot.chat.messages.is_empty());

        // Step 1: Create a channel
        let result = app.dispatch(Intent::CreateChannel {
            name: "test-channel".to_string(),
            channel_type: ChannelType::Home,
        });
        assert!(
            result.is_ok(),
            "CreateChannel dispatch failed: {:?}",
            result
        );

        // Step 2: Commit pending facts and apply to ViewState
        let committed = app.commit_pending_facts().unwrap();
        assert_eq!(committed, 1, "Expected 1 fact to be committed");

        // Step 3: Verify channel appeared in ViewState
        let snapshot = app.snapshot();
        assert_eq!(snapshot.chat.channels.len(), 1, "Expected 1 channel");
        assert_eq!(snapshot.chat.channels[0].name, "test-channel");

        // Step 4: Get the channel ID (generated from the fact content hash)
        let _channel_id = snapshot.chat.channels[0].id;

        // Step 5: Send a message to the channel
        // Note: Messages only appear in the messages vec when channel is selected,
        // but they do update channel metadata (last_message)
        let channel_ctx = ContextId::new_from_entropy([1u8; 32]);
        let result = app.dispatch(Intent::SendMessage {
            channel_id: channel_ctx,
            content: "Hello, World!".to_string(),
            reply_to: None,
        });
        assert!(result.is_ok(), "SendMessage dispatch failed: {:?}", result);

        // Step 6: Commit the message fact
        let committed = app.commit_pending_facts().unwrap();
        assert_eq!(committed, 1, "Expected 1 message fact to be committed");

        // Step 7: Verify channel metadata was updated
        // Note: The message won't appear in channel[0].last_message because
        // apply_message only updates channels that exist in the state with matching ID.
        // In this test, the message's channel_id (from ContextId) doesn't match
        // the channel's ID (from CreateChannel fact hash). This is expected behavior.
        // The test verifies that the fact pipeline works correctly.
        let snapshot = app.snapshot();
        assert_eq!(
            snapshot.chat.channels.len(),
            1,
            "Channel should still exist"
        );
    }

    /// E2E test: Verify full fact pipeline (create → commit → verify)
    #[cfg(any())]
    #[test]
    fn test_e2e_fact_pipeline_complete() {
        use crate::core::intent::ChannelType;

        let config = AppConfig::default();
        let mut app = AppCore::new(config).unwrap();

        // Set authority
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        app.set_authority(authority);

        // Dispatch multiple intents
        app.dispatch(Intent::CreateChannel {
            name: "channel-1".to_string(),
            channel_type: ChannelType::Home,
        })
        .unwrap();

        app.dispatch(Intent::CreateChannel {
            name: "channel-2".to_string(),
            channel_type: ChannelType::DirectMessage,
        })
        .unwrap();

        // Verify pending facts
        assert_eq!(app.pending_facts().len(), 2, "Should have 2 pending facts");

        // Commit all
        let committed = app.commit_pending_facts().unwrap();
        assert_eq!(committed, 2, "Should commit 2 facts");
        assert!(app.pending_facts().is_empty(), "Pending should be cleared");

        // Verify ViewState updated
        let snapshot = app.snapshot();
        assert_eq!(snapshot.chat.channels.len(), 2, "Should have 2 channels");

        // Verify channel types
        let channel_names: Vec<_> = snapshot.chat.channels.iter().map(|c| &c.name).collect();
        assert!(channel_names.contains(&&"channel-1".to_string()));
        assert!(channel_names.contains(&&"channel-2".to_string()));
    }

    /// E2E test: SetNickname intent creates fact and updates contacts
    #[cfg(any())]
    #[test]
    fn test_e2e_set_nickname_updates_contacts() {
        let config = AppConfig::default();
        let mut app = AppCore::new(config).unwrap();

        // Set authority
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        app.set_authority(authority);

        // Dispatch SetNickname intent
        let contact_id = AuthorityId::new_from_entropy([7u8; 32]).to_string();
        let result = app.dispatch(Intent::SetNickname {
            contact_id,
            nickname: "Alice".to_string(),
        });
        assert!(result.is_ok(), "SetNickname dispatch failed: {:?}", result);

        // Verify fact was created
        assert_eq!(app.pending_facts().len(), 1);

        // Commit and apply
        let committed = app.commit_pending_facts().unwrap();
        assert_eq!(committed, 1);
    }

    /// E2E test: Recovery intents create proper facts
    #[cfg(any())]
    #[test]
    fn test_e2e_recovery_flow_creates_facts() {
        let config = AppConfig::default();
        let mut app = AppCore::new(config).unwrap();

        // Set authority
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        app.set_authority(authority);

        // Step 1: Initiate recovery
        let result = app.dispatch(Intent::InitiateRecovery);
        assert!(
            result.is_ok(),
            "InitiateRecovery dispatch failed: {:?}",
            result
        );

        let committed = app.commit_pending_facts().unwrap();
        assert_eq!(committed, 1, "Expected 1 recovery initiation fact");

        // Verify recovery state changed
        let _snapshot = app.snapshot();
        // The recovery.status should now reflect the initiated state
        // (actual state depends on how ViewState::apply_delta handles RecoveryRequested)
    }
}
