//! # Query System
//!
//! Unified query abstraction that bridges the algebraic effect system with
//! Datalog-based journal queries and Biscuit authorization.
//!
//! # Architecture
//!
//! Queries are the "read" side of the system:
//! ```text
//! Intent → Journal (write) → Facts (CRDT)
//! Query  → Journal (read)  → Datalog → Result
//! ```
//!
//! Each query:
//! - Compiles to a Datalog program
//! - Declares required Biscuit capabilities
//! - Specifies fact predicates for invalidation tracking
//! - Parses Datalog bindings to typed results
//!
//! # Integration with Effects
//!
//! Queries are executed through `QueryEffects` (in `effects/query.rs`).
//! Signals can be bound to queries via `ReactiveEffects`, enabling automatic
//! updates when underlying facts change.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

use crate::domain::ConsistencyMap;
use crate::time::PhysicalTime;
use crate::Hash32;
use crate::ResourceScope;

// ─────────────────────────────────────────────────────────────────────────────
// Query Isolation
// ─────────────────────────────────────────────────────────────────────────────

/// Query isolation levels for consistency requirements.
///
/// Aura's journal is eventually consistent via CRDT merge. These isolation levels
/// let queries specify their consistency requirements:
///
/// - `ReadUncommitted`: See latest CRDT state (fastest, may see uncommitted facts)
/// - `ReadCommitted`: Only see facts confirmed by consensus
/// - `Snapshot`: Query against a specific historical prestate
/// - `ReadLatest`: Wait for all pending consensus in scope to complete
///
/// # Example
///
/// ```ignore
/// // Fast query - may see uncommitted facts
/// let result = effects.query_with_isolation(
///     &ChannelsQuery::default(),
///     QueryIsolation::ReadUncommitted,
/// ).await?;
///
/// // Strong consistency - wait for specific consensus
/// let result = effects.query_with_isolation(
///     &ChannelsQuery::default(),
///     QueryIsolation::ReadCommitted { wait_for: vec![consensus_id] },
/// ).await?;
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum QueryIsolation {
    /// See all facts including uncommitted (CRDT state).
    ///
    /// Fastest option - queries execute immediately against the current
    /// CRDT state without waiting for consensus confirmation.
    #[default]
    ReadUncommitted,

    /// Only see facts with consensus commit.
    ///
    /// Waits for specified consensus instances to complete before executing
    /// the query. Use this when you need to see the results of specific
    /// operations that required consensus.
    ReadCommitted {
        /// Consensus instances to wait for before executing query
        wait_for: Vec<ConsensusId>,
    },

    /// Snapshot at specific prestate (time-travel query).
    ///
    /// Queries against a historical state identified by its prestate hash.
    /// Useful for auditing or debugging. The prestate must still be available
    /// (not garbage collected).
    Snapshot {
        /// Hash of the prestate to query against
        prestate_hash: Hash32,
    },

    /// Wait for all pending consensus in scope to complete.
    ///
    /// More expensive than `ReadCommitted` - waits for all pending consensus
    /// operations that affect the specified resource scope. Use sparingly.
    ///
    /// **Note:** This is NOT linearizable - just ensures all pending commits
    /// are visible at query time.
    ReadLatest {
        /// Resource scope to wait for
        scope: ResourceScope,
    },
}

impl QueryIsolation {
    /// Create a ReadCommitted isolation waiting for a single consensus
    pub fn read_committed(consensus_id: ConsensusId) -> Self {
        Self::ReadCommitted {
            wait_for: vec![consensus_id],
        }
    }

    /// Create a Snapshot isolation for a specific prestate
    pub fn snapshot(prestate_hash: Hash32) -> Self {
        Self::Snapshot { prestate_hash }
    }

    /// Create a ReadLatest isolation for a resource scope
    pub fn read_latest(scope: ResourceScope) -> Self {
        Self::ReadLatest { scope }
    }

    /// Check if this isolation level requires waiting
    pub fn requires_wait(&self) -> bool {
        !matches!(self, Self::ReadUncommitted | Self::Snapshot { .. })
    }
}

