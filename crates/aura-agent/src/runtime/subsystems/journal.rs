//! Journal Subsystem
//!
//! Groups journal-related fields from AuraEffectSystem:
//! - `indexed_journal`: Efficient fact lookups (B-tree, Bloom, Merkle)
//! - `fact_registry`: Domain fact registry for extensibility
//! - `fact_publish_tx`: Reactive scheduler publication channel
//! - `journal_policy`: Biscuit authorization for journal operations
//! - `journal_verifying_key`: Verification key for journal signatures
//!
//! ## Lock Usage
//!
//! Uses `parking_lot::Mutex` for `fact_publish_tx` because:
//! - Channel sender access is synchronous and brief (clone operation)
//! - Never held across async boundaries
//! - See `runtime/CONCURRENCY.md` for full rationale

#![allow(clippy::disallowed_types)]

use crate::database::IndexedJournalHandler;
use crate::reactive::{FactSource, ViewUpdate};
use aura_authorization::BiscuitAuthorizationBridge;
use aura_journal::extensibility::FactRegistry;
use biscuit_auth::Biscuit;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex as AsyncMutex};

/// Journal subsystem grouping fact storage and publication.
///
/// This subsystem encapsulates:
/// - Indexed journal for efficient fact lookups
/// - Fact registry for domain extensibility
/// - Publication channel for reactive updates
/// - Authorization policy for journal operations
struct JournalSubsystemShared {
    fact_publish_tx: Mutex<Option<mpsc::Sender<FactSource>>>,
    view_update_tx: Mutex<Option<broadcast::Sender<ViewUpdate>>>,
    indexed_keys: Arc<Mutex<HashSet<String>>>,
    cached_journal: Arc<AsyncMutex<Option<aura_core::Journal>>>,
}

pub struct JournalSubsystem {
    /// Indexed journal handler for efficient fact lookups
    ///
    /// Provides B-tree indices, Bloom filters, and Merkle proofs.
    indexed_journal: Arc<IndexedJournalHandler>,

    /// Domain fact registry for extensibility
    ///
    /// Maps fact types to reducers and validators.
    fact_registry: Arc<FactRegistry>,

    /// Biscuit authorization policy for journal operations
    ///
    /// Tuple of (token, bridge) for capability-based authorization.
    journal_policy: Option<(Biscuit, BiscuitAuthorizationBridge)>,

    /// Verification key for journal signatures
    journal_verifying_key: Option<Vec<u8>>,
    /// Shared runtime mutation boundary for publication and indexing state.
    shared: Arc<JournalSubsystemShared>,
}

impl JournalSubsystem {
    /// Create a new journal subsystem with the given capacity
    #[allow(dead_code)]
    pub fn new(capacity: u64, fact_registry: Arc<FactRegistry>) -> Self {
        Self {
            indexed_journal: Arc::new(IndexedJournalHandler::with_capacity(capacity)),
            fact_registry,
            journal_policy: None,
            journal_verifying_key: None,
            shared: Arc::new(JournalSubsystemShared {
                fact_publish_tx: Mutex::new(None),
                view_update_tx: Mutex::new(None),
                indexed_keys: Arc::new(Mutex::new(HashSet::new())),
                cached_journal: Arc::new(AsyncMutex::new(None)),
            }),
        }
    }

    /// Create from existing components
    pub fn from_parts(
        indexed_journal: Arc<IndexedJournalHandler>,
        fact_registry: Arc<FactRegistry>,
        fact_publish_tx: Option<mpsc::Sender<FactSource>>,
        journal_policy: Option<(Biscuit, BiscuitAuthorizationBridge)>,
        journal_verifying_key: Option<Vec<u8>>,
    ) -> Self {
        Self {
            indexed_journal,
            fact_registry,
            journal_policy,
            journal_verifying_key,
            shared: Arc::new(JournalSubsystemShared {
                fact_publish_tx: Mutex::new(fact_publish_tx),
                view_update_tx: Mutex::new(None),
                indexed_keys: Arc::new(Mutex::new(HashSet::new())),
                cached_journal: Arc::new(AsyncMutex::new(None)),
            }),
        }
    }

