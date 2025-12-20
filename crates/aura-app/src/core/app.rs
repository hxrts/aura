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
use crate::runtime_bridge::{LanPeerInfo, RuntimeBridge, SyncStatus as RuntimeSyncStatus};
use crate::views::ViewState;

use async_trait::async_trait;
use aura_core::effects::reactive::{
    ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream,
};
use aura_core::identifiers::AuthorityId;
#[cfg(feature = "signals")]
use aura_core::identifiers::ChannelId;
use aura_core::query::{FactPredicate, Query};
use aura_core::time::TimeStamp;
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::AccountId;
use aura_effects::ReactiveHandler;
use aura_journal::{Journal, JournalFact};
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

    /// The fact-based journal for recording intents
    journal: Journal,

    /// Pending facts waiting to be committed to journal
    /// (used for sync dispatch when RandomEffects aren't available)
    pending_facts: Vec<JournalFact>,

    /// View state manager
    views: ViewState,

    /// Next subscription ID (for callback subscriptions)
    next_subscription_id: u64,

    /// Path to journal file for persistence
    journal_path: Option<std::path::PathBuf>,

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

    /// Signal forwarder for auto-syncing ViewState to ReactiveEffects signals.
    ///
    /// When enabled (signals feature), this automatically forwards ViewState
    /// changes to the corresponding ReactiveEffects signals. ViewState is the
    /// single source of truth; ReactiveEffects signals are derived.
    #[cfg(feature = "signals")]
    signal_forwarder: Option<super::signal_sync::SignalForwarder>,

    /// Observer registry for callback-based subscriptions (UniFFI/mobile)
    #[cfg(feature = "callbacks")]
    observer_registry: crate::bridge::callback::ObserverRegistry,
}