/// Identifier for a consensus instance.
///
/// Used to track and wait for consensus completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ConsensusId(pub [u8; 32]);

impl ConsensusId {
    /// Create a new consensus ID from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Identifier for a fact in the journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FactId(pub [u8; 32]);

impl FactId {
    /// Create a new fact ID from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Mutation Receipt
// ─────────────────────────────────────────────────────────────────────────────

/// Receipt returned from mutation operations.
///
/// Indicates whether the operation was completed immediately (monotone) or
/// required consensus (non-monotone). This allows callers to:
/// - Know when their operation has taken effect
/// - Wait for consensus completion if needed
/// - Track latency and operation routing
///
/// # Example
///
/// ```ignore
/// let receipt = effects.mutate(mutation).await?;
/// match receipt {
///     MutationReceipt::Immediate { fact_ids, .. } => {
///         println!("Operation completed immediately, {} facts created", fact_ids.len());
///     }
///     MutationReceipt::Consensus { consensus_id, .. } => {
///         // Can use consensus_id with QueryIsolation::ReadCommitted to wait
///         println!("Operation requires consensus: {:?}", consensus_id);
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MutationReceipt {
    /// Monotone operation completed immediately via CRDT merge.
    ///
    /// No coordination was required - the facts were appended directly
    /// to the local journal and will propagate via anti-entropy.
    Immediate {
        /// IDs of facts created by this operation
        fact_ids: Vec<FactId>,
        /// When the operation completed
        timestamp: PhysicalTime,
    },

    /// Non-monotone operation required consensus.
    ///
    /// The operation has been submitted to the consensus protocol.
    /// Use `consensus_id` with `QueryIsolation::ReadCommitted` to wait
    /// for the operation to complete before querying.
    Consensus {
        /// ID of the consensus instance handling this operation
        consensus_id: ConsensusId,
        /// Hash of the prestate the consensus is based on
        prestate_hash: Hash32,
        /// Time taken to submit to consensus (not total completion time)
        submit_latency: Duration,
    },
}

impl MutationReceipt {
    /// Create an immediate receipt
    pub fn immediate(fact_ids: Vec<FactId>, timestamp: PhysicalTime) -> Self {
        Self::Immediate {
            fact_ids,
            timestamp,
        }
    }

    /// Create a consensus receipt
    pub fn consensus(
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        submit_latency: Duration,
    ) -> Self {
        Self::Consensus {
            consensus_id,
            prestate_hash,
            submit_latency,
        }
    }

    /// Check if this was an immediate (monotone) operation
    pub fn is_immediate(&self) -> bool {
        matches!(self, Self::Immediate { .. })
    }

    /// Check if this required consensus
    pub fn requires_consensus(&self) -> bool {
        matches!(self, Self::Consensus { .. })
    }

    /// Get the consensus ID if this was a consensus operation
    pub fn consensus_id(&self) -> Option<ConsensusId> {
        match self {
            Self::Consensus { consensus_id, .. } => Some(*consensus_id),
            Self::Immediate { .. } => None,
        }
    }

