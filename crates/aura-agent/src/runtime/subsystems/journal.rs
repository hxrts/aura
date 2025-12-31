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
use crate::reactive::FactSource;
use aura_authorization::BiscuitAuthorizationBridge;
use aura_journal::extensibility::FactRegistry;
use biscuit_auth::Biscuit;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Journal subsystem grouping fact storage and publication.
///
/// This subsystem encapsulates:
/// - Indexed journal for efficient fact lookups
/// - Fact registry for domain extensibility
/// - Publication channel for reactive updates
/// - Authorization policy for journal operations
pub struct JournalSubsystem {
    /// Indexed journal handler for efficient fact lookups
    ///
    /// Provides B-tree indices, Bloom filters, and Merkle proofs.
    indexed_journal: Arc<IndexedJournalHandler>,

    /// Domain fact registry for extensibility
    ///
    /// Maps fact types to reducers and validators.
    fact_registry: Arc<FactRegistry>,

    /// Reactive scheduler publication channel
    ///
    /// Protected by parking_lot::Mutex for synchronous access.
    /// Only the channel sender clone is accessed (brief operation).
    fact_publish_tx: Mutex<Option<mpsc::Sender<FactSource>>>,

    /// Biscuit authorization policy for journal operations
    ///
    /// Tuple of (token, bridge) for capability-based authorization.
    journal_policy: Option<(Biscuit, BiscuitAuthorizationBridge)>,

    /// Verification key for journal signatures
    journal_verifying_key: Option<Vec<u8>>,
}

impl JournalSubsystem {
    /// Create a new journal subsystem with the given capacity
    #[allow(dead_code)]
    pub fn new(capacity: u64, fact_registry: Arc<FactRegistry>) -> Self {
        Self {
            indexed_journal: Arc::new(IndexedJournalHandler::with_capacity(capacity)),
            fact_registry,
            fact_publish_tx: Mutex::new(None),
            journal_policy: None,
            journal_verifying_key: None,
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
            fact_publish_tx: Mutex::new(fact_publish_tx),
            journal_policy,
            journal_verifying_key,
        }
    }

    /// Get the indexed journal handler
    pub fn indexed_journal(&self) -> Arc<IndexedJournalHandler> {
        self.indexed_journal.clone()
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
        *self.fact_publish_tx.lock() = Some(tx);
    }

    /// Detach the fact sink
    #[allow(dead_code)]
    pub fn detach_fact_sink(&self) {
        *self.fact_publish_tx.lock() = None;
    }

    /// Get a clone of the fact publication sender (if attached)
    pub fn fact_publisher(&self) -> Option<mpsc::Sender<FactSource>> {
        self.fact_publish_tx.lock().clone()
    }

    /// Check if a fact sink is attached
    pub fn has_fact_sink(&self) -> bool {
        self.fact_publish_tx.lock().is_some()
    }

    /// Publish facts to the reactive scheduler
    ///
    /// Returns Ok(()) if publication succeeded or no sink is attached.
    /// Returns Err if the sink channel is closed.
    #[allow(dead_code)]
    pub async fn publish_facts(&self, source: FactSource) -> Result<(), JournalSubsystemError> {
        let tx = self.fact_publish_tx.lock().clone();
        if let Some(tx) = tx {
            tx.send(source)
                .await
                .map_err(|_| JournalSubsystemError::SinkClosed)?;
        }
        Ok(())
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
            fact_publish_tx: Mutex::new(self.fact_publish_tx.lock().clone()),
            journal_policy: self.journal_policy.clone(),
            journal_verifying_key: self.journal_verifying_key.clone(),
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
            .field(
                "has_verifying_key",
                &self.journal_verifying_key.is_some(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_registry::build_fact_registry;

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
}