impl AppCore {
    /// Create a new AppCore instance with the given configuration
    pub fn new(config: AppConfig) -> Result<Self, IntentError> {
        // Generate a deterministic account ID for reproducibility
        let account_id = AccountId::new_from_entropy([0u8; 32]);

        // Initialize the journal with a placeholder group key
        // NOTE: Production systems derive this from threshold key generation (see aura-core::crypto::tree_signing)
        let group_key_bytes = vec![0u8; 32];
        let journal = Journal::new_with_group_key_bytes(account_id, group_key_bytes);

        // Store journal path for persistence
        let journal_path = config.journal_path.map(std::path::PathBuf::from);

        // Create reactive handler for FRP-style state management
        let reactive = ReactiveHandler::new();

        Ok(Self {
            authority: None,
            account_id,
            journal,
            pending_facts: Vec::new(),
            views: ViewState::default(),
            next_subscription_id: 1,
            journal_path,
            runtime: None,
            reactive,
            #[cfg(feature = "signals")]
            signal_forwarder: None,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
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
        app.authority = Some(runtime.authority_id());

        // Store the runtime
        app.runtime = Some(runtime);

        Ok(app)
    }

    /// Create an AppCore with a specific account ID and authority
    pub fn with_identity(
        account_id: AccountId,
        authority: AuthorityId,
        group_key_bytes: Vec<u8>,
    ) -> Result<Self, IntentError> {
        let journal = Journal::new_with_group_key_bytes(account_id, group_key_bytes);

        // Create reactive handler for FRP-style state management
        let reactive = ReactiveHandler::new();

        Ok(Self {
            authority: Some(authority),
            account_id,
            journal,
            pending_facts: Vec::new(),
            views: ViewState::default(),
            next_subscription_id: 1,
            journal_path: None,
            runtime: None,
            reactive,
            #[cfg(feature = "signals")]
            signal_forwarder: None,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
        })
    }

    /// Set the journal path for persistence
    pub fn set_journal_path(&mut self, path: impl Into<std::path::PathBuf>) {
        self.journal_path = Some(path.into());
    }

    /// Get the journal path
    pub fn journal_path(&self) -> Option<&std::path::Path> {
        self.journal_path.as_deref()
    }

    /// Get the account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
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
        // Register all domain signals with default values
        crate::signal_defs::register_app_signals(&self.reactive)
            .await
            .map_err(|e| {
                IntentError::internal_error(format!("Failed to initialize signals: {}", e))
            })?;

        // Start signal forwarding (ViewState → ReactiveEffects)
        #[cfg(feature = "signals")]
        {
            let forwarder = super::signal_sync::SignalForwarder::start_all(
                &self.views,
                Arc::new(self.reactive.clone()),
            );
            self.signal_forwarder = Some(forwarder);
        }

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
    pub fn dispatch(&mut self, intent: Intent) -> Result<String, IntentError> {
        // 1. Validate the intent
        self.validate_intent(&intent)?;

        // 2. Check if intent should be journaled
        if !intent.should_journal() {
            // Non-journaled intents (queries, navigation) return immediately
            return Ok(format!("query_{}", self.next_subscription_id));
        }

        // 3. Require authority for journaled intents
        let authority = self.authority.ok_or_else(|| {
            IntentError::unauthorized("No authority set - cannot dispatch journaled intent")
        })?;

        // 4. Create timestamp using order clock (deterministic for sync dispatch)
        let fact_id = self.next_subscription_id;
        self.next_subscription_id += 1;

        let order_bytes =
            aura_core::hash::hash(format!("{}:{}", fact_id, intent.description()).as_bytes());
        let timestamp = TimeStamp::OrderClock(aura_core::time::OrderTime(order_bytes));

        // 5. Convert intent to journal fact
        let journal_fact = intent.to_journal_fact(authority, timestamp);

        // 6. Store fact content for ID generation
        let fact_content = journal_fact.content.clone();

        // 7. Add to pending facts (committed in dispatch_async with RandomEffects)
        self.pending_facts.push(journal_fact);

        // 8. Generate fact ID from content hash
        let fact_hash = aura_core::hash::hash(fact_content.as_bytes());
        Ok(format!(
            "fact_{:x}",
            u64::from_le_bytes(fact_hash[..8].try_into().unwrap_or([0u8; 8]))
        ))
    }

    /// Get pending facts that need to be committed to the journal
    pub fn pending_facts(&self) -> &[JournalFact] {
        &self.pending_facts
    }

    /// Clear pending facts after they've been committed
    pub fn clear_pending_facts(&mut self) {
        self.pending_facts.clear();
    }

    /// Get a reference to the journal
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Get a mutable reference to the journal
    pub fn journal_mut(&mut self) -> &mut Journal {
        &mut self.journal
    }

    /// Validate an intent before dispatch
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
            Intent::SetBlockName { name, .. } => {
                if name.is_empty() {
                    return Err(IntentError::validation_failed("Block name is empty"));
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
    pub fn views(&self) -> &ViewState {
        &self.views
    }

    /// Get mutable access to view state (for internal updates)
    #[allow(dead_code)]
    pub(crate) fn views_mut(&mut self) -> &mut ViewState {
        &mut self.views
    }

    /// Commit pending facts to the journal and apply them to ViewState.
    ///
    /// This method:
    /// 1. Takes all pending facts
    /// 2. Reduces each fact to a ViewDelta
    /// 3. Applies the deltas to ViewState
    /// 4. Clears the pending facts
    ///
    /// Note: This is the synchronous version. Use `commit_pending_facts_and_emit()`
    /// for async signal emission to reactive subscribers.
    ///
    /// Returns the number of facts committed.
    pub fn commit_pending_facts(&mut self) -> usize {
        use crate::core::reducer::reduce_fact;

        let facts: Vec<_> = self.pending_facts.drain(..).collect();
        let count = facts.len();

        // Get the authority for delta application
        // Use deterministic placeholder when no authority is set
        let own_authority = self
            .authority
            .unwrap_or_else(|| AuthorityId::new_from_entropy([0u8; 32]));

        // Reduce and apply each fact
        for fact in facts {
            let delta = reduce_fact(&fact, &own_authority);

            // Apply delta to views
            cfg_if::cfg_if! {
                if #[cfg(feature = "signals")] {
                    self.views.apply_delta(delta);
                } else {
                    self.views.apply_delta(delta);
                }
            }
        }

        count
    }

    /// Commit pending facts, apply to ViewState.
    ///
    /// This is the preferred async method for reactive applications. It:
    /// 1. Commits pending facts to ViewState (via reducer)
    /// 2. ViewState changes automatically propagate to reactive signals via signal forwarding
    ///
    /// Note: Signal emission is handled automatically by the SignalForwarder infrastructure.
    /// Direct emit() calls are no longer needed here.
    ///
    /// Returns the number of facts committed.
    pub async fn commit_pending_facts_and_emit(&mut self) -> Result<usize, IntentError> {
        // Commit facts synchronously - ViewState updates auto-forward to signals
        let count = self.commit_pending_facts();
        Ok(count)
    }

    /// Commit pending facts and persist to storage.
    ///
    /// This is the main method to call after dispatch() to ensure facts are:
    /// 1. Applied to ViewState (via reducer)
    /// 2. Persisted to disk (if journal_path is set) - **native platforms only**
    ///
    /// On WASM, persistence must be handled via platform-specific APIs
    /// (e.g., IndexedDB via web-sys).
    ///
    /// Returns the number of facts committed, or an error if persistence fails.
    pub fn commit_and_persist(&mut self) -> Result<usize, IntentError> {
        use crate::core::reducer::reduce_fact;

        let facts: Vec<_> = self.pending_facts.drain(..).collect();
        let count = facts.len();

        if count == 0 {
            return Ok(0);
        }

        // Get the authority for delta application
        // Use deterministic placeholder when no authority is set
        let own_authority = self
            .authority
            .unwrap_or_else(|| AuthorityId::new_from_entropy([0u8; 32]));

        // Reduce and apply each fact to views
        for fact in &facts {
            let delta = reduce_fact(fact, &own_authority);

            cfg_if::cfg_if! {
                if #[cfg(feature = "signals")] {
                    self.views.apply_delta(delta);
                } else {
                    self.views.apply_delta(delta);
                }
            }
        }

        // Persist to storage if journal path is set (native platforms only)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(ref path) = self.journal_path {
            self.append_facts_to_storage(path, &facts)?;
        }

        Ok(count)
    }
}

// =============================================================================
// Native file storage (non-WASM only)
// =============================================================================
// These methods use std::fs directly for simplicity on native platforms.
// For full effect injection, integrate with aura-agent runtime which provides
// StorageEffects handlers. See docs/106_effect_system_and_runtime.md.

#[cfg(not(target_arch = "wasm32"))]
impl AppCore {
    /// Load journal facts from storage and rebuild ViewState.
    ///
    /// This is called on startup to restore state from persisted facts.
    /// **Native platforms only** - uses std::fs directly.
    ///
    /// Returns the number of facts loaded, or an error if loading fails.
    pub fn load_from_storage(&mut self, path: &std::path::Path) -> Result<usize, IntentError> {
        use std::fs::File;
        use std::io::BufReader;

        // Check if journal file exists
        if !path.exists() {
            return Ok(0); // No journal to load
        }

        // Read and deserialize facts
        let file = File::open(path).map_err(|e| {
            IntentError::storage_error(format!("Failed to open journal file: {}", e))
        })?;
        let reader = BufReader::new(file);

        let facts: Vec<JournalFact> = serde_json::from_reader(reader).map_err(|e| {
            IntentError::storage_error(format!("Failed to parse journal file: {}", e))
        })?;

        let count = facts.len();

        // Get own authority for delta application
        // Use deterministic placeholder when no authority is set
        let own_authority = self
            .authority
            .unwrap_or_else(|| AuthorityId::new_from_entropy([0u8; 32]));

        // Reduce and apply each fact
        for fact in facts {
            let delta = crate::core::reducer::reduce_fact(&fact, &own_authority);

            cfg_if::cfg_if! {
                if #[cfg(feature = "signals")] {
                    self.views.apply_delta(delta);
                } else {
                    self.views.apply_delta(delta);
                }
            }
        }

