//! Unified Effect Handler
//!
//! Composes Authorization, Journal, Query, and Reactive effects into a single
//! cohesive handler for the Effects-Query-FRP architecture.
//!
//! # Architecture
//!
//! ```text
//! Intent → AuthorizationEffects → JournalEffects → QueryEffects → ReactiveEffects → UI
//!              (Biscuit)           (CRDT)         (Datalog)        (Signals)
//! ```
//!
//! The unified handler provides:
//! - Capability-based authorization via Biscuit tokens
//! - CRDT-based fact storage via Journal
//! - Datalog query execution with capability checks
//! - Reactive signal updates when facts change
//!
//! # Usage
//!
//! ```ignore
//! use aura_app::UnifiedHandler;
//!
//! // Create with default configuration
//! let handler = UnifiedHandler::new();
//!
//! // Execute a query with authorization
//! let results = handler.authorized_query(&token, &contacts_query).await?;
//!
//! // Subscribe to query results
//! let stream = handler.subscribe_query(&contacts_query);
//!
//! // Commit a fact (triggers query invalidation)
//! handler.commit_fact(fact).await?;
//! ```

use async_trait::async_trait;
use std::sync::Arc;

use aura_core::domain::ConsistencyMap;
use aura_core::effects::{
    indexed::IndexedJournalEffects,
    query::{QueryEffects, QueryError, QuerySubscription},
    reactive::{ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream},
};
use aura_core::query::{
    DatalogBindings, DatalogProgram, FactPredicate, Query, QueryCapability, QueryIsolation,
    QueryStats,
};

use crate::effects::query::{CapabilityPolicy, QueryHandler};
use crate::effects::reactive::ReactiveHandler;

// ─────────────────────────────────────────────────────────────────────────────
// Unified Handler
// ─────────────────────────────────────────────────────────────────────────────

/// Unified effect handler composing Authorization, Journal, Query, and Reactive effects.
///
/// This is the primary entry point for the Effects-Query-FRP architecture.
/// It coordinates between:
/// - Query execution with capability checks
/// - Signal updates when facts change
/// - Automatic query invalidation
pub struct UnifiedHandler {
    /// Query effect handler (includes reactive for subscriptions)
    query: QueryHandler,
    /// Reactive effect handler for signal management
    reactive: Arc<ReactiveHandler>,
    /// Capability context for authorization (token bytes, if available)
    capability_context: Option<Vec<u8>>,
}

impl UnifiedHandler {
    /// Create a new unified handler with default configuration.
    pub fn new() -> Self {
        let reactive = Arc::new(ReactiveHandler::new());
        let query =
            QueryHandler::new_with_policy(reactive.clone(), CapabilityPolicy::DenyUnlessGranted);

        Self {
            query,
            reactive,
            capability_context: None,
        }
    }

    /// Create a unified handler with a shared reactive handler.
    ///
    /// Allows multiple handlers to share the same signal graph.
    pub fn with_reactive(reactive: Arc<ReactiveHandler>) -> Self {
        let query =
            QueryHandler::new_with_policy(reactive.clone(), CapabilityPolicy::DenyUnlessGranted);

        Self {
            query,
            reactive,
            capability_context: None,
        }
    }

    /// Create a unified handler with indexed journal support.
    ///
    /// When an indexed journal is provided, queries can:
    /// - Load facts from the index for O(log n) predicate lookups
    /// - Use bloom filter to quickly check if facts exist before expensive queries
    pub fn with_indexed_journal(
        reactive: Arc<ReactiveHandler>,
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
    ) -> Self {
        let query = QueryHandler::with_indexed_journal_with_policy(
            reactive.clone(),
            indexed_journal,
            CapabilityPolicy::DenyUnlessGranted,
        );

        Self {
            query,
            reactive,
            capability_context: None,
        }
    }

    /// Set the capability context (Biscuit token) for authorization.
    ///
    /// All subsequent queries will be checked against this token.
    pub fn set_capability_context(&mut self, token: Vec<u8>) {
        self.capability_context = Some(token);
    }

    /// Clear the capability context.
    pub fn clear_capability_context(&mut self) {
        self.capability_context = None;
    }

    /// Allow all queries regardless of capabilities (testing/offline convenience).
    pub async fn allow_unrestricted_queries(&self) {
        self.query
            .set_capability_policy(CapabilityPolicy::AllowAll)
            .await;
    }

    /// Get a reference to the query handler.
    pub fn query_handler(&self) -> &QueryHandler {
        &self.query
    }

    /// Get a reference to the reactive handler.
    pub fn reactive_handler(&self) -> &ReactiveHandler {
        &self.reactive
    }

    // =========================================================================
    // Fact Management
    // =========================================================================

    /// Add a fact to the query store.
    ///
    /// This makes the fact available for queries. After adding facts,
    /// call `invalidate_affected_queries` to update bound signals.
    pub async fn add_fact(&self, predicate: &str, args: Vec<String>) {
        self.query.add_fact(predicate, args).await;
    }