    /// Get the indexed journal handler
    pub fn indexed_journal(&self) -> Arc<IndexedJournalHandler> {
        self.indexed_journal.clone()
    }

    /// Load the journal into the shared runtime cache if needed, then return it.
    pub async fn get_or_load_journal<F, Fut>(
        &self,
        load: F,
    ) -> Result<aura_core::Journal, aura_core::AuraError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<aura_core::Journal, aura_core::AuraError>>,
    {
        let mut cache = self.shared.cached_journal.lock().await;
        if let Some(journal) = cache.as_ref() {
            return Ok(journal.clone());
        }

        let journal = load().await?;
        *cache = Some(journal.clone());
        Ok(journal)
    }

    /// Replace the shared runtime journal cache with the latest persisted state.
    pub async fn update_cached_journal(&self, journal: &aura_core::Journal) {
        *self.shared.cached_journal.lock().await = Some(journal.clone());
    }

    /// Index only journal facts that have not been mirrored yet.
    pub fn index_new_journal_facts(
        &self,
        journal: &aura_core::Journal,
        authority: Option<aura_core::AuthorityId>,
        timestamp: Option<aura_core::time::TimeStamp>,
    ) -> usize {
        let mut indexed_keys = self.shared.indexed_keys.lock();
        let mut added = 0usize;

        for (key, value) in journal.facts.iter() {
            let key_owned = key.as_str().to_string();
            if !indexed_keys.insert(key_owned.clone()) {
                continue;
            }

            self.indexed_journal
                .add_fact(key_owned, value.clone(), authority, timestamp.clone());
            added += 1;
        }

        added
    }

    /// Get the fact registry
    pub fn fact_registry(&self) -> Arc<FactRegistry> {
        self.fact_registry.clone()
    }

    /// Get the journal policy (if set)
    pub fn journal_policy(&self) -> Option<&(Biscuit, BiscuitAuthorizationBridge)> {
        self.journal_policy.as_ref()
    }

    /// Get the journal verifying key (if set)
    pub fn journal_verifying_key(&self) -> Option<&[u8]> {
        self.journal_verifying_key.as_deref()
    }

    /// Set the journal policy
    #[allow(dead_code)]
    pub fn set_journal_policy(&mut self, policy: (Biscuit, BiscuitAuthorizationBridge)) {
        self.journal_policy = Some(policy);
    }

    /// Set the journal verifying key
    #[allow(dead_code)]
    pub fn set_journal_verifying_key(&mut self, key: Vec<u8>) {
        self.journal_verifying_key = Some(key);
    }

    /// Attach a fact sink for reactive scheduling
    ///
    /// Facts committed to the journal will be published to this channel
    /// for processing by the reactive scheduler.
    pub fn attach_fact_sink(&self, tx: mpsc::Sender<FactSource>) {
        *self.shared.fact_publish_tx.lock() = Some(tx);
    }

    /// Detach the fact sink
    #[allow(dead_code)]
    pub fn detach_fact_sink(&self) {
        *self.shared.fact_publish_tx.lock() = None;
    }

    /// Check if a fact sink is attached
    pub fn has_fact_sink(&self) -> bool {
        self.shared.fact_publish_tx.lock().is_some()
    }

    /// Publish facts to the reactive scheduler
    ///
    /// Returns Ok(()) if publication succeeded or no sink is attached.
    /// Returns Err if the sink channel is closed.
    #[allow(dead_code)]
    pub async fn publish_facts(&self, source: FactSource) -> Result<(), JournalSubsystemError> {
        let tx = self.shared.fact_publish_tx.lock().clone();
        if let Some(tx) = tx {
            tx.send(source)
                .await
                .map_err(|_| JournalSubsystemError::SinkClosed)?;
        }
        Ok(())
    }

