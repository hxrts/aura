//! Query Handler Implementation
//!
//! The production query effect handler that bridges typed queries with the
//! Datalog execution engine (Biscuit).

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use aura_core::domain::journal::FactValue;
use aura_core::effects::{
    indexed::IndexedJournalEffects,
    query::{QueryEffects, QueryError, QuerySubscription},
    reactive::{ReactiveEffects, Signal},
};
use aura_core::query::{
    DatalogBindings, DatalogProgram, FactPredicate, Query, QueryCapability, QueryIsolation,
    QueryStats,
};

use crate::database::query::AuraQuery;
use crate::reactive::ReactiveHandler;

use super::datalog::{format_rule, parse_fact_to_row};

// ─────────────────────────────────────────────────────────────────────────────
// Query Facts Store
// ─────────────────────────────────────────────────────────────────────────────

/// Facts available for querying (loaded from journal).
#[derive(Debug, Default)]
pub(super) struct QueryFacts {
    /// Raw facts keyed by predicate
    facts: HashMap<String, Vec<Vec<String>>>,
}

impl QueryFacts {
    /// Add a fact to the store.
    pub fn add(&mut self, predicate: &str, args: Vec<String>) {
        self.facts
            .entry(predicate.to_string())
            .or_default()
            .push(args);
    }

    /// Clear all facts.
    pub fn clear(&mut self) {
        self.facts.clear();
    }

    /// Check if the store is empty (used in tests).
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    /// Get the total number of facts across all predicates.
    pub fn len(&self) -> usize {
        self.facts.values().map(|v| v.len()).sum()
    }

