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
use crate::views::ViewState;

use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use aura_core::AccountId;
use aura_journal::{Journal, JournalFact};
use serde::{Deserialize, Serialize};

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

    /// Observer registry for callback-based subscriptions (UniFFI/mobile)
    #[cfg(feature = "callbacks")]
    observer_registry: crate::bridge::callback::ObserverRegistry,
}

impl AppCore {
    /// Create a new AppCore instance with the given configuration
    pub fn new(_config: AppConfig) -> Result<Self, IntentError> {
        // Generate a deterministic account ID for reproducibility
        let account_id = AccountId::new_from_entropy([0u8; 32]);

        // Initialize the journal with a placeholder group key
        // NOTE: Production systems derive this from threshold key generation (see aura-core::crypto::tree_signing)
        let group_key_bytes = vec![0u8; 32];
        let journal = Journal::new_with_group_key_bytes(account_id, group_key_bytes);

        Ok(Self {
            authority: None,
            account_id,
            journal,
            pending_facts: Vec::new(),
            views: ViewState::default(),
            next_subscription_id: 1,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
        })
    }

    /// Create an AppCore with a specific account ID and authority
    pub fn with_identity(
        account_id: AccountId,
        authority: AuthorityId,
        group_key_bytes: Vec<u8>,
    ) -> Result<Self, IntentError> {
        let journal = Journal::new_with_group_key_bytes(account_id, group_key_bytes);

        Ok(Self {
            authority: Some(authority),
            account_id,
            journal,
            pending_facts: Vec::new(),
            views: ViewState::default(),
            next_subscription_id: 1,
            #[cfg(feature = "callbacks")]
            observer_registry: crate::bridge::callback::ObserverRegistry::new(),
        })
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
            Intent::SetPetname { petname, .. } => {
                if petname.is_empty() {
                    return Err(IntentError::validation_failed("Petname is empty"));
                }
                if petname.len() > 100 {
                    return Err(IntentError::validation_failed("Petname too long"));
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
    fn reduce_and_apply(&self, fact: &aura_journal::JournalFact) {
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
}