    /// Get the fact IDs if this was an immediate operation
    pub fn fact_ids(&self) -> Option<&[FactId]> {
        match self {
            Self::Immediate { fact_ids, .. } => Some(fact_ids),
            Self::Consensus { .. } => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query Statistics
// ─────────────────────────────────────────────────────────────────────────────

/// Statistics about query execution.
///
/// Returned alongside query results when using `query_with_stats()`.
/// Useful for debugging, profiling, and optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStats {
    /// Time taken to execute the query
    pub execution_time: Duration,

    /// Number of facts scanned during execution
    pub facts_scanned: u32,

    /// Number of facts that matched the query
    pub facts_matched: u32,

    /// Whether the result was served from cache
    pub cache_hit: bool,

    /// Isolation level used for the query
    pub isolation_used: QueryIsolation,

    /// Time spent waiting for consensus (if any)
    pub consensus_wait_time: Option<Duration>,

    /// Consistency metadata for matched facts.
    ///
    /// Maps fact identifiers (or result item IDs) to their consistency status.
    /// Empty if consistency tracking was not requested.
    pub consistency: ConsistencyMap,
}

impl Default for QueryStats {
    fn default() -> Self {
        Self {
            execution_time: Duration::ZERO,
            facts_scanned: 0,
            facts_matched: 0,
            cache_hit: false,
            isolation_used: QueryIsolation::default(),
            consensus_wait_time: None,
            consistency: ConsistencyMap::new(),
        }
    }
}

impl QueryStats {
    /// Create new stats with execution time
    pub fn new(execution_time: Duration) -> Self {
        Self {
            execution_time,
            ..Default::default()
        }
    }

    /// Mark as a cache hit
    #[must_use]
    pub fn with_cache_hit(mut self) -> Self {
        self.cache_hit = true;
        self
    }

    /// Set facts scanned
    #[must_use]
    pub fn with_facts_scanned(mut self, count: u32) -> Self {
        self.facts_scanned = count;
        self
    }

    /// Set facts matched
    #[must_use]
    pub fn with_facts_matched(mut self, count: u32) -> Self {
        self.facts_matched = count;
        self
    }

    /// Set isolation level used
    #[must_use]
    pub fn with_isolation(mut self, isolation: QueryIsolation) -> Self {
        self.isolation_used = isolation;
        self
    }

    /// Set consensus wait time
    #[must_use]
    pub fn with_consensus_wait(mut self, duration: Duration) -> Self {
        self.consensus_wait_time = Some(duration);
        self
    }

    /// Set consistency metadata for matched facts
    #[must_use]
    pub fn with_consistency(mut self, consistency: ConsistencyMap) -> Self {
        self.consistency = consistency;
        self
    }

    /// Calculate selectivity (matched / scanned)
    pub fn selectivity(&self) -> f64 {
        if self.facts_scanned == 0 {
            0.0
        } else {
            self.facts_matched as f64 / self.facts_scanned as f64
        }
    }

    /// Check if any matched fact is finalized
    pub fn any_finalized(&self) -> bool {
        self.consistency
            .iter()
            .any(|(_, c)| c.agreement.is_finalized())
    }

    /// Check if all matched facts are finalized
    pub fn all_finalized(&self) -> bool {
        !self.consistency.is_empty()
            && self
                .consistency
                .iter()
                .all(|(_, c)| c.agreement.is_finalized())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Datalog Types
// ─────────────────────────────────────────────────────────────────────────────

/// A Datalog program consisting of rules and facts.
///
/// This is the intermediate representation that queries compile to.
/// The actual execution happens in the QueryHandler.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatalogProgram {
    /// Rules that define derived relations
    pub rules: Vec<DatalogRule>,
    /// Base facts to include (optional, usually come from journal)
    pub facts: Vec<DatalogFact>,
    /// The goal query to evaluate
    pub goal: Option<String>,
}

impl DatalogProgram {
    /// Create an empty program
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create a program with the given rules
    pub fn new(rules: Vec<DatalogRule>) -> Self {
        Self {
            rules,
            facts: Vec::new(),
            goal: None,
        }
    }

    /// Add a rule to the program
    #[must_use]
    pub fn with_rule(mut self, rule: DatalogRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add a fact to the program
    #[must_use]
    pub fn with_fact(mut self, fact: DatalogFact) -> Self {
        self.facts.push(fact);
        self
    }

    /// Set the goal query
    #[must_use]
    pub fn with_goal(mut self, goal: impl Into<String>) -> Self {
        self.goal = Some(goal.into());
        self
    }

    /// Convert to Datalog source string
    pub fn to_datalog_source(&self) -> String {
        let mut source = String::new();

        // Emit facts
        for fact in &self.facts {
            source.push_str(&fact.to_string());
            source.push_str(".\n");
        }

        // Emit rules
        for rule in &self.rules {
            source.push_str(&rule.to_string());
            source.push_str(".\n");
        }

        // Emit goal
        if let Some(ref goal) = self.goal {
            source.push_str("?- ");
            source.push_str(goal);
            source.push_str(".\n");
        }

        source
    }

    /// Check if the program is empty
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.facts.is_empty()
    }
}

/// A Datalog rule (head :- body)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatalogRule {
    /// The rule head (conclusion fact)
    pub head: DatalogFact,
    /// The rule body (conditions)
    pub body: Vec<DatalogFact>,
}

impl DatalogRule {
    /// Create a new rule with a head and empty body
    pub fn new(head: DatalogFact) -> Self {
        Self {
            head,
            body: Vec::new(),
        }
    }

    /// Create a rule with head and body
    #[must_use]
    pub fn with_body(head: DatalogFact, body: Vec<DatalogFact>) -> Self {
        Self { head, body }
    }

    /// Add a condition to the body
    pub fn when(mut self, condition: DatalogFact) -> Self {
        self.body.push(condition);
        self
    }

    /// Add multiple conditions
    pub fn when_all(mut self, conditions: impl IntoIterator<Item = DatalogFact>) -> Self {
        for c in conditions {
            self.body.push(c);
        }
        self
    }
}

impl fmt::Display for DatalogRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.head)?;
        if !self.body.is_empty() {
            write!(f, " :- ")?;
            for (i, fact) in self.body.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{fact}")?;
            }
        }
        Ok(())
    }
}