    /// Add multiple facts at once.
    pub async fn add_facts(&self, entries: Vec<(String, Vec<String>)>) {
        self.query.add_facts(entries).await;
    }

    /// Commit a fact and invalidate affected queries.
    ///
    /// This is the primary method for adding facts that should trigger
    /// reactive updates. It:
    /// 1. Adds the fact to the query store
    /// 2. Creates a FactPredicate for the fact
    /// 3. Invalidates all queries that depend on this predicate
    pub async fn commit_fact(&self, predicate: &str, args: Vec<String>) {
        // Add the fact
        self.query.add_fact(predicate, args).await;

        // Invalidate affected queries
        let fact_predicate = FactPredicate::new(predicate);
        self.query.invalidate(&fact_predicate).await;
    }

    /// Clear all facts from the query store.
    pub async fn clear_facts(&self) {
        self.query.clear_facts().await;
    }

    // =========================================================================
    // Query Operations (with Authorization)
    // =========================================================================

    /// Execute a query with the current capability context.
    ///
    /// If no capability context is set, queries execute without authorization.
    pub async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError> {
        // Check capabilities if context is set
        if self.capability_context.is_some() {
            let required = query.required_capabilities();
            self.query.check_capabilities(&required).await?;
        }

        // Execute the query
        self.query.query(query).await
    }

    /// Execute a query with explicit authorization token.
    pub async fn authorized_query<Q: Query>(
        &self,
        _token: &[u8],
        query: &Q,
    ) -> Result<Q::Result, QueryError> {
        // Check required capabilities
        let required = query.required_capabilities();
        self.query.check_capabilities(&required).await?;

        // Execute the query
        self.query.query(query).await
    }

    /// Subscribe to query results.
    ///
    /// Returns a stream that emits new results whenever the query's
    /// dependent facts change.
    pub fn subscribe_query<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result> {
        self.query.subscribe(query)
    }

    // =========================================================================
    // Signal Operations (Reactive)
    // =========================================================================

    /// Register a signal with an initial value.
    pub async fn register_signal<T>(
        &self,
        signal: &Signal<T>,
        initial: T,
    ) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.register(signal, initial).await
    }

    /// Register a query-bound signal.
    ///
    /// The signal will automatically update when facts matching the
    /// query's dependencies change.
    pub async fn register_query_signal<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), ReactiveError> {
        use aura_core::effects::query::QuerySignalEffects;

        QuerySignalEffects::register_query_signal(self, signal, query).await
    }

    /// Read a signal's current value.
    pub async fn read_signal<T>(&self, signal: &Signal<T>) -> Result<T, ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.read(signal).await
    }

    /// Emit a new value to a signal.
    pub async fn emit_signal<T>(&self, signal: &Signal<T>, value: T) -> Result<(), ReactiveError>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.emit(signal, value).await
    }

    /// Subscribe to signal changes.
    pub fn subscribe_signal<T>(&self, signal: &Signal<T>) -> SignalStream<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.reactive.subscribe(signal)
    }

    /// Check if a signal is registered.
    pub fn is_signal_registered(&self, signal_id: &SignalId) -> bool {
        self.reactive.is_registered(signal_id)
    }

    // =========================================================================
    // Query-Signal Integration
    // =========================================================================

    /// Invalidate queries affected by a fact change.
    ///
    /// This should be called after committing facts to trigger
    /// reactive updates for bound signals.
    pub async fn invalidate_affected_queries(&self, predicate: &FactPredicate) {
        self.query.invalidate(predicate).await;
    }

    /// Get the dependencies for a signal (if query-bound).
    pub fn signal_dependencies(&self, signal_id: &SignalId) -> Option<Vec<FactPredicate>> {
        self.reactive.query_dependencies(signal_id)
    }
}