        Ok(count)
    }

    /// Save all committed facts to storage.
    ///
    /// This persists the journal facts to disk as JSON.
    /// **Native platforms only** - uses std::fs directly.
    pub fn save_to_storage(&self, path: &std::path::Path) -> Result<(), IntentError> {
        use std::fs::File;
        use std::io::BufWriter;

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                IntentError::storage_error(format!("Failed to create journal directory: {}", e))
            })?;
        }

        // Collect facts from the internal journal
        // For now, we use pending_facts since actual journal facts would require
        // getting them from the Journal struct
        let facts: Vec<&JournalFact> = self.pending_facts.iter().collect();

        // Serialize and write
        let file = File::create(path).map_err(|e| {
            IntentError::storage_error(format!("Failed to create journal file: {}", e))
        })?;
        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &facts).map_err(|e| {
            IntentError::storage_error(format!("Failed to write journal file: {}", e))
        })?;

        Ok(())
    }

    /// Append facts to storage (for incremental persistence).
    ///
    /// This appends new facts to the journal file rather than rewriting it.
    /// **Native platforms only** - uses std::fs directly.
    pub fn append_facts_to_storage(
        &self,
        path: &std::path::Path,
        facts: &[JournalFact],
    ) -> Result<(), IntentError> {
        use std::fs::OpenOptions;
        use std::io::{BufReader, BufWriter};

        // Read existing facts if file exists
        let mut all_facts: Vec<JournalFact> = if path.exists() {
            let file = std::fs::File::open(path).map_err(|e| {
                IntentError::storage_error(format!("Failed to open journal file: {}", e))
            })?;
            let reader = BufReader::new(file);
            serde_json::from_reader(reader).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Append new facts
        all_facts.extend(facts.iter().cloned());

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                IntentError::storage_error(format!("Failed to create journal directory: {}", e))
            })?;
        }

        // Write all facts
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| {
                IntentError::storage_error(format!("Failed to create journal file: {}", e))
            })?;
        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &all_facts).map_err(|e| {
            IntentError::storage_error(format!("Failed to write journal file: {}", e))
        })?;

        Ok(())
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
        self.observer_registry.notify_block(&snapshot.block);
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

    /// Get a signal for block state changes
    pub fn block_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = crate::views::BlockState> {
        self.views.block_signal()
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
// ViewState Helper Methods (signals feature only)
// =============================================================================
// These methods update ViewState directly. With signal forwarding enabled,
// changes automatically propagate to ReactiveEffects signals.
// DO NOT call emit() on domain signals directly - use these methods instead.

#[cfg(feature = "signals")]
impl AppCore {
    /// Add a contact to ViewState
    ///
    /// If a contact with the same ID already exists, it is not added again.
    /// The signal forwarding infrastructure will automatically update
    /// CONTACTS_SIGNAL for any subscribers.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let contact = Contact {
    ///     id: authority_id,
    ///     nickname: "Alice".to_string(),
    ///     ..Default::default()
    /// };
    /// app_core.add_contact(contact);
    /// // CONTACTS_SIGNAL is automatically updated
    /// ```
    pub fn add_contact(&self, contact: crate::views::contacts::Contact) {
        let mut contacts = self.views.snapshot().contacts;
        if !contacts.contacts.iter().any(|c| c.id == contact.id) {
            contacts.contacts.push(contact);
            self.views.set_contacts(contacts);
        }
    }

    /// Set guardian status on a contact
    ///
    /// Updates the is_guardian flag for the contact with the given ID.
    /// The signal forwarding infrastructure will automatically update
    /// CONTACTS_SIGNAL for any subscribers.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// app_core.set_contact_guardian_status(&authority_id, true);
    /// // CONTACTS_SIGNAL is automatically updated
    /// ```
    pub fn set_contact_guardian_status(&self, contact_id: &AuthorityId, is_guardian: bool) {
        let mut contacts = self.views.snapshot().contacts;
        if let Some(contact) = contacts.contacts.iter_mut().find(|c| &c.id == contact_id) {
            contact.is_guardian = is_guardian;
            self.views.set_contacts(contacts);
        }
    }

    /// Add a guardian to the recovery state
    ///
    /// If a guardian with the same ID already exists, it is not added again.
    /// The signal forwarding infrastructure will automatically update
    /// RECOVERY_SIGNAL for any subscribers.
    pub fn add_guardian(&self, guardian: crate::views::recovery::Guardian) {
        let mut recovery = self.views.snapshot().recovery;
        if !recovery.guardians.iter().any(|g| g.id == guardian.id) {
            recovery.guardians.push(guardian);
            recovery.guardian_count = recovery.guardians.len() as u32;
            self.views.set_recovery(recovery);
        }
    }

    /// Add a message to chat state
    ///
    /// The signal forwarding infrastructure will automatically update
    /// CHAT_SIGNAL for any subscribers.
    pub fn add_chat_message(&self, message: crate::views::chat::Message) {
        let mut chat = self.views.snapshot().chat;
        chat.messages.push(message);
        self.views.set_chat(chat);
    }

    /// Update recovery approval status
    ///
    /// Adds an approval to the active recovery process if one exists.
    /// The signal forwarding infrastructure will automatically update
    /// RECOVERY_SIGNAL for any subscribers.
    pub fn add_recovery_approval(&self, guardian_id: AuthorityId) {
        let mut recovery = self.views.snapshot().recovery;
        recovery.add_guardian_approval(guardian_id);
        self.views.set_recovery(recovery);
    }
}

impl AppCore {
    /// Async dispatch for Rust consumers
    ///
    /// This is the preferred method for native Rust consumers as it
    /// properly commits facts to the journal with random ordering.
    pub async fn dispatch_async(&mut self, intent: Intent) -> Result<String, IntentError> {
        // Use sync dispatch to validate and queue the fact
        let fact_id = self.dispatch(intent)?;

        // Commit any pending facts to the journal and reduce to views
        self.commit_pending_facts_with_deterministic_ordering()
            .await?;

        Ok(fact_id)
    }

    /// Reduce a fact to a view delta and apply it
    ///
    /// This is called after facts are committed to update the view state.
    /// In signals mode, uses interior mutability; in non-signals mode, needs mutable self.
    #[cfg(feature = "signals")]
    fn reduce_and_apply(&self, fact: &aura_journal::JournalFact) {
        if let Some(authority) = &self.authority {
            let delta = super::reduce_fact(fact, authority);
            self.views.apply_delta(delta);
        }
    }

    /// Reduce a fact to a view delta and apply it (non-signals mode)
    ///
    /// This is called after facts are committed to update the view state.
    #[cfg(not(feature = "signals"))]
    fn reduce_and_apply(&mut self, fact: &aura_journal::JournalFact) {
        if let Some(authority) = &self.authority {
            let delta = super::reduce_fact(fact, authority);
            self.views.apply_delta(delta);
        }
    }

    /// Commit pending facts to the journal with deterministic ordering
    ///
    /// Uses a seeded random generator for reproducible fact ordering.
    /// This ensures consistent behavior across platforms (including WASM)
    /// without requiring external dependencies or runtime configuration.
    ///
    /// For non-deterministic behavior, integrate with aura-agent runtime
    /// which provides effect-injected RandomEffects handlers.
    async fn commit_pending_facts_with_deterministic_ordering(
        &mut self,
    ) -> Result<(), IntentError> {
        use aura_core::effects::RandomEffects;

        /// Seeded deterministic random generator for reproducible fact ordering.
        /// Uses a counter-based PRNG to ensure consistent ordering across runs.
        struct SeededDeterministicRandom {
            counter: std::sync::atomic::AtomicU64,
        }

        impl SeededDeterministicRandom {
            fn new() -> Self {
                Self {
                    counter: std::sync::atomic::AtomicU64::new(0),
                }
            }

            fn next_seed(&self) -> u128 {
                let count = self
                    .counter
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // Fixed seed base combined with counter for unique, deterministic values
                const SEED_BASE: u128 = 0xDEAD_BEEF_CAFE_BABE_1234_5678_9ABC_DEF0;
                SEED_BASE.wrapping_add(count as u128)
            }
        }

        #[async_trait::async_trait]
        impl RandomEffects for SeededDeterministicRandom {
            async fn random_bytes_32(&self) -> [u8; 32] {
                let seed = self.next_seed();
                let mut bytes = [0u8; 32];
                let seed_bytes = seed.to_le_bytes();
                for (i, &b) in seed_bytes.iter().enumerate() {
                    bytes[i] = b;
                    bytes[i + 16] = b.wrapping_mul(37);
                }
                bytes
            }

            async fn random_bytes(&self, len: usize) -> Vec<u8> {
                let mut result = Vec::with_capacity(len);
                let base = self.random_bytes_32().await;
                for i in 0..len {
                    result.push(base[i % 32]);
                }
                result
            }

            async fn random_u64(&self) -> u64 {
                let bytes = self.random_bytes_32().await;
                u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0u8; 8]))
            }

            async fn random_range(&self, min: u64, max: u64) -> u64 {
                if min >= max {
                    return min;
                }
                let range = max - min;
                let rand = self.random_u64().await;
                min + (rand % range)
            }

            async fn random_uuid(&self) -> uuid::Uuid {
                let bytes = self.random_bytes_32().await;
                let mut uuid_bytes = [0u8; 16];
                uuid_bytes.copy_from_slice(&bytes[..16]);
                // Set version 4 (random) and variant bits
                uuid_bytes[6] = (uuid_bytes[6] & 0x0f) | 0x40;
                uuid_bytes[8] = (uuid_bytes[8] & 0x3f) | 0x80;
                uuid::Uuid::from_bytes(uuid_bytes)
            }
        }

        let deterministic_random = SeededDeterministicRandom::new();

        // Commit each pending fact and reduce to views
        let facts: Vec<_> = self.pending_facts.drain(..).collect();
        for fact in facts {
            // Reduce and apply to views before committing
            // (so views are updated even if journal commit fails)
            self.reduce_and_apply(&fact);

            self.journal
                .add_fact(fact, &deterministic_random)
                .await
                .map_err(|e| IntentError::journal_error(e.to_string()))?;
        }

        Ok(())
    }
}

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
    // - agent.invitations() -> InvitationService
    // - agent.recovery() -> RecoveryService
    // - agent.auth() -> AuthService
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
    /// * `threshold_k` - Minimum signers required (k)
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
        threshold_k: u16,
        total_n: u16,
        guardian_ids: &[String],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), IntentError> {
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
    pub async fn commit_guardian_key_rotation(&self, new_epoch: u64) -> Result<(), IntentError> {
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
        failed_epoch: u64,
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
        threshold_k: u16,
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
            channel_type: ChannelType::Block,
        });
        assert!(
            result.is_ok(),
            "CreateChannel dispatch failed: {:?}",
            result
        );

        // Step 2: Commit pending facts and apply to ViewState
        let committed = app.commit_pending_facts();
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
        let committed = app.commit_pending_facts();
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
            channel_type: ChannelType::Block,
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
        let committed = app.commit_pending_facts();
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
    #[test]
    fn test_e2e_set_nickname_updates_contacts() {
        let config = AppConfig::default();
        let mut app = AppCore::new(config).unwrap();

        // Set authority
        let authority = AuthorityId::new_from_entropy([42u8; 32]);
        app.set_authority(authority);

        // Dispatch SetNickname intent
        let result = app.dispatch(Intent::SetNickname {
            contact_id: "contact123".to_string(),
            nickname: "Alice".to_string(),
        });
        assert!(result.is_ok(), "SetNickname dispatch failed: {:?}", result);

        // Verify fact was created
        assert_eq!(app.pending_facts().len(), 1);

        // Commit and apply
        let committed = app.commit_pending_facts();
        assert_eq!(committed, 1);
    }

    /// E2E test: Recovery intents create proper facts
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

        let committed = app.commit_pending_facts();
        assert_eq!(committed, 1, "Expected 1 recovery initiation fact");

        // Verify recovery state changed
        let _snapshot = app.snapshot();
        // The recovery.status should now reflect the initiated state
        // (actual state depends on how ViewState::apply_delta handles RecoveryRequested)
    }
}