/// A Datalog fact (ground term)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatalogFact {
    /// Predicate name
    pub predicate: String,
    /// Arguments (as strings for serialization)
    pub args: Vec<DatalogValue>,
}

impl DatalogFact {
    /// Create a new fact
    pub fn new(predicate: impl Into<String>, args: Vec<DatalogValue>) -> Self {
        Self {
            predicate: predicate.into(),
            args,
        }
    }
}

impl fmt::Display for DatalogFact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.predicate)?;
        for (i, arg) in self.args.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{arg}")?;
        }
        write!(f, ")")
    }
}

/// A value in Datalog (string, number, or boolean)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DatalogValue {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Boolean value
    Boolean(bool),
    /// Variable (for patterns)
    Variable(String),
    /// Symbol (unquoted identifier)
    Symbol(String),
    /// Null/none value
    Null,
}

impl DatalogValue {
    /// Create a variable value (shorthand for `DatalogValue::Variable`)
    pub fn var(name: impl Into<String>) -> Self {
        Self::Variable(name.into())
    }

    /// Create a symbol value
    pub fn symbol(name: impl Into<String>) -> Self {
        Self::Symbol(name.into())
    }
}

impl fmt::Display for DatalogValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "\"{}\"", s.replace('"', "\\\"")),
            Self::Integer(n) => write!(f, "{n}"),
            Self::Boolean(b) => write!(f, "{b}"),
            Self::Variable(v) => write!(f, "${v}"),
            Self::Symbol(s) => write!(f, "{s}"),
            Self::Null => write!(f, "null"),
        }
    }
}

/// Result bindings from Datalog evaluation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatalogBindings {
    /// Each row is a set of variable bindings
    pub rows: Vec<DatalogRow>,
}

impl DatalogBindings {
    /// Create empty bindings
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a row
    #[must_use]
    pub fn with_row(mut self, row: DatalogRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Number of result rows
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// A row of variable bindings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatalogRow {
    /// Variable name to value mappings
    pub bindings: Vec<(String, DatalogValue)>,
}

impl DatalogRow {
    /// Create a new row
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a binding
    #[must_use]
    pub fn with_binding(mut self, name: impl Into<String>, value: DatalogValue) -> Self {
        self.bindings.push((name.into(), value));
        self
    }