    /// Attach a view update sender for awaiting fact processing
    ///
    /// This allows commit operations to wait for the reactive scheduler
    /// to process their facts before returning.
    pub fn attach_view_update_sender(&self, tx: broadcast::Sender<ViewUpdate>) {
        *self.shared.view_update_tx.lock() = Some(tx);
    }

    /// Subscribe to view updates for awaiting fact processing
    ///
    /// Returns None if no view update sender is attached.
    pub fn subscribe_view_updates(&self) -> Option<broadcast::Receiver<ViewUpdate>> {
        self.shared
            .view_update_tx
            .lock()
            .as_ref()
            .map(|tx| tx.subscribe())
    }

    /// Check if view update subscription is available
    #[allow(dead_code)]
    pub fn has_view_update_sender(&self) -> bool {
        self.shared.view_update_tx.lock().is_some()
    }
}

/// Errors from journal subsystem operations
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum JournalSubsystemError {
    #[error("Fact publication sink is closed")]
    SinkClosed,
}

impl Clone for JournalSubsystem {
    fn clone(&self) -> Self {
        Self {
            indexed_journal: self.indexed_journal.clone(),
            fact_registry: self.fact_registry.clone(),
            journal_policy: self.journal_policy.clone(),
            journal_verifying_key: self.journal_verifying_key.clone(),
            shared: Arc::clone(&self.shared),
        }
    }
}

impl std::fmt::Debug for JournalSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalSubsystem")
            .field("indexed_journal", &"<Arc<IndexedJournalHandler>>")
            .field("fact_registry", &"<Arc<FactRegistry>>")
            .field("has_fact_sink", &self.has_fact_sink())
            .field("has_journal_policy", &self.journal_policy.is_some())
            .field("has_verifying_key", &self.journal_verifying_key.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_registry::build_fact_registry;
    use aura_core::effects::IndexedJournalEffects;

    #[test]
    fn test_journal_subsystem_creation() {
        let registry = Arc::new(build_fact_registry());
        let subsystem = JournalSubsystem::new(1000u64, registry);
        assert!(!subsystem.has_fact_sink());
        assert!(subsystem.journal_policy().is_none());
    }

    #[test]
    fn test_fact_sink_attachment() {
        let registry = Arc::new(build_fact_registry());
        let subsystem = JournalSubsystem::new(1000u64, registry);

        let (tx, _rx) = mpsc::channel(16);
        subsystem.attach_fact_sink(tx);
        assert!(subsystem.has_fact_sink());

        subsystem.detach_fact_sink();
        assert!(!subsystem.has_fact_sink());
    }

    #[tokio::test]
    async fn test_publish_facts_reports_closed_sink() {
        let registry = Arc::new(build_fact_registry());
        let subsystem = JournalSubsystem::new(1000u64, registry);
        let (tx, rx) = mpsc::channel(1);
        subsystem.attach_fact_sink(tx);
        drop(rx);

        let result = subsystem
            .publish_facts(FactSource::Journal(Vec::new()))
            .await;
        assert!(matches!(result, Err(JournalSubsystemError::SinkClosed)));
    }

    #[tokio::test]
    async fn test_index_new_journal_facts_skips_already_indexed_keys() {
        let registry = Arc::new(build_fact_registry());
        let subsystem = JournalSubsystem::new(1000u64, registry);
        let mut journal = aura_core::Journal::new();
        journal
            .facts
            .insert(
                "relational:test:1".to_string(),
                aura_core::FactValue::String("value".to_string()),
            )
            .unwrap();

        let added_first = subsystem.index_new_journal_facts(&journal, None, None);
        let added_second = subsystem.index_new_journal_facts(&journal, None, None);
        let stats = subsystem.indexed_journal().index_stats().await.unwrap();

        assert_eq!(added_first, 1);
        assert_eq!(added_second, 0);
        assert_eq!(stats.fact_count, 1);
    }
}
