//! Query Handler Implementation
//!
//! The production query effect handler that bridges typed queries with the
//! Datalog execution engine (Biscuit).

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};

use aura_core::domain::journal::FactValue;
use aura_core::effects::{
    indexed::IndexedJournalEffects,
    query::{QueryEffects, QueryError, QuerySubscription},
    reactive::{ReactiveEffects, Signal},
};
use aura_core::query::{
    ConsensusId, DatalogBindings, DatalogProgram, FactPredicate, Query, QueryCapability,
    QueryIsolation, QueryStats,
};
use aura_core::{Hash32, ResourceScope};
use aura_core::effects::reactive::SignalId;

use aura_effects::database::query::AuraQuery;
use crate::effects::reactive::ReactiveHandler;

use super::datalog::{format_rule, parse_fact_to_row};

// ─────────────────────────────────────────────────────────────────────────────
// Query Registration
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
trait QueryRegistration: Send + Sync {
    fn signal_id(&self) -> &SignalId;
    fn dependencies(&self) -> &[FactPredicate];
    async fn refresh(&self, handler: &QueryHandler) -> Result<(), QueryError>;
}

struct QueryRegistrationImpl<Q: Query> {
    signal: Signal<Q::Result>,
    query: Q,
    deps: Vec<FactPredicate>,
}

#[async_trait]
impl<Q: Query> QueryRegistration for QueryRegistrationImpl<Q> {
    fn signal_id(&self) -> &SignalId {
        self.signal.id()
    }

    fn dependencies(&self) -> &[FactPredicate] {
        &self.deps
    }

    async fn refresh(&self, handler: &QueryHandler) -> Result<(), QueryError> {
        let result = handler.query(&self.query).await?;
        handler
            .reactive
            .emit(&self.signal, result)
            .await
            .map_err(|e| QueryError::execution_error(e.to_string()))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query Facts Store
// ─────────────────────────────────────────────────────────────────────────────

/// Facts available for querying (loaded from journal).
#[derive(Debug, Default, Clone)]
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

/// Policy for capability enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityPolicy {
    /// Allow all queries regardless of granted capabilities.
    AllowAll,
    /// Deny queries unless capabilities are explicitly granted.
    DenyUnlessGranted,
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        CapabilityPolicy::DenyUnlessGranted
    }
}

/// Capability checker for query authorization.
///
/// Uses a strict-by-default model: queries pass only if all required
/// capabilities are granted (unless policy is AllowAll).
#[derive(Debug)]
pub(super) struct CapabilityChecker {
    /// Granted capabilities (derived from Biscuit tokens in production)
    granted: Vec<QueryCapability>,
    /// Capability enforcement policy
    policy: CapabilityPolicy,
}

impl CapabilityChecker {
    /// Check if a capability is granted.
    ///
    /// Returns true if:
    /// - Policy is AllowAll, OR
    /// - The requested capability matches a granted capability
    pub fn check(&self, cap: &QueryCapability) -> bool {
        if self.policy == CapabilityPolicy::AllowAll {
            return true;
        }

        self.granted
            .iter()
            .any(|g| g.resource == cap.resource && g.action == cap.action)
    }

    /// Grant a capability for subsequent queries.
    pub fn grant(&mut self, cap: QueryCapability) {
        self.granted.push(cap);
    }