    /// Get a binding by name
    pub fn get(&self, name: &str) -> Option<&DatalogValue> {
        self.bindings
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// Get a string value by name
    pub fn get_string(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(DatalogValue::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Get an integer value by name
    pub fn get_integer(&self, name: &str) -> Option<i64> {
        match self.get(name) {
            Some(DatalogValue::Integer(n)) => Some(*n),
            _ => None,
        }
    }

    /// Get a boolean value by name
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        match self.get(name) {
            Some(DatalogValue::Boolean(b)) => Some(*b),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fact Predicates (for invalidation tracking)
// ─────────────────────────────────────────────────────────────────────────────

/// A predicate pattern for matching facts.
///
/// Used to determine which queries need re-evaluation when facts change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FactPredicate {
    /// The predicate name to match
    pub name: String,
    /// Optional argument patterns (None = wildcard)
    pub arg_patterns: Vec<Option<String>>,
}

impl FactPredicate {
    /// Create a predicate that matches any fact with the given name
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arg_patterns: Vec::new(),
        }
    }

    /// Create a predicate (alias for named)
    pub fn new(name: impl Into<String>) -> Self {
        Self::named(name)
    }

    /// Create a predicate with specific argument constraints
    #[must_use]
    pub fn with_args(name: impl Into<String>, args: Vec<(&str, &str)>) -> Self {
        let mut predicate = Self::named(name);
        // Convert (name, value) pairs to positional arg patterns
        // For simplicity, we store constraints as named patterns
        // In a full implementation, this would use a more sophisticated matching
        for (arg_name, arg_value) in args {
            predicate
                .arg_patterns
                .push(Some(format!("{arg_name}={arg_value}")));
        }
        predicate
    }

    /// Add an argument pattern (Some = must match, None = wildcard)
    #[must_use]
    pub fn with_arg(mut self, pattern: Option<String>) -> Self {
        self.arg_patterns.push(pattern);
        self
    }

    /// Check if this predicate matches a fact
    pub fn matches_fact(&self, fact_name: &str, fact_args: &[String]) -> bool {
        if self.name != fact_name {
            return false;
        }

        // If no arg patterns, match any args
        if self.arg_patterns.is_empty() {
            return true;
        }

        // Check each arg pattern
        for (i, pattern) in self.arg_patterns.iter().enumerate() {
            if let Some(expected) = pattern {
                if fact_args.get(i) != Some(expected) {
                    return false;
                }
            }
        }

        true
    }

    /// Check if this predicate could match another predicate.
    ///
    /// Two predicates match if:
    /// - They have the same name
    /// - Either has no arg patterns (wildcard), OR
    /// - Their arg patterns are compatible (same values where both specify)
    pub fn matches(&self, other: &FactPredicate) -> bool {
        // Names must match
        if self.name != other.name {
            return false;
        }

        // If either has no arg patterns, they match
        if self.arg_patterns.is_empty() || other.arg_patterns.is_empty() {
            return true;
        }

        // Check that specified args are compatible
        let max_len = self.arg_patterns.len().max(other.arg_patterns.len());
        for i in 0..max_len {
            let self_arg = self.arg_patterns.get(i).and_then(|a| a.as_ref());
            let other_arg = other.arg_patterns.get(i).and_then(|a| a.as_ref());

            match (self_arg, other_arg) {
                // Both specify a value - must match
                (Some(a), Some(b)) if a != b => return false,
                // At least one is wildcard - compatible
                _ => continue,
            }
        }

        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Capability Requirements
// ─────────────────────────────────────────────────────────────────────────────

/// A capability required to execute a query.
///
/// This integrates with Biscuit authorization - queries declare what
/// capabilities they need, and the QueryHandler checks them before execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct QueryCapability {
    /// Resource being accessed
    pub resource: String,
    /// Action being performed (e.g., "read", "list")
    pub action: String,
    /// Optional constraints
    pub constraints: Vec<(String, String)>,
}

impl QueryCapability {
    /// Create a read capability for a resource
    pub fn read(resource: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            action: "read".to_string(),
            constraints: Vec::new(),
        }
    }

    /// Create a list capability for a resource
    pub fn list(resource: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            action: "list".to_string(),
            constraints: Vec::new(),
        }
    }

    /// Add a constraint
    #[must_use]
    pub fn with_constraint(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.constraints.push((key.into(), value.into()));
        self
    }

    /// Convert to Biscuit Datalog check
    pub fn to_biscuit_check(&self) -> String {
        let mut check = format!("check if right(\"{}\", \"{}\")", self.resource, self.action);
        for (key, value) in &self.constraints {
            check.push_str(&format!(", {key} == \"{value}\""));
        }
        check
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query Trait
// ─────────────────────────────────────────────────────────────────────────────

/// Error type for query parsing
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum QueryParseError {
    /// Missing required field
    #[error("Missing required field: {field}")]
    MissingField { field: String },

    /// Invalid field value
    #[error("Invalid value for field {field}: {reason}")]
    InvalidValue { field: String, reason: String },

    /// Type conversion error
    #[error("Type conversion error: {reason}")]
    TypeConversion { reason: String },
}

/// Trait for typed queries that compile to Datalog.
///
/// Queries are the portable read interface for the journal. They:
/// - Compile to Datalog programs for execution
/// - Declare Biscuit capabilities for authorization
/// - Specify fact predicates for change tracking
/// - Parse results to typed values
///
/// # Example
///
/// ```ignore
/// use aura_core::query::{Query, DatalogProgram, QueryCapability, FactPredicate};
///
/// struct ChannelsQuery {
///     channel_type: Option<String>,
/// }
///
/// impl Query for ChannelsQuery {
///     type Result = Vec<Channel>;
///
///     fn to_datalog(&self) -> DatalogProgram {
///         DatalogProgram::new()
///             .with_rule(DatalogRule::new("channel($id, $name, $type)")
///                 .when("channel_fact($id, $name, $type)"))
///             .with_goal("channel($id, $name, $type)")
///     }
///
///     fn required_capabilities(&self) -> Vec<QueryCapability> {
///         vec![QueryCapability::list("channels")]
///     }
///
///     fn dependencies(&self) -> Vec<FactPredicate> {
///         vec![FactPredicate::named("channel_fact")]
///     }
///
///     fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
///         // Parse bindings to Vec<Channel>
///     }
/// }
/// ```
pub trait Query: Send + Sync + Clone + 'static {
    /// The result type of this query
    type Result: Clone + Send + Sync + Default + 'static;

    /// Compile this query to a Datalog program.
    ///
    /// The program will be executed against the journal facts,
    /// filtered by Biscuit authorization.
    fn to_datalog(&self) -> DatalogProgram;

    /// Get the Biscuit capabilities required to execute this query.
    ///
    /// The QueryHandler will verify these capabilities before execution.
    fn required_capabilities(&self) -> Vec<QueryCapability>;

    /// Get the fact predicates this query depends on.
    ///
    /// Used for invalidation tracking - when facts matching these predicates
    /// change, subscriptions to this query will re-evaluate.
    fn dependencies(&self) -> Vec<FactPredicate>;

    /// Parse Datalog bindings to the typed result.
    ///
    /// Called after query execution to convert raw bindings to the result type.
    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError>;

    /// Get a unique identifier for this query type.
    ///
    /// Used for caching and subscription management.
    fn query_id(&self) -> String {
        std::any::type_name::<Self>().to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datalog_rule_display() {
        let head = DatalogFact::new(
            "channel",
            vec![DatalogValue::var("id"), DatalogValue::var("name")],
        );
        let cond1 = DatalogFact::new(
            "channel_fact",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("type"),
            ],
        );
        let cond2 = DatalogFact::new(
            "eq",
            vec![
                DatalogValue::var("type"),
                DatalogValue::String("home".to_string()),
            ],
        );

        let rule = DatalogRule::new(head).when(cond1).when(cond2);

        let s = rule.to_string();
        assert!(s.contains("channel($id, $name)"));
        assert!(s.contains(":-"));
        assert!(s.contains("channel_fact"));
    }

    #[test]
    fn test_datalog_fact_display() {
        let fact = DatalogFact::new(
            "channel",
            vec![
                DatalogValue::String("ch1".to_string()),
                DatalogValue::String("General".to_string()),
                DatalogValue::Boolean(true),
            ],
        );

        let s = fact.to_string();
        assert_eq!(s, "channel(\"ch1\", \"General\", true)");
    }

    #[test]
    fn test_datalog_program_source() {
        let program = DatalogProgram::new(vec![DatalogRule::new(DatalogFact::new(
            "active_user",
            vec![DatalogValue::var("name")],
        ))
        .when(DatalogFact::new("user", vec![DatalogValue::var("name")]))
        .when(DatalogFact::new("online", vec![DatalogValue::var("name")]))])
        .with_fact(DatalogFact::new(
            "user",
            vec![DatalogValue::String("alice".to_string())],
        ))
        .with_goal("active_user($name)");

        let source = program.to_datalog_source();
        assert!(source.contains("user(\"alice\")"));
        assert!(source.contains("active_user($name) :- user($name), online($name)"));
        assert!(source.contains("?- active_user($name)"));
    }

    #[test]
    fn test_fact_predicate_matches() {
        let pred = FactPredicate::named("channel_fact");
        assert!(pred.matches(&FactPredicate::named("channel_fact")));
        assert!(pred.matches(&FactPredicate::named("channel_fact")));
        assert!(!pred.matches(&FactPredicate::named("other_fact")));

        let pred_with_arg =
            FactPredicate::named("channel_fact").with_arg(Some("specific_id".to_string()));
        assert!(pred_with_arg.matches(
            &FactPredicate::named("channel_fact").with_arg(Some("specific_id".to_string()))
        ));
        assert!(!pred_with_arg
            .matches(&FactPredicate::named("channel_fact").with_arg(Some("other_id".to_string()))));
    }

    #[test]
    fn test_query_capability() {
        let cap = QueryCapability::read("channels").with_constraint("owner", "alice");

        let check = cap.to_biscuit_check();
        assert!(check.contains("right(\"channels\", \"read\")"));
        assert!(check.contains("owner == \"alice\""));
    }

    #[test]
    fn test_datalog_row_get() {
        let row = DatalogRow::new()
            .with_binding("id", DatalogValue::String("ch1".to_string()))
            .with_binding("count", DatalogValue::Integer(42))
            .with_binding("active", DatalogValue::Boolean(true));

        assert_eq!(row.get_string("id"), Some("ch1"));
        assert_eq!(row.get_integer("count"), Some(42));
        assert_eq!(row.get_bool("active"), Some(true));
        assert_eq!(row.get_string("missing"), None);
    }

    #[test]
    fn test_query_isolation_default() {
        let isolation = QueryIsolation::default();
        assert_eq!(isolation, QueryIsolation::ReadUncommitted);
        assert!(!isolation.requires_wait());
    }

    #[test]
    fn test_query_isolation_read_committed() {
        let consensus_id = ConsensusId::new([1u8; 32]);
        let isolation = QueryIsolation::read_committed(consensus_id);
        assert!(isolation.requires_wait());
        if let QueryIsolation::ReadCommitted { wait_for } = isolation {
            assert_eq!(wait_for.len(), 1);
            assert_eq!(wait_for[0], consensus_id);
        } else {
            panic!("Expected ReadCommitted variant");
        }
    }

    #[test]
    fn test_query_isolation_snapshot() {
        let hash = crate::Hash32([42u8; 32]);
        let isolation = QueryIsolation::snapshot(hash);
        assert!(!isolation.requires_wait()); // Snapshot doesn't require waiting
        if let QueryIsolation::Snapshot { prestate_hash } = isolation {
            assert_eq!(prestate_hash, hash);
        } else {
            panic!("Expected Snapshot variant");
        }
    }

    #[test]
    fn test_query_stats_default() {
        let stats = QueryStats::default();
        assert_eq!(stats.execution_time, Duration::ZERO);
        assert_eq!(stats.facts_scanned, 0);
        assert_eq!(stats.facts_matched, 0);
        assert!(!stats.cache_hit);
        assert_eq!(stats.selectivity(), 0.0);
    }

    #[test]
    fn test_query_stats_builder() {
        let stats = QueryStats::new(Duration::from_millis(50))
            .with_facts_scanned(100)
            .with_facts_matched(25)
            .with_cache_hit()
            .with_isolation(QueryIsolation::ReadUncommitted);

        assert_eq!(stats.execution_time, Duration::from_millis(50));
        assert_eq!(stats.facts_scanned, 100);
        assert_eq!(stats.facts_matched, 25);
        assert!(stats.cache_hit);
        assert!((stats.selectivity() - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_consensus_id() {
        let bytes = [0xAB; 32];
        let id = ConsensusId::new(bytes);
        assert_eq!(id.as_bytes(), &bytes);
    }
}
