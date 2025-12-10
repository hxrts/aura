//! Fact Effect Traits
//!
//! Algebraic effects for temporal database operations on facts.
//! This module provides the mutation/write interface to complement QueryEffects (read).
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-effects` (Layer 3) or domain crates
//! - **Dependencies**: JournalEffects, CryptoEffects (for hashing)
//!
//! # Architecture
//!
//! FactEffects bridges:
//! - **Temporal Model**: Datomic-inspired immutable database semantics
//! - **Journal**: CRDT-based fact storage
//! - **Finality**: Configurable durability levels
//! - **Scopes**: Hierarchical namespace organization
//!
//! ```text
//! FactOp (Assert/Tombstone/EpochBump/Checkpoint)
//!        ↓
//! FactEffects::apply_op() → Check scope finality config
//!        ↓
//! Apply to journal (CRDT merge or consensus)
//!        ↓
//! FactReceipt with current finality level
//! ```
//!
//! # Relationship to QueryEffects
//!
//! - `QueryEffects`: Read interface (Datalog queries, subscriptions)
//! - `FactEffects`: Write interface (temporal mutations, transactions)
//!
//! Together they form the complete database interface.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::domain::temporal::{
    FactOp, FactReceipt, Finality, FinalityError, ScopeFinalityConfig, ScopeId, TemporalPoint,
    TemporalQuery, Transaction, TransactionReceipt,
};
use crate::query::FactId;
use crate::time::PhysicalTime;
use crate::Hash32;

// ─────────────────────────────────────────────────────────────────────────────
// Error Types
// ─────────────────────────────────────────────────────────────────────────────