impl Default for UnifiedHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for UnifiedHandler {
    fn clone(&self) -> Self {
        Self {
            query: self.query.clone(),
            reactive: self.reactive.clone(),
            capability_context: self.capability_context.clone(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// QueryEffects Implementation
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl QueryEffects for UnifiedHandler {
    async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError> {
        self.query.query(query).await
    }

    async fn query_raw(&self, program: &DatalogProgram) -> Result<DatalogBindings, QueryError> {
        self.query.query_raw(program).await
    }

    fn subscribe<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result> {
        self.query.subscribe(query)
    }

    async fn check_capabilities(&self, capabilities: &[QueryCapability]) -> Result<(), QueryError> {
        self.query.check_capabilities(capabilities).await
    }

    async fn invalidate(&self, predicate: &FactPredicate) {
        self.query.invalidate(predicate).await;
    }

    async fn query_with_isolation<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<Q::Result, QueryError> {
        self.query.query_with_isolation(query, isolation).await
    }

    async fn query_with_stats<Q: Query>(
        &self,
        query: &Q,
    ) -> Result<(Q::Result, QueryStats), QueryError> {
        self.query.query_with_stats(query).await
    }

    async fn query_with_consistency<Q: Query>(
        &self,
        query: &Q,
    ) -> Result<(Q::Result, ConsistencyMap), QueryError> {
        self.query.query_with_consistency(query).await
    }

    async fn query_full<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<(Q::Result, QueryStats), QueryError> {
        self.query.query_full(query, isolation).await
    }

    async fn register_query_binding<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), QueryError> {
        self.query.register_query_binding(signal, query).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ReactiveEffects Implementation
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl ReactiveEffects for UnifiedHandler {
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
        self.reactive.invalidate_queries(changed).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::reactive::Signal;
    use aura_core::query::{DatalogFact, DatalogProgram, DatalogRule, DatalogValue};

    #[derive(Clone)]
    struct ContactQuery;

    impl Query for ContactQuery {
        type Result = Vec<(String, String)>;

        fn to_datalog(&self) -> DatalogProgram {
            let head = DatalogFact::new(
                "result",
                vec![DatalogValue::var("a"), DatalogValue::var("b")],
            );
            let body = vec![DatalogFact::new(
                "contact",
                vec![DatalogValue::var("a"), DatalogValue::var("b")],
            )];
            DatalogProgram::new(vec![DatalogRule { head, body }])
        }

        fn required_capabilities(&self) -> Vec<QueryCapability> {
            vec![]
        }

        fn dependencies(&self) -> Vec<FactPredicate> {
            vec![FactPredicate::new("contact")]
        }

        fn parse(
            bindings: DatalogBindings,
        ) -> Result<Self::Result, aura_core::query::QueryParseError> {
            let mut out = Vec::new();
            for row in bindings.rows {
                let a = row
                    .get("arg0")
                    .and_then(|v| match v {
                        DatalogValue::String(s) | DatalogValue::Symbol(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let b = row
                    .get("arg1")
                    .and_then(|v| match v {
                        DatalogValue::String(s) | DatalogValue::Symbol(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                out.push((a, b));
            }
            Ok(out)
        }
    }

    #[tokio::test]
    async fn test_unified_handler_creation() {
        let handler = UnifiedHandler::new();
        assert!(handler.capability_context.is_none());
    }

    #[tokio::test]
    async fn test_capability_context() {
        let mut handler = UnifiedHandler::new();

        handler.set_capability_context(vec![1, 2, 3]);
        assert!(handler.capability_context.is_some());

        handler.clear_capability_context();
        assert!(handler.capability_context.is_none());
    }

    #[tokio::test]
    async fn test_signal_operations() {
        let handler = UnifiedHandler::new();
        let signal: Signal<u32> = Signal::new("test_counter");

        // Register signal
        handler.register_signal(&signal, 0).await.unwrap();

        // Read initial value
        let value = handler.read_signal(&signal).await.unwrap();
        assert_eq!(value, 0);

        // Emit new value
        handler.emit_signal(&signal, 42).await.unwrap();

        // Read updated value
        let value = handler.read_signal(&signal).await.unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn test_fact_operations() {
        let handler = UnifiedHandler::new();

        // Add facts
        handler
            .add_fact("user", vec!["alice".to_string(), "admin".to_string()])
            .await;
        handler
            .add_fact("user", vec!["bob".to_string(), "member".to_string()])
            .await;

        // Facts should be available for queries
        // (Full query test requires query implementation)
    }

    #[tokio::test]
    async fn test_commit_fact_triggers_invalidation() {
        let handler = UnifiedHandler::new();

        // Commit a fact (should trigger invalidation)
        handler
            .commit_fact("contact", vec!["alice".to_string(), "Alice".to_string()])
            .await;

        // In a full implementation, bound signals would be re-evaluated
    }

    #[tokio::test]
    async fn test_query_bound_signal_updates_on_commit() {
        let handler = UnifiedHandler::new();
        let signal: Signal<Vec<(String, String)>> = Signal::new("contact_query");

        handler
            .register_query_signal(&signal, ContactQuery)
            .await
            .unwrap();

        let initial = handler.read_signal(&signal).await.unwrap();
        assert!(initial.is_empty());

        handler
            .commit_fact("contact", vec!["alice".to_string(), "bob".to_string()])
            .await;

        let updated = handler.read_signal(&signal).await.unwrap();
        assert_eq!(updated, vec![("alice".to_string(), "bob".to_string())]);
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let handler1 = UnifiedHandler::new();
        let signal: Signal<String> = Signal::new("shared");

        handler1
            .register_signal(&signal, "initial".to_string())
            .await
            .unwrap();

        let handler2 = handler1.clone();

        // Both handlers should see the same value
        let v1 = handler1.read_signal(&signal).await.unwrap();
        let v2 = handler2.read_signal(&signal).await.unwrap();
        assert_eq!(v1, v2);

        // Update via handler2
        handler2
            .emit_signal(&signal, "updated".to_string())
            .await
            .unwrap();

        // Both handlers see the update
        let v1 = handler1.read_signal(&signal).await.unwrap();
        let v2 = handler2.read_signal(&signal).await.unwrap();
        assert_eq!(v1, "updated");
        assert_eq!(v2, "updated");
    }
}