    /// Set capability enforcement policy.
    pub fn set_policy(&mut self, policy: CapabilityPolicy) {
        self.policy = policy;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Consensus Tracker
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks consensus completion for ReadCommitted isolation.
///
/// When a consensus instance completes, call `mark_completed()` to allow
/// queries waiting on that consensus ID to proceed.
#[derive(Debug)]
pub struct ConsensusTracker {
    /// Set of completed consensus IDs
    completed: HashSet<ConsensusId>,
    /// Broadcast sender for completion notifications
    notify_tx: broadcast::Sender<ConsensusId>,
}

impl Default for ConsensusTracker {
    fn default() -> Self {
        let (notify_tx, _) = broadcast::channel(256);
        Self {
            completed: HashSet::new(),
            notify_tx,
        }
    }
}

impl ConsensusTracker {
    /// Mark a consensus instance as completed.
    ///
    /// This wakes up any queries waiting for this consensus ID.
    pub fn mark_completed(&mut self, id: ConsensusId) {
        self.completed.insert(id);
        // Ignore send errors - no receivers is fine
        let _ = self.notify_tx.send(id);
    }

    /// Check if a consensus instance has completed.
    pub fn is_completed(&self, id: &ConsensusId) -> bool {
        self.completed.contains(id)
    }

    /// Subscribe to consensus completion notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<ConsensusId> {
        self.notify_tx.subscribe()
    }

    /// Check if all specified consensus IDs have completed.
    pub fn all_completed(&self, ids: &[ConsensusId]) -> bool {
        ids.iter().all(|id| self.completed.contains(id))
    }

    /// Convert from aura_core::query::ConsensusId to local representation
    pub fn from_core_id(id: &aura_core::query::ConsensusId) -> ConsensusId {
        ConsensusId::new(id.0)
    }

    /// Convert to aura_core::query::ConsensusId
    pub fn to_core_id(id: &ConsensusId) -> aura_core::query::ConsensusId {
        aura_core::query::ConsensusId::new(*id.as_bytes())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot Store
// ─────────────────────────────────────────────────────────────────────────────

/// Stores historical fact snapshots for Snapshot isolation.
///
/// Snapshots are identified by prestate hash and contain a frozen copy
/// of facts at that point in time. Old snapshots may be garbage collected
/// to limit memory usage.
#[derive(Debug, Default)]
pub struct SnapshotStore {
    /// Snapshots keyed by prestate hash
    snapshots: HashMap<Hash32, QueryFacts>,
    /// Maximum number of snapshots to retain
    max_snapshots: usize,
    /// Order of snapshot creation (for LRU eviction)
    creation_order: Vec<Hash32>,
}

impl SnapshotStore {
    /// Create a new snapshot store with the specified capacity.
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: HashMap::new(),
            max_snapshots,
            creation_order: Vec::new(),
        }
    }

    /// Create a snapshot of the current facts at the given prestate hash.
    pub fn create_snapshot(&mut self, prestate_hash: Hash32, facts: &QueryFacts) {
        // Clone the facts for the snapshot
        let snapshot = QueryFacts {
            facts: facts.facts.clone(),
        };

        // Evict oldest snapshot if at capacity
        while self.snapshots.len() >= self.max_snapshots && !self.creation_order.is_empty() {
            let oldest = self.creation_order.remove(0);
            self.snapshots.remove(&oldest);
        }

        self.snapshots.insert(prestate_hash, snapshot);
        self.creation_order.push(prestate_hash);
    }

    /// Get a snapshot by prestate hash.
    pub fn get_snapshot(&self, prestate_hash: &Hash32) -> Option<&QueryFacts> {
        self.snapshots.get(prestate_hash)
    }

    /// Check if a snapshot exists.
    pub fn has_snapshot(&self, prestate_hash: &Hash32) -> bool {
        self.snapshots.contains_key(prestate_hash)
    }

    /// Remove a specific snapshot.
    pub fn remove_snapshot(&mut self, prestate_hash: &Hash32) {
        self.snapshots.remove(prestate_hash);
        self.creation_order.retain(|h| h != prestate_hash);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pending Consensus Tracker
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks pending consensus instances by resource scope for ReadLatest isolation.
#[derive(Debug, Default)]
pub struct PendingConsensusTracker {
    /// Pending consensus IDs by resource scope
    pending_by_scope: HashMap<ResourceScope, HashSet<ConsensusId>>,
}

impl PendingConsensusTracker {
    /// Register a pending consensus instance for a resource scope.
    pub fn register_pending(&mut self, scope: ResourceScope, id: ConsensusId) {
        self.pending_by_scope.entry(scope).or_default().insert(id);
    }

    /// Mark a consensus instance as completed across all scopes.
    pub fn mark_completed(&mut self, id: &ConsensusId) {
        for pending_set in self.pending_by_scope.values_mut() {
            pending_set.remove(id);
        }
    }

    /// Get all pending consensus IDs for a scope.
    pub fn pending_for_scope(&self, scope: &ResourceScope) -> Vec<ConsensusId> {
        self.pending_by_scope
            .get(scope)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
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

/// Default timeout for waiting on consensus completion (30 seconds).
const DEFAULT_CONSENSUS_TIMEOUT: Duration = Duration::from_secs(30);

/// Default maximum number of snapshots to retain.
const DEFAULT_MAX_SNAPSHOTS: usize = 100;

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
///
/// # Query Isolation
///
/// Supports four isolation levels:
/// - `ReadUncommitted`: Immediate query against current CRDT state
/// - `ReadCommitted`: Wait for specific consensus IDs before querying
/// - `Snapshot`: Query against historical state by prestate hash
/// - `ReadLatest`: Wait for all pending consensus in a scope
pub struct QueryHandler {
    /// Reactive handler for signal-based subscriptions
    reactive: Arc<ReactiveHandler>,
    /// Facts loaded into the query engine (populated from journal)
    pub(super) facts: Arc<RwLock<QueryFacts>>,
    /// Capability checker for authorization
    capabilities: Arc<RwLock<CapabilityChecker>>,
    /// Optional indexed journal for efficient lookups
    indexed_journal: Option<Arc<dyn IndexedJournalEffects + Send + Sync>>,
    /// Consensus completion tracker for ReadCommitted isolation
    consensus_tracker: Arc<RwLock<ConsensusTracker>>,
    /// Snapshot store for historical state queries
    snapshot_store: Arc<RwLock<SnapshotStore>>,
    /// Pending consensus tracker for ReadLatest isolation
    pending_consensus: Arc<RwLock<PendingConsensusTracker>>,
    /// Registered query bindings for reactive refresh
    query_bindings: Arc<RwLock<HashMap<SignalId, Box<dyn QueryRegistration>>>>,
    /// Timeout for waiting on consensus
    consensus_timeout: Duration,
}

impl QueryHandler {
    /// Create a new query handler with a reactive handler for subscriptions.
    pub fn new(reactive: Arc<ReactiveHandler>) -> Self {
        Self::new_with_policy(reactive, CapabilityPolicy::default())
    }

    /// Create a new query handler with an explicit capability policy.
    pub fn new_with_policy(reactive: Arc<ReactiveHandler>, policy: CapabilityPolicy) -> Self {
        Self {
            reactive,
            facts: Arc::new(RwLock::new(QueryFacts::default())),
            capabilities: Arc::new(RwLock::new(CapabilityChecker {
                granted: Vec::new(),
                policy,
            })),
            indexed_journal: None,
            consensus_tracker: Arc::new(RwLock::new(ConsensusTracker::default())),
            snapshot_store: Arc::new(RwLock::new(SnapshotStore::new(DEFAULT_MAX_SNAPSHOTS))),
            pending_consensus: Arc::new(RwLock::new(PendingConsensusTracker::default())),
            query_bindings: Arc::new(RwLock::new(HashMap::new())),
            consensus_timeout: DEFAULT_CONSENSUS_TIMEOUT,
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
        Self::with_indexed_journal_with_policy(
            reactive,
            indexed_journal,
            CapabilityPolicy::default(),
        )
    }

    /// Create a query handler with indexed journal support and explicit policy.
    pub fn with_indexed_journal_with_policy(
        reactive: Arc<ReactiveHandler>,
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
        policy: CapabilityPolicy,
    ) -> Self {
        Self {
            reactive,
            facts: Arc::new(RwLock::new(QueryFacts::default())),
            capabilities: Arc::new(RwLock::new(CapabilityChecker {
                granted: Vec::new(),
                policy,
            })),
            indexed_journal: Some(indexed_journal),
            consensus_tracker: Arc::new(RwLock::new(ConsensusTracker::default())),
            snapshot_store: Arc::new(RwLock::new(SnapshotStore::new(DEFAULT_MAX_SNAPSHOTS))),
            pending_consensus: Arc::new(RwLock::new(PendingConsensusTracker::default())),
            query_bindings: Arc::new(RwLock::new(HashMap::new())),
            consensus_timeout: DEFAULT_CONSENSUS_TIMEOUT,
        }
    }

    /// Set the consensus wait timeout.
    pub fn with_consensus_timeout(mut self, timeout: Duration) -> Self {
        self.consensus_timeout = timeout;
        self
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

    /// Set capability enforcement policy.
    ///
    /// Use AllowAll in tests or offline scenarios that do not enforce Biscuit checks.
    pub async fn set_capability_policy(&self, policy: CapabilityPolicy) {
        let mut checker = self.capabilities.write().await;
        checker.set_policy(policy);
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

    /// Register a query binding for reactive refresh.
    ///
    /// This stores the query and signal mapping and emits the initial query result.
    pub async fn register_query_binding<Q: Query>(
        &self,
        signal: &Signal<Q::Result>,
        query: Q,
    ) -> Result<(), QueryError> {
        let deps = query.dependencies();
        let registration = QueryRegistrationImpl {
            signal: signal.clone(),
            query: query.clone(),
            deps,
        };

        self.query_bindings
            .write()
            .await
            .insert(signal.id().clone(), Box::new(registration));

        let result = self.query(&query).await?;
        self.reactive
            .emit(signal, result)
            .await
            .map_err(|e| QueryError::execution_error(e.to_string()))?;

        Ok(())
    }

    async fn refresh_queries_for_predicate(&self, predicate: &FactPredicate) {
        let bindings = self.query_bindings.read().await;
        for registration in bindings.values() {
            if registration
                .dependencies()
                .iter()
                .any(|dep| dep.matches(predicate))
            {
                if let Err(err) = registration.refresh(self).await {
                    tracing::warn!(
                        error = %err,
                        signal_id = %registration.signal_id(),
                        "Failed to refresh query-bound signal"
                    );
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Isolation Infrastructure Management
    // ─────────────────────────────────────────────────────────────────────────

    /// Mark a consensus instance as completed.
    ///
    /// Call this when a consensus operation completes to wake up any
    /// queries waiting with `ReadCommitted` isolation.
    pub async fn mark_consensus_completed(&self, id: ConsensusId) {
        let mut tracker = self.consensus_tracker.write().await;
        tracker.mark_completed(id);

        // Also update pending consensus tracker
        let mut pending = self.pending_consensus.write().await;
        pending.mark_completed(&id);
    }

    /// Register a pending consensus instance for a resource scope.
    ///
    /// Call this when a new consensus operation is started to track it
    /// for `ReadLatest` isolation.
    pub async fn register_pending_consensus(&self, scope: ResourceScope, id: ConsensusId) {
        let mut pending = self.pending_consensus.write().await;
        pending.register_pending(scope, id);
    }

    /// Create a snapshot of current facts at a specific prestate hash.
    ///
    /// Call this when a consensus operation starts to enable `Snapshot`
    /// isolation queries against this state.
    pub async fn create_snapshot(&self, prestate_hash: Hash32) {
        let facts = self.facts.read().await;
        let mut store = self.snapshot_store.write().await;
        store.create_snapshot(prestate_hash, &facts);
    }

    /// Remove a snapshot (e.g., after garbage collection window).
    pub async fn remove_snapshot(&self, prestate_hash: Hash32) {
        let mut store = self.snapshot_store.write().await;
        store.remove_snapshot(&prestate_hash);
    }

    /// Check if a snapshot exists for a prestate hash.
    pub async fn has_snapshot(&self, prestate_hash: &Hash32) -> bool {
        let store = self.snapshot_store.read().await;
        store.has_snapshot(prestate_hash)
    }

    /// Wait for specific consensus instances to complete.
    ///
    /// Returns Ok(()) when all consensus IDs have completed, or an error
    /// if the timeout is reached.
    async fn wait_for_consensus(&self, ids: &[ConsensusId]) -> Result<(), QueryError> {
        if ids.is_empty() {
            return Ok(());
        }

        // Check if already completed
        {
            let tracker = self.consensus_tracker.read().await;
            if tracker.all_completed(ids) {
                return Ok(());
            }
        }

        // Subscribe and wait
        let mut receiver = {
            let tracker = self.consensus_tracker.read().await;
            tracker.subscribe()
        };

        let deadline = tokio::time::Instant::now() + self.consensus_timeout;

        loop {
            // Check current state
            {
                let tracker = self.consensus_tracker.read().await;
                if tracker.all_completed(ids) {
                    return Ok(());
                }
            }

            // Wait for next notification or timeout
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                // Return timeout error with the first incomplete consensus ID
                let tracker = self.consensus_tracker.read().await;
                for id in ids {
                    if !tracker.is_completed(id) {
                        return Err(QueryError::consensus_timeout(ConsensusTracker::to_core_id(
                            id,
                        )));
                    }
                }
                return Ok(()); // All completed while checking
            }

            match tokio::time::timeout(remaining, receiver.recv()).await {
                Ok(Ok(_completed_id)) => {
                    // Check if we're done
                    continue;
                }
                Ok(Err(_)) => {
                    // Channel closed - all senders dropped
                    return Err(QueryError::internal(
                        "Consensus tracker channel closed unexpectedly",
                    ));
                }
                Err(_) => {
                    // Timeout
                    let tracker = self.consensus_tracker.read().await;
                    for id in ids {
                        if !tracker.is_completed(id) {
                            return Err(QueryError::consensus_timeout(
                                ConsensusTracker::to_core_id(id),
                            ));
                        }
                    }
                    return Ok(());
                }
            }
        }
    }

    /// Wait for all pending consensus in a resource scope to complete.
    async fn wait_for_scope_consensus(&self, scope: &ResourceScope) -> Result<(), QueryError> {
        let pending_ids = {
            let pending = self.pending_consensus.read().await;
            pending.pending_for_scope(scope)
        };

        if pending_ids.is_empty() {
            return Ok(());
        }

        self.wait_for_consensus(&pending_ids).await
    }

    /// Execute a query against a specific snapshot.
    async fn execute_snapshot_query<Q: Query>(
        &self,
        query: &Q,
        prestate_hash: &Hash32,
    ) -> Result<Q::Result, QueryError> {
        // Check capabilities first
        let required = query.required_capabilities();
        self.check_capabilities(&required).await?;

        // Get the snapshot
        let snapshot = {
            let store = self.snapshot_store.read().await;
            store
                .get_snapshot(prestate_hash)
                .ok_or_else(|| QueryError::snapshot_not_available(*prestate_hash))?
                .clone()
        };

        // Execute against snapshot facts
        let program = query.to_datalog();
        let bindings = self.execute_program_with_facts(&program, &snapshot).await?;

        Q::parse(bindings).map_err(QueryError::from)
    }

    /// Execute a Datalog program against a specific facts store.
    async fn execute_program_with_facts(
        &self,
        program: &DatalogProgram,
        facts: &QueryFacts,
    ) -> Result<DatalogBindings, QueryError> {
        let mut aura_query = AuraQuery::new();

        // Load facts from the provided store
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
                    tracing::warn!(rule = %rule_string, error = %e, "Rule execution failed");
                }
            }
        }

        Ok(DatalogBindings { rows: all_rows })
    }

    /// Execute a Datalog program and return bindings.
    async fn execute_program(
        &self,
        program: &DatalogProgram,
    ) -> Result<DatalogBindings, QueryError> {
        let facts = self.facts.read().await;
        self.execute_program_with_facts(program, &facts).await
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
            consensus_tracker: self.consensus_tracker.clone(),
            snapshot_store: self.snapshot_store.clone(),
            pending_consensus: self.pending_consensus.clone(),
            query_bindings: self.query_bindings.clone(),
            consensus_timeout: self.consensus_timeout,
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
        self.refresh_queries_for_predicate(predicate).await;
    }

    async fn query_with_isolation<Q: Query>(
        &self,
        query: &Q,
        isolation: QueryIsolation,
    ) -> Result<Q::Result, QueryError> {
        match &isolation {
            QueryIsolation::ReadUncommitted => {
                // Standard query execution - immediate against current CRDT state
                self.query(query).await
            }
            QueryIsolation::ReadCommitted { wait_for } => {
                // Convert from aura_core::query::ConsensusId to local ConsensusId
                let local_ids: Vec<ConsensusId> = wait_for
                    .iter()
                    .map(ConsensusTracker::from_core_id)
                    .collect();

                // Wait for all specified consensus instances to complete
                self.wait_for_consensus(&local_ids).await?;

                // Now execute the query against updated state
                self.query(query).await
            }
            QueryIsolation::Snapshot { prestate_hash } => {
                // Execute query against historical snapshot state
                self.execute_snapshot_query(query, prestate_hash).await
            }
            QueryIsolation::ReadLatest { scope } => {
                // Wait for all pending consensus in the specified scope
                self.wait_for_scope_consensus(scope).await?;

                // Execute query against updated state
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
    async fn test_check_capabilities_denied_by_default() {
        let handler = QueryHandler::default();

        let cap = QueryCapability::read("messages");
        let result = handler.check_capabilities(&[cap]).await;
        assert!(result.is_err());
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

    // ─────────────────────────────────────────────────────────────────────────
    // Isolation Infrastructure Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_consensus_tracker_completion() {
        let handler = QueryHandler::default();

        let consensus_id = ConsensusId::new([1u8; 32]);

        // Not completed initially
        {
            let tracker = handler.consensus_tracker.read().await;
            assert!(!tracker.is_completed(&consensus_id));
        }

        // Mark completed
        handler.mark_consensus_completed(consensus_id).await;

        // Should be completed now
        {
            let tracker = handler.consensus_tracker.read().await;
            assert!(tracker.is_completed(&consensus_id));
        }
    }

    #[tokio::test]
    async fn test_snapshot_create_and_retrieve() {
        let handler = QueryHandler::default();

        // Add some facts
        handler.add_fact("user", vec!["alice".to_string()]).await;
        handler.add_fact("user", vec!["bob".to_string()]).await;

        // Create snapshot
        let prestate_hash = Hash32([42u8; 32]);
        handler.create_snapshot(prestate_hash).await;

        // Verify snapshot exists
        assert!(handler.has_snapshot(&prestate_hash).await);

        // Verify snapshot contains facts
        {
            let store = handler.snapshot_store.read().await;
            let snapshot = store.get_snapshot(&prestate_hash).unwrap();
            assert_eq!(snapshot.len(), 2);
        }
    }

    #[tokio::test]
    async fn test_snapshot_removal() {
        let handler = QueryHandler::default();

        // Create snapshot
        let prestate_hash = Hash32([42u8; 32]);
        handler.create_snapshot(prestate_hash).await;

        assert!(handler.has_snapshot(&prestate_hash).await);

        // Remove snapshot
        handler.remove_snapshot(prestate_hash).await;

        assert!(!handler.has_snapshot(&prestate_hash).await);
    }

    #[tokio::test]
    async fn test_pending_consensus_registration() {
        let handler = QueryHandler::default();

        let consensus_id = ConsensusId::new([1u8; 32]);
        let scope = ResourceScope::Authority {
            authority_id: aura_core::AuthorityId::new_from_entropy([1u8; 32]),
            operation: aura_core::AuthorityOp::UpdateTree,
        };

        // Register pending consensus
        handler
            .register_pending_consensus(scope.clone(), consensus_id)
            .await;

        // Check pending
        {
            let pending = handler.pending_consensus.read().await;
            let ids = pending.pending_for_scope(&scope);
            assert_eq!(ids.len(), 1);
            assert_eq!(ids[0], consensus_id);
        }

        // Mark completed
        handler.mark_consensus_completed(consensus_id).await;

        // Should no longer be pending
        {
            let pending = handler.pending_consensus.read().await;
            let ids = pending.pending_for_scope(&scope);
            assert!(ids.is_empty());
        }
    }

    #[tokio::test]
    async fn test_wait_for_consensus_already_completed() {
        let handler = QueryHandler::default();

        let consensus_id = ConsensusId::new([1u8; 32]);

        // Mark completed before waiting
        handler.mark_consensus_completed(consensus_id).await;

        // Wait should return immediately
        let result = handler.wait_for_consensus(&[consensus_id]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_consensus_empty_list() {
        let handler = QueryHandler::default();

        // Waiting for empty list should succeed immediately
        let result = handler.wait_for_consensus(&[]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_consensus_id_conversion_roundtrip() {
        let original_bytes = [42u8; 32];
        let core_id = aura_core::query::ConsensusId::new(original_bytes);

        let local_id = ConsensusTracker::from_core_id(&core_id);
        let back_to_core = ConsensusTracker::to_core_id(&local_id);

        assert_eq!(core_id, back_to_core);
        assert_eq!(local_id.as_bytes(), &original_bytes);
    }
}
