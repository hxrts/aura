//! Query Effect Traits
//!
//! Algebraic effects for executing Datalog queries against the journal.
//! Integrates with Biscuit authorization and reactive signal subscriptions.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Dependencies**: JournalEffects, AuthorizationEffects
//!
//! # Architecture
//!
//! QueryEffects bridges the gap between:
//! - **Journal**: CRDT fact storage
//! - **Datalog**: Query language for facts
//! - **Biscuit**: Authorization for query execution
//! - **Reactive**: Signal subscriptions for live queries
//!
//! ```text
//! Query::to_datalog() → DatalogProgram
//!        ↓
//! QueryEffects::query() → Check Biscuit capabilities
//!        ↓
//! Execute Datalog against journal facts
//!        ↓
//! Query::parse() → Typed result
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::effects::reactive::SignalStream;
use crate::effects::reactive::{ReactiveEffects, ReactiveError, Signal, SignalId};
use crate::query::{
    ConsensusId, DatalogBindings, Query, QueryCapability, QueryIsolation, QueryParseError,
    QueryStats,
};

// ─────────────────────────────────────────────────────────────────────────────
// Error Types
// ─────────────────────────────────────────────────────────────────────────────

/// Error type for query operations
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum QueryError {
    /// Authorization failed
    #[error("Query authorization failed: {reason}")]
    AuthorizationFailed { reason: String },

    /// Missing required capability
    #[error("Missing capability: {capability}")]
    MissingCapability { capability: String },

    /// Datalog execution error
    #[error("Datalog execution error: {reason}")]
    ExecutionError { reason: String },

    /// Result parsing error
    #[error("Failed to parse query results: {0}")]
    ParseError(#[from] QueryParseError),

    /// Query not found (for subscriptions)
    #[error("Query subscription not found: {query_id}")]
    SubscriptionNotFound { query_id: String },

    /// Journal access error
    #[error("Journal access error: {reason}")]
    JournalError { reason: String },

    /// Handler not available
    #[error("Query handler not available")]
    HandlerUnavailable,

    /// Internal error
    #[error("Internal query error: {reason}")]
    Internal { reason: String },

    /// Consensus wait timeout
    #[error("Timed out waiting for consensus: {consensus_id:?}")]
    ConsensusTimeout { consensus_id: ConsensusId },

    /// Snapshot not available (garbage collected)
    #[error("Snapshot not available: prestate {prestate_hash:?} has been garbage collected")]
    SnapshotNotAvailable { prestate_hash: crate::Hash32 },

    /// Isolation level not supported
    #[error("Isolation level not supported: {reason}")]
    IsolationNotSupported { reason: String },
}

impl QueryError {
    /// Create an authorization failed error
    pub fn authorization_failed(reason: impl Into<String>) -> Self {
        Self::AuthorizationFailed {
            reason: reason.into(),
        }
    }

    /// Create a missing capability error
    pub fn missing_capability(cap: &QueryCapability) -> Self {
        Self::MissingCapability {
            capability: format!("{}:{}", cap.resource, cap.action),
        }
    }

    /// Create an execution error
    pub fn execution_error(reason: impl Into<String>) -> Self {
        Self::ExecutionError {
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

    /// Create a consensus timeout error
    pub fn consensus_timeout(consensus_id: ConsensusId) -> Self {
        Self::ConsensusTimeout { consensus_id }
    }

    /// Create a snapshot not available error
    pub fn snapshot_not_available(prestate_hash: crate::Hash32) -> Self {
        Self::SnapshotNotAvailable { prestate_hash }
    }

    /// Create an isolation not supported error
    pub fn isolation_not_supported(reason: impl Into<String>) -> Self {
        Self::IsolationNotSupported {
            reason: reason.into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query Effects Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Effects for executing typed Datalog queries.
///
/// This trait provides the read interface to the journal through Datalog queries.
/// All queries:
/// 1. Compile to Datalog programs via `Query::to_datalog()`
/// 2. Are authorized via Biscuit capabilities
/// 3. Execute against journal facts
/// 4. Parse results to typed values
///
/// # Example
///
/// ```ignore
/// use aura_core::effects::QueryEffects;
/// use aura_app::queries::ChannelsQuery;
///
/// // One-shot query
/// let channels = handler.query(&ChannelsQuery::default()).await?;
///
/// // Live subscription (re-evaluates when facts change)
/// let mut stream = handler.subscribe(&ChannelsQuery::default());
/// while let Some(channels) = stream.recv().await {
///     println!("Channels updated: {} total", channels.len());
/// }
/// ```
#[async_trait]
pub trait QueryEffects: Send + Sync {
    /// Execute a one-shot query.
    ///
    /// This compiles the query to Datalog, checks authorization,
    /// executes against the journal, and parses the results.
    ///
    /// # Errors
    ///
    /// - `QueryError::AuthorizationFailed` if capability check fails
    /// - `QueryError::ExecutionError` if Datalog execution fails
    /// - `QueryError::ParseError` if result parsing fails
    async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError>;

    /// Execute a raw Datalog program and return bindings.
    ///
    /// Lower-level API for executing arbitrary Datalog without typed parsing.
    /// Useful for debugging or dynamic queries.
    async fn query_raw(
        &self,
        program: &crate::query::DatalogProgram,
    ) -> Result<DatalogBindings, QueryError>;

    /// Subscribe to a query for live updates.
    ///
    /// Returns a stream that re-evaluates the query whenever facts
    /// matching the query's `dependencies()` change.
    ///
    /// The stream yields new results after each relevant fact change.
    fn subscribe<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result>;

    /// Check if a query's capabilities are satisfied.
    ///
    /// Can be used to pre-check authorization before execution.
    async fn check_capabilities(&self, capabilities: &[QueryCapability]) -> Result<(), QueryError>;

    /// Invalidate cached results for queries matching the given predicate.
    ///
    /// Called when facts change to trigger re-evaluation of subscriptions.
    async fn invalidate(&self, predicate: &crate::query::FactPredicate);

    /// Execute a query with a specific isolation level.
    ///
    /// Allows specifying consistency requirements for the query.
    /// See `QueryIsolation` for available levels.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Wait for specific consensus before querying
    /// let result = handler.query_with_isolation(
    ///     &ChannelsQuery::default(),
    ///     QueryIsolation::ReadCommitted { wait_for: vec![consensus_id] },
    /// ).await?;
    /// ```
    async fn query_with_isolation<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<Q::Result, QueryError>;

    /// Execute a query and return results with execution statistics.
    ///
    /// Useful for debugging, profiling, and optimization. Returns both
    /// the query results and metadata about the execution.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (channels, stats) = handler.query_with_stats(&ChannelsQuery::default()).await?;
    /// println!("Query took {:?}, scanned {} facts", stats.execution_time, stats.facts_scanned);
    /// ```
    async fn query_with_stats<Q: Query>(
        &self,
        query: &Q,
    ) -> Result<(Q::Result, QueryStats), QueryError>;

    /// Execute a query with both isolation level and statistics.
    ///
    /// Combines `query_with_isolation` and `query_with_stats`.
    async fn query_full<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<(Q::Result, QueryStats), QueryError>;

    /// Register a query binding for reactive refresh.
    ///
    /// Implementations should store the query/signal mapping and emit
    /// the initial query result.
    async fn register_query_binding<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), QueryError>;
}

/// Convenience extension trait to register query-bound signals via QueryEffects.
#[allow(async_fn_in_trait)]
pub trait QuerySignalEffects: QueryEffects + ReactiveEffects {
    /// Register a reactive signal that is bound to a query.
    async fn register_query_signal<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError> {
        let query_clone = query.clone();
        self.register_query(signal, query).await?;
        self.register_query_binding(signal, query_clone)
            .await
            .map_err(|e| ReactiveError::Internal {
                reason: e.to_string(),
            })?;
        Ok(())
    }

    /// Read query dependencies for a signal id.
    fn query_dependencies_for(&self, signal_id: &SignalId) -> Option<Vec<crate::query::FactPredicate>> {
        self.query_dependencies(signal_id)
    }
}

impl<T: QueryEffects + ReactiveEffects> QuerySignalEffects for T {}

// ─────────────────────────────────────────────────────────────────────────────
// Query Subscription
// ─────────────────────────────────────────────────────────────────────────────

/// A subscription to query results that updates when facts change.
///
/// QuerySubscription wraps a SignalStream but provides query-specific semantics.
/// Results are re-evaluated and emitted when underlying facts change.
pub struct QuerySubscription<T: Clone + Send + 'static> {
    /// Underlying signal stream
    stream: SignalStream<T>,
    /// Query ID for debugging
    query_id: String,
}

impl<T: Clone + Send + 'static> QuerySubscription<T> {
    /// Create a new query subscription
    pub fn new(stream: SignalStream<T>, query_id: impl Into<String>) -> Self {
        Self {
            stream,
            query_id: query_id.into(),
        }
    }

    /// Get the query ID
    pub fn query_id(&self) -> &str {
        &self.query_id
    }

    /// Try to receive the next result without blocking
    pub fn try_recv(&mut self) -> Option<T> {
        self.stream.try_recv()
    }

    /// Receive the next result, waiting if necessary
    pub async fn recv(&mut self) -> Result<T, QueryError> {
        self.stream.recv().await.map_err(|e| QueryError::Internal {
            reason: e.to_string(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Blanket Implementations
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;

/// Blanket implementation for Arc<T> where T: QueryEffects
#[async_trait]
impl<T: QueryEffects + ?Sized> QueryEffects for Arc<T> {
    async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError> {
        (**self).query(query).await
    }

    async fn query_raw(
        &self,
        program: &crate::query::DatalogProgram,
    ) -> Result<DatalogBindings, QueryError> {
        (**self).query_raw(program).await
    }

    fn subscribe<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result> {
        (**self).subscribe(query)
    }

    async fn check_capabilities(&self, capabilities: &[QueryCapability]) -> Result<(), QueryError> {
        (**self).check_capabilities(capabilities).await
    }

    async fn invalidate(&self, predicate: &crate::query::FactPredicate) {
        (**self).invalidate(predicate).await
    }

    async fn query_with_isolation<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<Q::Result, QueryError> {
        (**self).query_with_isolation(query, isolation).await
    }

    async fn query_with_stats<Q: Query>(
        &self,
        query: &Q,
    ) -> Result<(Q::Result, QueryStats), QueryError> {
        (**self).query_with_stats(query).await
    }

    async fn query_full<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<(Q::Result, QueryStats), QueryError> {
        (**self).query_full(query, isolation).await
    }

    async fn register_query_binding<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), QueryError> {
        (**self).register_query_binding(signal, query).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_error_display() {
        let err = QueryError::authorization_failed("insufficient permissions");
        assert!(err.to_string().contains("authorization"));

        let cap = QueryCapability::read("channels");
        let err = QueryError::missing_capability(&cap);
        assert!(err.to_string().contains("channels:read"));
    }

    #[test]
    fn test_query_error_from_parse_error() {
        let parse_err = QueryParseError::MissingField {
            field: "id".to_string(),
        };
        let query_err: QueryError = parse_err.into();
        assert!(matches!(query_err, QueryError::ParseError(_)));
    }
}