/// Error type for fact operations
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum FactError {
    /// Scope not found or not accessible
    #[error("Scope not found: {scope}")]
    ScopeNotFound { scope: String },

    /// Fact not found (for tombstone/reference operations)
    #[error("Fact not found: {fact_id:?}")]
    FactNotFound { fact_id: FactId },

    /// Finality requirement not met
    #[error("Finality error: {0}")]
    Finality(#[from] FinalityError),

    /// Transaction conflict (concurrent modification)
    #[error("Transaction conflict in scope {scope}: {reason}")]
    TransactionConflict { scope: String, reason: String },

    /// Invalid epoch bump (must be strictly increasing)
    #[error("Invalid epoch bump: new epoch {new_epoch} not greater than current {current_epoch}")]
    InvalidEpochBump { current_epoch: u64, new_epoch: u64 },

    /// Authorization failed
    #[error("Not authorized to modify scope {scope}: {reason}")]
    NotAuthorized { scope: String, reason: String },

    /// Journal write failed
    #[error("Journal write failed: {reason}")]
    JournalError { reason: String },

    /// Handler not available
    #[error("Fact effect handler not available")]
    HandlerUnavailable,

    /// Internal error
    #[error("Internal fact error: {reason}")]
    Internal { reason: String },

    /// Temporal query error
    #[error("Temporal query error: {reason}")]
    TemporalQueryError { reason: String },

    /// Checkpoint not found (for as_of queries)
    #[error("Checkpoint not found at {point:?}")]
    CheckpointNotFound { point: String },

    /// Invalid operation
    #[error("Invalid operation: {reason}")]
    InvalidOperation { reason: String },
}

impl FactError {
    /// Create a scope not found error
    pub fn scope_not_found(scope: &ScopeId) -> Self {
        Self::ScopeNotFound {
            scope: scope.to_string(),
        }
    }

    /// Create a fact not found error
    pub fn fact_not_found(fact_id: FactId) -> Self {
        Self::FactNotFound { fact_id }
    }

    /// Create a transaction conflict error
    pub fn conflict(scope: &ScopeId, reason: impl Into<String>) -> Self {
        Self::TransactionConflict {
            scope: scope.to_string(),
            reason: reason.into(),
        }
    }

    /// Create an authorization error
    pub fn not_authorized(scope: &ScopeId, reason: impl Into<String>) -> Self {
        Self::NotAuthorized {
            scope: scope.to_string(),
            reason: reason.into(),
        }
    }

    /// Create a journal error
    pub fn journal_error(reason: impl Into<String>) -> Self {
        Self::JournalError {
            reason: reason.into(),
        }
    }

    /// Create an internal error
    pub fn internal(reason: impl Into<String>) -> Self {
        Self::Internal {
            reason: reason.into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fact Effects Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Effects for temporal database mutations.
///
/// This trait provides the write interface for the temporal database,
/// complementing `QueryEffects` which provides the read interface.
///
/// # Operations
///
/// - `apply_op`: Apply a single fact operation
/// - `apply_transaction`: Apply a group of operations atomically
/// - `wait_for_finality`: Wait for an operation to reach a finality level
/// - `configure_scope`: Set finality configuration for a scope
///
/// # Example
///
/// ```ignore
/// use aura_core::effects::FactEffects;
/// use aura_core::domain::temporal::{FactOp, FactContent, ScopeId, Finality};
///
/// // Simple assertion (no transaction needed)
/// let receipt = handler.apply_op(
///     FactOp::assert(FactContent::new("message", content_bytes)),
///     &ScopeId::parse("authority:abc/chat/channel:xyz")?,
/// ).await?;
///
/// // Wait for replication
/// handler.wait_for_finality(
///     receipt.fact_id,
///     Finality::replicated(3),
/// ).await?;
/// ```
#[async_trait]
pub trait FactEffects: Send + Sync {
    /// Apply a single fact operation to a scope.
    ///
    /// For simple monotonic operations, this is the preferred method.
    /// The operation is applied immediately to the local journal and
    /// replicated according to scope configuration.
    ///
    /// # Arguments
    ///
    /// * `op` - The operation to apply (Assert, Tombstone, EpochBump, Checkpoint)
    /// * `scope` - The scope to apply the operation in
    ///
    /// # Returns
    ///
    /// A receipt containing the fact ID, timestamp, and initial finality level.
    async fn apply_op(&self, op: FactOp, scope: &ScopeId) -> Result<FactReceipt, FactError>;

    /// Apply a transaction atomically.
    ///
    /// All operations in the transaction succeed or none do.
    /// The transaction is applied according to its required finality level.
    ///
    /// # Arguments
    ///
    /// * `transaction` - The transaction containing operations to apply
    ///
    /// # Returns
    ///
    /// A receipt containing receipts for each operation and transaction finality.
    async fn apply_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<TransactionReceipt, FactError>;

    /// Wait for a fact to reach a specific finality level.
    ///
    /// Blocks until the fact achieves the requested finality or times out.
    ///
    /// # Arguments
    ///
    /// * `fact_id` - The fact to wait for
    /// * `target` - The finality level to wait for
    ///
    /// # Returns
    ///
    /// The achieved finality level (may exceed target).
    async fn wait_for_finality(
        &self,
        fact_id: FactId,
        target: Finality,
    ) -> Result<Finality, FactError>;

    /// Get the current finality level of a fact.
    async fn get_finality(&self, fact_id: FactId) -> Result<Finality, FactError>;

    /// Configure finality requirements for a scope.
    ///
    /// Sets default and minimum finality levels, plus content-type overrides.
    async fn configure_scope(&self, config: ScopeFinalityConfig) -> Result<(), FactError>;

    /// Get the finality configuration for a scope.
    ///
    /// Returns the effective configuration, considering inheritance from parent scopes.
    async fn get_scope_config(&self, scope: &ScopeId) -> Result<ScopeFinalityConfig, FactError>;

    /// Get the current epoch for a scope.
    async fn get_epoch(&self, scope: &ScopeId) -> Result<crate::types::Epoch, FactError>;

    /// Create a checkpoint for a scope.
    ///
    /// Computes the state hash and creates a checkpoint fact.
    /// Returns the checkpoint fact receipt.
    async fn checkpoint(&self, scope: &ScopeId) -> Result<FactReceipt, FactError>;

    /// Get the state hash at a specific temporal point.
    ///
    /// Used for:
    /// - Verifying checkpoint integrity
    /// - Constructing as_of queries
    /// - Comparing states across time
    async fn get_state_hash(
        &self,
        scope: &ScopeId,
        point: TemporalPoint,
    ) -> Result<Hash32, FactError>;

    /// Query facts with temporal semantics.
    ///
    /// Executes a query with temporal constraints (as_of, since, history).
    /// This is a lower-level interface than QueryEffects, returning raw fact data.
    async fn query_temporal(
        &self,
        scope: &ScopeId,
        temporal: TemporalQuery,
    ) -> Result<Vec<TemporalFact>, FactError>;

    /// List available checkpoints for a scope.
    ///
    /// Returns checkpoints in order, which can be used for as_of queries.
    async fn list_checkpoints(&self, scope: &ScopeId) -> Result<Vec<CheckpointInfo>, FactError>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Supporting Types
// ─────────────────────────────────────────────────────────────────────────────

/// A fact with temporal metadata for query results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalFact {
    /// The fact identifier
    pub fact_id: FactId,
    /// When the fact was asserted
    pub asserted_at: PhysicalTime,
    /// When the fact was retracted (if tombstoned)
    pub retracted_at: Option<PhysicalTime>,
    /// The scope containing this fact
    pub scope: ScopeId,
    /// The epoch when this fact was created
    pub epoch: crate::types::Epoch,
    /// The content type
    pub content_type: String,
    /// The fact content (serialized)
    pub content: Vec<u8>,
    /// Current finality level
    pub finality: Finality,
    /// Optional entity ID for entity-based queries
    pub entity_id: Option<String>,
}

impl TemporalFact {
    /// Check if this fact is currently valid (not tombstoned)
    pub fn is_valid(&self) -> bool {
        self.retracted_at.is_none()
    }

    /// Check if this fact was valid at a specific time
    pub fn was_valid_at(&self, time: PhysicalTime) -> bool {
        if self.asserted_at > time {
            return false;
        }
        match &self.retracted_at {
            Some(retracted) => *retracted > time,
            None => true,
        }
    }
}

/// Information about a checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    /// The checkpoint fact ID
    pub fact_id: FactId,
    /// When the checkpoint was created
    pub created_at: PhysicalTime,
    /// The state hash at this checkpoint
    pub state_hash: Hash32,
    /// The epoch at this checkpoint
    pub epoch: crate::types::Epoch,
    /// Number of facts covered by this checkpoint
    pub fact_count: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Blanket Implementations
// ─────────────────────────────────────────────────────────────────────────────

/// Blanket implementation for Arc<T> where T: FactEffects
#[async_trait]
impl<T: FactEffects + ?Sized> FactEffects for Arc<T> {
    async fn apply_op(&self, op: FactOp, scope: &ScopeId) -> Result<FactReceipt, FactError> {
        (**self).apply_op(op, scope).await
    }

    async fn apply_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<TransactionReceipt, FactError> {
        (**self).apply_transaction(transaction).await
    }

    async fn wait_for_finality(
        &self,
        fact_id: FactId,
        target: Finality,
    ) -> Result<Finality, FactError> {
        (**self).wait_for_finality(fact_id, target).await
    }

    async fn get_finality(&self, fact_id: FactId) -> Result<Finality, FactError> {
        (**self).get_finality(fact_id).await
    }

    async fn configure_scope(&self, config: ScopeFinalityConfig) -> Result<(), FactError> {
        (**self).configure_scope(config).await
    }

    async fn get_scope_config(&self, scope: &ScopeId) -> Result<ScopeFinalityConfig, FactError> {
        (**self).get_scope_config(scope).await
    }

    async fn get_epoch(&self, scope: &ScopeId) -> Result<crate::types::Epoch, FactError> {
        (**self).get_epoch(scope).await
    }

    async fn checkpoint(&self, scope: &ScopeId) -> Result<FactReceipt, FactError> {
        (**self).checkpoint(scope).await
    }

    async fn get_state_hash(
        &self,
        scope: &ScopeId,
        point: TemporalPoint,
    ) -> Result<Hash32, FactError> {
        (**self).get_state_hash(scope, point).await
    }

    async fn query_temporal(
        &self,
        scope: &ScopeId,
        temporal: TemporalQuery,
    ) -> Result<Vec<TemporalFact>, FactError> {
        (**self).query_temporal(scope, temporal).await
    }

    async fn list_checkpoints(&self, scope: &ScopeId) -> Result<Vec<CheckpointInfo>, FactError> {
        (**self).list_checkpoints(scope).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_error_display() {
        let err = FactError::scope_not_found(&ScopeId::authority("abc"));
        assert!(err.to_string().contains("authority:abc"));

        let err = FactError::conflict(&ScopeId::root(), "concurrent update");
        assert!(err.to_string().contains("concurrent update"));
    }

    fn time_at(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_temporal_fact_validity() {
        let fact = TemporalFact {
            fact_id: FactId([0; 32]),
            asserted_at: time_at(1000),
            retracted_at: None,
            scope: ScopeId::root(),
            epoch: crate::types::Epoch::new(0),
            content_type: "test".to_string(),
            content: vec![],
            finality: Finality::Local,
            entity_id: None,
        };

        assert!(fact.is_valid());
        assert!(fact.was_valid_at(time_at(1500)));
        assert!(!fact.was_valid_at(time_at(500)));
    }

    #[test]
    fn test_temporal_fact_tombstoned() {
        let fact = TemporalFact {
            fact_id: FactId([0; 32]),
            asserted_at: time_at(1000),
            retracted_at: Some(time_at(2000)),
            scope: ScopeId::root(),
            epoch: crate::types::Epoch::new(0),
            content_type: "test".to_string(),
            content: vec![],
            finality: Finality::Local,
            entity_id: None,
        };

        assert!(!fact.is_valid());
        assert!(fact.was_valid_at(time_at(1500)));
        assert!(!fact.was_valid_at(time_at(2500)));
    }
}