    /// Load facts into an AuraQuery for execution.
    ///
    /// Facts are loaded best-effort; malformed facts are silently skipped
    /// to allow partial query execution even with imperfect data.
    pub fn load_into(&self, query: &mut AuraQuery) {
        for (predicate, rows) in &self.facts {
            for args in rows {
                let terms: Vec<crate::database::query::FactTerm> =
                    args.iter().map(|s| s.clone().into()).collect();
                let _ = query.add_fact(predicate, terms);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Capability Checker
// ─────────────────────────────────────────────────────────────────────────────

/// Capability checker for query authorization.
///
/// Uses a permissive model: queries pass if no capabilities are explicitly
/// required OR if all required capabilities are granted. In production,
/// capabilities are derived from Biscuit tokens via `AuthorizationEffects`.
#[derive(Debug, Default)]
pub(super) struct CapabilityChecker {
    /// Granted capabilities (derived from Biscuit tokens in production)
    granted: Vec<QueryCapability>,
}

impl CapabilityChecker {
    /// Check if a capability is granted.
    ///
    /// Returns true if:
    /// - No capabilities have been granted (permissive default), OR
    /// - The requested capability matches a granted capability
    pub fn check(&self, cap: &QueryCapability) -> bool {
        self.granted.is_empty()
            || self
                .granted
                .iter()
                .any(|g| g.resource == cap.resource && g.action == cap.action)
    }

    /// Grant a capability for subsequent queries.
    pub fn grant(&mut self, cap: QueryCapability) {
        self.granted.push(cap);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a FactValue to query arguments (Vec<String>).
///
/// This bridges the indexed journal's FactValue to the query engine's string-based
/// argument format.
fn fact_value_to_args(value: &FactValue) -> Vec<String> {
    match value {
        FactValue::String(s) => vec![s.clone()],
        FactValue::Number(n) => vec![n.to_string()],
        FactValue::Bytes(b) => {
            // Convert bytes to hex string manually
            let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
            vec![hex]
        }
        FactValue::Set(s) => s.iter().cloned().collect(),
        FactValue::Nested(fact) => {
            // For nested facts, flatten to string representation
            vec![format!("{:?}", fact)]
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query Handler
// ─────────────────────────────────────────────────────────────────────────────

/// Production query effect handler.
///
/// Implements `QueryEffects` by:
/// - Using `AuraQuery` for Datalog execution via Biscuit
/// - Delegating subscription management to `ReactiveHandler`
/// - Tracking query dependencies for invalidation
/// - Optionally using IndexedJournalEffects for efficient fact lookups
///
/// # Authorization Model
///
/// Capability checks use a permissive model: queries pass if no capabilities
/// are explicitly required OR if all required capabilities are granted.
/// For production use with Biscuit integration, inject capabilities via
/// `grant_capability()` based on the authenticated token.
///
/// # Indexed Journal Integration
///
/// When an indexed journal is provided via `with_indexed_journal()`, the handler:
/// - Uses bloom filter pre-filtering to skip queries for non-existent predicates
/// - Can load facts directly from the index for O(log n) lookups
pub struct QueryHandler {
    /// Reactive handler for signal-based subscriptions
    reactive: Arc<ReactiveHandler>,
    /// Facts loaded into the query engine (populated from journal)
    pub(super) facts: Arc<RwLock<QueryFacts>>,
    /// Capability checker for authorization
    capabilities: Arc<RwLock<CapabilityChecker>>,
    /// Optional indexed journal for efficient lookups
    indexed_journal: Option<Arc<dyn IndexedJournalEffects + Send + Sync>>,
}

impl QueryHandler {
    /// Create a new query handler with a reactive handler for subscriptions.
    pub fn new(reactive: Arc<ReactiveHandler>) -> Self {
        Self {
            reactive,
            facts: Arc::new(RwLock::new(QueryFacts::default())),
            capabilities: Arc::new(RwLock::new(CapabilityChecker::default())),
            indexed_journal: None,
        }
    }

    /// Create a query handler with indexed journal support.
    ///
    /// When an indexed journal is provided, the handler can:
    /// - Load facts from the index for efficient O(log n) predicate lookups
    /// - Use bloom filter to quickly check if facts exist before expensive queries
    pub fn with_indexed_journal(
        reactive: Arc<ReactiveHandler>,
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
    ) -> Self {
        Self {
            reactive,
            facts: Arc::new(RwLock::new(QueryFacts::default())),
            capabilities: Arc::new(RwLock::new(CapabilityChecker::default())),
            indexed_journal: Some(indexed_journal),
        }
    }

    /// Add a fact to the query store.
    ///
    /// Facts added here are available for all subsequent queries.
    /// In production, facts come from the journal via a fact stream.
    pub async fn add_fact(&self, predicate: &str, args: Vec<String>) {
        let mut facts = self.facts.write().await;
        facts.add(predicate, args);
    }

    /// Add multiple facts at once.
    pub async fn add_facts(&self, entries: Vec<(String, Vec<String>)>) {
        let mut facts = self.facts.write().await;
        for (predicate, args) in entries {
            facts.add(&predicate, args);
        }
    }

    /// Clear all facts.
    pub async fn clear_facts(&self) {
        let mut facts = self.facts.write().await;
        facts.clear();
    }

    /// Grant a capability for subsequent queries.
    ///
    /// In production, capabilities are typically derived from Biscuit tokens
    /// after authentication. This method allows manual capability injection
    /// for testing or scenarios where capabilities are determined externally.
    pub async fn grant_capability(&self, cap: QueryCapability) {
        let mut checker = self.capabilities.write().await;
        checker.grant(cap);
    }

    /// Load facts from the indexed journal for a specific predicate.
    ///
    /// This uses O(log n) B-tree lookup to fetch all facts matching the predicate.
    /// Facts are loaded into the local store for query execution.
    ///
    /// Returns the number of facts loaded.
    pub async fn load_facts_for_predicate(&self, predicate: &str) -> Result<usize, QueryError> {
        let Some(ref indexed) = self.indexed_journal else {
            return Ok(0); // No indexed journal configured
        };

        let indexed_facts = indexed
            .facts_by_predicate(predicate)
            .await
            .map_err(|e| QueryError::execution_error(e.to_string()))?;

        let count = indexed_facts.len();
        let mut facts = self.facts.write().await;

        for fact in indexed_facts {
            let args = fact_value_to_args(&fact.value);
            facts.add(&fact.predicate, args);
        }

        Ok(count)
    }

    /// Check if facts with the given predicate and value might exist.
    ///
    /// Uses the bloom filter for O(1) membership testing.
    /// Returns `true` if facts might exist, `false` if definitely not present.
    ///
    /// This is useful for early query rejection - if returns `false`, the query
    /// can be skipped entirely.
    pub fn might_contain_fact(&self, predicate: &str, value: &FactValue) -> bool {
        match &self.indexed_journal {
            Some(indexed) => indexed.might_contain(predicate, value),
            None => true, // Without indexed journal, assume facts might exist
        }
    }

    /// Execute a Datalog program and return bindings.
    async fn execute_program(
        &self,
        program: &DatalogProgram,
    ) -> Result<DatalogBindings, QueryError> {
        let facts = self.facts.read().await;
        let mut aura_query = AuraQuery::new();

        // Load all facts
        facts.load_into(&mut aura_query);

        // Execute each rule and collect results
        let mut all_rows = Vec::new();

        for rule in &program.rules {
            let rule_string = format_rule(rule);

            match aura_query.query(&rule_string) {
                Ok(result) => {
                    for fact_strings in result.facts {
                        let row = parse_fact_to_row(&fact_strings);
                        all_rows.push(row);
                    }
                }
                Err(e) => {
                    // Log but continue - some rules may fail
                    tracing::warn!(rule = %rule_string, error = %e, "Rule execution failed");
                }
            }
        }

        Ok(DatalogBindings { rows: all_rows })
    }
}

impl Default for QueryHandler {
    fn default() -> Self {
        Self::new(Arc::new(ReactiveHandler::new()))
    }
}

impl Clone for QueryHandler {
    fn clone(&self) -> Self {
        Self {
            reactive: self.reactive.clone(),
            facts: self.facts.clone(),
            capabilities: self.capabilities.clone(),
            indexed_journal: self.indexed_journal.clone(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// QueryEffects Implementation
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl QueryEffects for QueryHandler {
    async fn query<Q: Query>(&self, query: &Q) -> Result<Q::Result, QueryError> {
        // Step 1: Check capabilities
        let required = query.required_capabilities();
        self.check_capabilities(&required).await?;

        // Step 2: Compile query to Datalog
        let program = query.to_datalog();

        // Step 3: Execute the program
        let bindings = self.execute_program(&program).await?;

        // Step 4: Parse results
        Q::parse(bindings).map_err(QueryError::from)
    }

    async fn query_raw(&self, program: &DatalogProgram) -> Result<DatalogBindings, QueryError> {
        self.execute_program(program).await
    }

    fn subscribe<Q: Query>(&self, query: &Q) -> QuerySubscription<Q::Result> {
        // Create a signal name for this query instance
        let signal_name = format!("query:{}:{}", std::any::type_name::<Q>(), query.query_id());
        let signal: Signal<Q::Result> = Signal::new(signal_name.as_str());

        // Get the signal stream
        let stream = self.reactive.subscribe(&signal);

        // Create subscription with query dependencies tracked
        QuerySubscription::new(stream, query.query_id())
    }

    async fn check_capabilities(&self, capabilities: &[QueryCapability]) -> Result<(), QueryError> {
        let checker = self.capabilities.read().await;

        for cap in capabilities {
            if !checker.check(cap) {
                return Err(QueryError::missing_capability(cap));
            }
        }

        Ok(())
    }

    async fn invalidate(&self, predicate: &FactPredicate) {
        self.reactive.invalidate_queries(predicate).await;
    }

    async fn query_with_isolation<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<Q::Result, QueryError> {
        // For now, only ReadUncommitted is fully supported
        // Other isolation levels would require consensus integration
        match &isolation {
            QueryIsolation::ReadUncommitted => {
                // Standard query execution
                self.query(query).await
            }
            QueryIsolation::ReadCommitted { .. } => {
                // TODO: Wait for consensus instances to complete
                // For now, fall back to ReadUncommitted with a warning
                tracing::warn!(
                    "ReadCommitted isolation not yet fully implemented, using ReadUncommitted"
                );
                self.query(query).await
            }
            QueryIsolation::Snapshot { prestate_hash } => {
                // TODO: Query against historical state
                Err(QueryError::snapshot_not_available(*prestate_hash))
            }
            QueryIsolation::ReadLatest { .. } => {
                // TODO: Wait for all pending consensus
                tracing::warn!(
                    "ReadLatest isolation not yet fully implemented, using ReadUncommitted"
                );
                self.query(query).await
            }
        }
    }

    #[allow(clippy::disallowed_methods)] // Instant::now() legitimate for internal performance measurement
    async fn query_with_stats<Q: Query>(
        &self,
        query: &Q,
    ) -> Result<(Q::Result, QueryStats), QueryError> {
        let start = std::time::Instant::now();

        // Execute the query
        let result = self.query(query).await?;

        // Build stats
        let stats = QueryStats::new(start.elapsed())
            .with_facts_scanned(self.facts.read().await.len())
            .with_isolation(QueryIsolation::ReadUncommitted);

        Ok((result, stats))
    }

    #[allow(clippy::disallowed_methods)] // Instant::now() legitimate for internal performance measurement
    async fn query_full<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<(Q::Result, QueryStats), QueryError> {
        let start = std::time::Instant::now();

        // Execute with isolation
        let result = self.query_with_isolation(query, isolation.clone()).await?;

        // Build stats
        let stats = QueryStats::new(start.elapsed())
            .with_facts_scanned(self.facts.read().await.len())
            .with_isolation(isolation);

        Ok((result, stats))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_creation() {
        let handler = QueryHandler::default();
        assert!(handler.facts.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_add_fact() {
        let handler = QueryHandler::default();

        handler
            .add_fact("user", vec!["alice".to_string(), "admin".to_string()])
            .await;

        let facts = handler.facts.read().await;
        assert!(!facts.is_empty());
    }

    #[tokio::test]
    async fn test_add_multiple_facts() {
        let handler = QueryHandler::default();

        handler
            .add_facts(vec![
                ("user".to_string(), vec!["alice".to_string()]),
                ("user".to_string(), vec!["bob".to_string()]),
                ("role".to_string(), vec!["admin".to_string()]),
            ])
            .await;

        let facts = handler.facts.read().await;
        assert!(!facts.is_empty());
    }

    #[tokio::test]
    async fn test_clear_facts() {
        let handler = QueryHandler::default();

        handler.add_fact("test", vec!["value".to_string()]).await;
        handler.clear_facts().await;

        assert!(handler.facts.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_check_capabilities_empty() {
        let handler = QueryHandler::default();

        // Empty capabilities should pass (no restrictions)
        let result = handler.check_capabilities(&[]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_grant_capability() {
        let handler = QueryHandler::default();

        let cap = QueryCapability::read("messages");
        handler.grant_capability(cap.clone()).await;

        // Should pass now
        let result = handler.check_capabilities(&[cap]).await;
        assert!(result.is_ok());
    }
}
