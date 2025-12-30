//! Temporal Database Types
//!
//! This module provides immutable, append-only database semantics for Aura's journal.
//! Inspired by Datomic's approach, all data changes are represented as facts with:
//!
//! - **Assert**: Add a fact to a scope
//! - **Retract**: Mark a specific fact as retracted
//! - **EpochBump**: Invalidate all facts in a scope before a point
//! - **Checkpoint**: Create a queryable snapshot of scope state
//!
//! # Key Concepts
//!
//! ## Scopes
//!
//! Scopes are hierarchical paths that organize facts:
//! - `authority:abc123` - Authority-level scope
//! - `authority:abc123/chat/channel:xyz` - Nested channel scope
//! - `context:def456/messages` - Context-level message scope
//!
//! ## Finality
//!
//! Facts progress through finality stages:
//! - `Local`: Written locally, not yet replicated
//! - `Replicated`: Acknowledged by N peers
//! - `Checkpointed`: Included in a durable checkpoint
//! - `Consensus`: Confirmed via Aura Consensus
//! - `Anchored`: Anchored to external chain (strongest)
//!
//! ## Transactions
//!
//! For operations requiring atomicity, facts can be grouped into transactions.
//! Simple monotonic operations don't need transactions.
//!
//! # Design Rationale
//!
//! This design follows the "hybrid transaction" approach:
//! - Simple appends: Direct fact assertions (no transaction overhead)
//! - Atomic operations: Explicit transaction grouping when needed
//!
//! Finality configuration is per-scope with per-operation override, allowing:
//! - Scope-level defaults (e.g., channels require Checkpointed)
//! - Operation-specific elevation (e.g., payment requires Consensus)

/// Maximum size for fact content data in bytes.
/// Prevents unbounded memory allocation from malformed inputs.
pub const MAX_TEMPORAL_DATA_BYTES: usize = 65536;

use crate::query::{ConsensusId, FactId};
use crate::time::{OrderTime, PhysicalTime};
use crate::types::Epoch;
use crate::Hash32;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// Scope Identifiers
// ─────────────────────────────────────────────────────────────────────────────

/// A segment in a hierarchical scope path.
///
/// Segments can be:
/// - Named: Simple identifiers like "chat", "messages"
/// - Typed: Type-qualified IDs like "channel:abc123", "authority:xyz"
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ScopeSegment {
    /// A simple named segment (e.g., "chat", "messages")
    Named(String),
    /// A typed segment with type and ID (e.g., "channel:abc123")
    Typed { kind: String, id: String },
}

impl ScopeSegment {
    /// Create a named segment
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }

    /// Create a typed segment
    pub fn typed(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self::Typed {
            kind: kind.into(),
            id: id.into(),
        }
    }

    /// Check if this segment matches a pattern segment
    pub fn matches(&self, pattern: &ScopeSegment) -> bool {
        match (self, pattern) {
            (Self::Named(a), Self::Named(b)) => a == b || b == "*",
            (Self::Typed { kind: k1, id: i1 }, Self::Typed { kind: k2, id: i2 }) => {
                (k1 == k2 || k2 == "*") && (i1 == i2 || i2 == "*")
            }
            _ => false,
        }
    }
}

impl fmt::Display for ScopeSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::Typed { kind, id } => write!(f, "{kind}:{id}"),
        }
    }
}

/// A hierarchical scope identifier for organizing facts.
///
/// Scopes use path-like semantics:
/// - `/` separates segments
/// - Segments can be named or typed (with `:`)
///
/// # Examples
///
/// ```ignore
/// let scope = ScopeId::parse("authority:abc123/chat/channel:xyz")?;
/// assert_eq!(scope.depth(), 3);
/// assert!(scope.starts_with(&ScopeId::parse("authority:abc123")?));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ScopeId {
    segments: Vec<ScopeSegment>,
}

impl ScopeId {
    /// Create an empty (root) scope
    pub fn root() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Create a scope from segments
    pub fn new(segments: Vec<ScopeSegment>) -> Self {
        Self { segments }
    }

    /// Create a single-segment scope
    pub fn single(segment: ScopeSegment) -> Self {
        Self {
            segments: vec![segment],
        }
    }

    /// Create an authority scope
    pub fn authority(authority_id: impl Into<String>) -> Self {
        Self::single(ScopeSegment::typed("authority", authority_id))
    }

    /// Create a context scope
    pub fn context(context_id: impl Into<String>) -> Self {
        Self::single(ScopeSegment::typed("context", context_id))
    }

    /// Parse a scope from a string path
    pub fn parse(path: &str) -> Result<Self, ScopeParseError> {
        if path.is_empty() {
            return Ok(Self::root());
        }

        let segments: Result<Vec<_>, _> = path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| {
                if let Some((kind, id)) = s.split_once(':') {
                    Ok(ScopeSegment::typed(kind, id))
                } else {
                    Ok(ScopeSegment::named(s))
                }
            })
            .collect();

        Ok(Self {
            segments: segments?,
        })
    }

    /// Append a segment to this scope
    pub fn push(&mut self, segment: ScopeSegment) {
        self.segments.push(segment);
    }

    /// Create a child scope with an additional segment
    pub fn child(&self, segment: ScopeSegment) -> Self {
        let mut new_segments = self.segments.clone();
        new_segments.push(segment);
        Self {
            segments: new_segments,
        }
    }

    /// Get the parent scope (or None if root)
    pub fn parent(&self) -> Option<Self> {
        if self.segments.is_empty() {
            None
        } else {
            let mut parent_segments = self.segments.clone();
            parent_segments.pop();
            Some(Self {
                segments: parent_segments,
            })
        }
    }

    /// Get the depth of this scope (number of segments)
    pub fn depth(&self) -> usize {
        self.segments.len()
    }

    /// Check if this scope is the root
    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    /// Check if this scope starts with another scope
    pub fn starts_with(&self, prefix: &ScopeId) -> bool {
        if prefix.segments.len() > self.segments.len() {
            return false;
        }
        self.segments
            .iter()
            .zip(prefix.segments.iter())
            .all(|(a, b)| a == b)
    }

    /// Check if this scope matches a pattern (with wildcards)
    pub fn matches(&self, pattern: &ScopeId) -> bool {
        if pattern.segments.len() != self.segments.len() {
            return false;
        }
        self.segments
            .iter()
            .zip(pattern.segments.iter())
            .all(|(a, b)| a.matches(b))
    }

    /// Get the segments
    pub fn segments(&self) -> &[ScopeSegment] {
        &self.segments
    }

    /// Get the last segment (leaf)
    pub fn leaf(&self) -> Option<&ScopeSegment> {
        self.segments.last()
    }
}

impl fmt::Display for ScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.segments.is_empty() {
            write!(f, "/")
        } else {
            for (i, segment) in self.segments.iter().enumerate() {
                if i > 0 {
                    write!(f, "/")?;
                }
                write!(f, "{segment}")?;
            }
            Ok(())
        }
    }
}

impl FromStr for ScopeId {
    type Err = ScopeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Error parsing a scope ID
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScopeParseError {
    #[error("Invalid scope segment: {segment}")]
    InvalidSegment { segment: String },

    #[error("Empty segment in path")]
    EmptySegment,
}

// ─────────────────────────────────────────────────────────────────────────────
// Finality Levels
// ─────────────────────────────────────────────────────────────────────────────

/// Finality level for a fact, indicating durability guarantees.
///
/// Facts progress through finality stages as they're replicated and confirmed.
/// Higher finality means stronger durability but typically higher latency.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Finality {
    /// Written to local storage only.
    ///
    /// Fastest, but may be lost if the device fails before replication.
    #[default]
    Local,

    /// Acknowledged by N peers.
    ///
    /// Provides redundancy but not strong consistency.
    Replicated {
        /// Number of peers that acknowledged
        ack_count: u16,
    },

    /// Included in a durable checkpoint.
    ///
    /// The checkpoint hash covers this fact, providing integrity verification.
    Checkpointed,

    /// Confirmed via Aura Consensus.
    ///
    /// Provides strong consistency within the authority/context scope.
    Consensus {
        /// The consensus instance that confirmed this fact
        proof: ConsensusId,
    },

    /// Anchored to an external chain.
    ///
    /// Strongest durability - the anchor provides external proof of existence.
    Anchored {
        /// Proof of anchoring
        anchor: AnchorProof,
    },
}

impl Finality {
    /// Create a Local finality
    pub fn local() -> Self {
        Self::Local
    }

    /// Create a Replicated finality with a specific ack count
    pub fn replicated(ack_count: u16) -> Self {
        Self::Replicated { ack_count }
    }

    /// Create a Checkpointed finality
    pub fn checkpointed() -> Self {
        Self::Checkpointed
    }

    /// Create a Consensus finality
    pub fn consensus(proof: ConsensusId) -> Self {
        Self::Consensus { proof }
    }

    /// Check if this finality level is at least as strong as another
    pub fn is_at_least(&self, other: &Finality) -> bool {
        self >= other
    }

    /// Get the numeric strength level (for comparison)
    pub fn strength(&self) -> u8 {
        match self {
            Self::Local => 0,
            Self::Replicated { .. } => 1,
            Self::Checkpointed => 2,
            Self::Consensus { .. } => 3,
            Self::Anchored { .. } => 4,
        }
    }
}

impl fmt::Display for Finality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Replicated { ack_count } => write!(f, "replicated({ack_count})"),
            Self::Checkpointed => write!(f, "checkpointed"),
            Self::Consensus { proof } => write!(f, "consensus({proof:?})"),
            Self::Anchored { anchor } => write!(f, "anchored({anchor:?})"),
        }
    }
}

/// Proof of external anchoring
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnchorProof {
    /// Chain identifier (e.g., "ethereum", "bitcoin")
    pub chain_id: String,
    /// Transaction or block hash on the external chain
    pub tx_hash: Hash32,
    /// Block number where anchored
    pub block_number: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Fact Operations
// ─────────────────────────────────────────────────────────────────────────────

/// Low-level operations on facts in the temporal database.
///
/// These are the primitive operations that modify the journal:
///
/// - `Assert`: Add a new fact
/// - `Retract`: Mark a specific fact as retracted
/// - `EpochBump`: Invalidate all facts in a scope before a point
/// - `Checkpoint`: Create a queryable snapshot
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactOp {
    /// Assert a new fact into a scope.
    ///
    /// This is the most common operation - adding new data.
    Assert {
        /// The content being asserted
        content: FactContent,
        /// Optional scope (defaults to the transaction/operation scope)
        scope: Option<ScopeId>,
    },

    /// Mark a specific fact as retracted.
    ///
    /// The fact remains in the journal but is marked as no longer valid.
    /// Queries can choose to include or exclude retracted facts.
    Retract {
        /// The fact being retracted
        target: FactId,
        /// Reason for the retraction
        reason: RetractReason,
    },

    /// Bump the epoch for a scope, invalidating older facts.
    ///
    /// All facts in the scope with epochs before `new_epoch` are considered
    /// superseded. This is more efficient than individual retractions when
    /// replacing large amounts of data.
    EpochBump {
        /// The scope to bump
        scope: ScopeId,
        /// The new epoch (must be greater than current)
        new_epoch: Epoch,
        /// Optional checkpoint hash at the new epoch
        checkpoint: Option<Hash32>,
    },

    /// Create a checkpoint (snapshot) of scope state.
    ///
    /// Checkpoints enable:
    /// - Historical queries (`as_of`)
    /// - Garbage collection of superseded facts
    /// - Proof of state at a point in time
    Checkpoint {
        /// The scope being checkpointed
        scope: ScopeId,
        /// Hash of the state at this checkpoint
        state_hash: Hash32,
        /// Facts superseded by this checkpoint (can be garbage collected)
        supersedes: Vec<FactId>,
    },
}

impl FactOp {
    /// Create an Assert operation
    pub fn assert(content: FactContent) -> Self {
        Self::Assert {
            content,
            scope: None,
        }
    }

    /// Create an Assert operation with explicit scope
    pub fn assert_in_scope(content: FactContent, scope: ScopeId) -> Self {
        Self::Assert {
            content,
            scope: Some(scope),
        }
    }

    /// Create a Retract operation
    pub fn retract(target: FactId, reason: RetractReason) -> Self {
        Self::Retract { target, reason }
    }

    /// Create an EpochBump operation
    pub fn epoch_bump(scope: ScopeId, new_epoch: Epoch) -> Self {
        Self::EpochBump {
            scope,
            new_epoch,
            checkpoint: None,
        }
    }

    /// Create a Checkpoint operation
    pub fn checkpoint(scope: ScopeId, state_hash: Hash32, supersedes: Vec<FactId>) -> Self {
        Self::Checkpoint {
            scope,
            state_hash,
            supersedes,
        }
    }

    /// Check if this operation is monotonic (doesn't require consensus)
    pub fn is_monotonic(&self) -> bool {
        matches!(self, Self::Assert { .. } | Self::Checkpoint { .. })
    }

    /// Get the scope this operation affects
    pub fn affected_scope(&self) -> Option<&ScopeId> {
        match self {
            Self::Assert { scope, .. } => scope.as_ref(),
            Self::Retract { .. } => None, // Scope determined by target fact
            Self::EpochBump { scope, .. } => Some(scope),
            Self::Checkpoint { scope, .. } => Some(scope),
        }
    }
}

/// Content of a fact assertion.
///
/// This wraps the actual data being stored. The content is opaque to the
/// temporal layer - it's just bytes that higher layers interpret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactContent {
    /// Content type identifier (e.g., "message", "channel", "contact")
    pub content_type: String,
    /// Serialized content data
    pub data: Vec<u8>,
    /// Optional entity ID for deduplication/updates
    pub entity_id: Option<String>,
}

impl FactContent {
    /// Create new fact content
    #[must_use]
    pub fn new(content_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            content_type: content_type.into(),
            data,
            entity_id: None,
        }
    }

    /// Create fact content with an entity ID
    #[must_use]
    pub fn with_entity(content_type: impl Into<String>, data: Vec<u8>, entity_id: String) -> Self {
        Self {
            content_type: content_type.into(),
            data,
            entity_id: Some(entity_id),
        }
    }
}

/// Reason for tombstoning a fact
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetractReason {
    /// User-initiated deletion
    #[default]
    UserDeleted,
    /// Superseded by a newer fact (with reference)
    Superseded { by: FactId },
    /// Policy-based expiration
    Expired,
    /// Correction of erroneous data
    Correction { correction_note: String },
    /// Compliance/legal requirement
    Compliance { regulation: String },
    /// Application-specific reason
    Custom { reason: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Fact Receipt
// ─────────────────────────────────────────────────────────────────────────────

/// Receipt returned after a fact operation completes.
///
/// Contains the fact ID, timestamp, and current finality level.
/// Can be used to:
/// - Reference the fact in later operations
/// - Wait for higher finality levels
/// - Query the exact point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactReceipt {
    /// Unique identifier for the created fact
    pub fact_id: FactId,
    /// When the operation was applied
    pub timestamp: PhysicalTime,
    /// Order token for deterministic ordering
    pub order: OrderTime,
    /// Current finality level
    pub finality: Finality,
    /// Scope the fact was written to
    pub scope: ScopeId,
    /// The epoch at write time
    pub epoch: Epoch,
}

impl FactReceipt {
    /// Create a new fact receipt
    #[must_use]
    pub fn new(
        fact_id: FactId,
        timestamp: PhysicalTime,
        order: OrderTime,
        scope: ScopeId,
        epoch: Epoch,
    ) -> Self {
        Self {
            fact_id,
            timestamp,
            order,
            finality: Finality::Local,
            scope,
            epoch,
        }
    }

    /// Upgrade the finality level
    #[must_use]
    pub fn with_finality(mut self, finality: Finality) -> Self {
        self.finality = finality;
        self
    }

    /// Check if finality is at least the specified level
    pub fn is_finalized_at(&self, level: &Finality) -> bool {
        self.finality.is_at_least(level)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Transactions
// ─────────────────────────────────────────────────────────────────────────────

/// Unique identifier for a transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(pub [u8; 32]);

impl TransactionId {
    /// Create a new transaction ID from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A transaction grouping multiple fact operations atomically.
///
/// Transactions ensure that:
/// - All operations succeed or none do
/// - Operations are applied in order
/// - The transaction gets a single finality status
///
/// # Example
///
/// ```ignore
/// let tx = Transaction::new(scope)
///     .with_op(FactOp::assert(content1))
///     .with_op(FactOp::assert(content2))
///     .with_op(FactOp::retract(old_fact_id, RetractReason::Superseded { by: new_id }));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique transaction identifier
    pub id: TransactionId,
    /// The scope this transaction operates in
    pub scope: ScopeId,
    /// Operations to apply atomically
    pub operations: Vec<FactOp>,
    /// Required finality level for the transaction
    pub required_finality: Finality,
    /// Optional metadata
    pub metadata: Option<TransactionMetadata>,
}

impl Transaction {
    /// Create a new transaction in a scope
    #[must_use]
    pub fn new(scope: ScopeId) -> Self {
        Self {
            id: TransactionId([0; 32]), // Will be assigned by the system
            scope,
            operations: Vec::new(),
            required_finality: Finality::Local,
            metadata: None,
        }
    }

    /// Add an operation to the transaction
    #[must_use]
    pub fn with_op(mut self, op: FactOp) -> Self {
        self.operations.push(op);
        self
    }

    /// Add multiple operations
    #[must_use]
    pub fn with_ops(mut self, ops: impl IntoIterator<Item = FactOp>) -> Self {
        self.operations.extend(ops);
        self
    }

    /// Set required finality level
    #[must_use]
    pub fn with_finality(mut self, finality: Finality) -> Self {
        self.required_finality = finality;
        self
    }

    /// Set transaction metadata
    #[must_use]
    pub fn with_metadata(mut self, metadata: TransactionMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Check if the transaction is empty
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// Get the number of operations
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if all operations are monotonic
    pub fn is_monotonic(&self) -> bool {
        self.operations.iter().all(|op| op.is_monotonic())
    }
}

/// Metadata about a transaction
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionMetadata {
    /// Human-readable description
    pub description: Option<String>,
    /// Correlation ID for tracing
    pub correlation_id: Option<String>,
    /// Application-specific tags
    pub tags: Vec<String>,
}

/// Receipt for a completed transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
    /// Transaction identifier
    pub transaction_id: TransactionId,
    /// Receipts for each operation
    pub fact_receipts: Vec<FactReceipt>,
    /// Transaction-level finality
    pub finality: Finality,
    /// Total time to apply
    pub duration: Duration,
}

// ─────────────────────────────────────────────────────────────────────────────
// Temporal Queries
// ─────────────────────────────────────────────────────────────────────────────

/// A point in time for temporal queries.
///
/// Can be specified as:
/// - Physical time (wall clock)
/// - Order token (opaque ordering)
/// - Transaction ID (state after transaction)
/// - Epoch (scope epoch number)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporalPoint {
    /// Physical wall-clock time
    Physical(PhysicalTime),
    /// Opaque order token
    Order(OrderTime),
    /// State after a specific transaction
    AfterTransaction(TransactionId),
    /// State at a specific epoch in a scope
    AtEpoch { scope: ScopeId, epoch: Epoch },
    /// Current (latest) state
    #[default]
    Now,
}

impl TemporalPoint {
    /// Create a point at a physical time
    pub fn at_physical(time: PhysicalTime) -> Self {
        Self::Physical(time)
    }

    /// Create a point at an order token
    pub fn at_order(order: OrderTime) -> Self {
        Self::Order(order)
    }

    /// Create a point after a transaction
    pub fn after_tx(tx_id: TransactionId) -> Self {
        Self::AfterTransaction(tx_id)
    }

    /// Create a point at an epoch
    pub fn at_epoch(scope: ScopeId, epoch: Epoch) -> Self {
        Self::AtEpoch { scope, epoch }
    }

    /// Create a point at the current time
    pub fn now() -> Self {
        Self::Now
    }
}

/// Temporal query mode.
///
/// Specifies how to query facts with respect to time:
///
/// - `AsOf`: Database state at a point in time
/// - `Since`: Facts added since a point in time
/// - `History`: All versions of facts over a time range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TemporalQuery {
    /// Query database state as of a point in time.
    ///
    /// Returns facts that were valid at that point, excluding
    /// later retractions and epoch bumps.
    AsOf(TemporalPoint),

    /// Query facts added since a point in time.
    ///
    /// Returns a delta of changes (assertions and retractions)
    /// that occurred after the specified point.
    Since(TemporalPoint),

    /// Query the full history of an entity or scope.
    ///
    /// Returns all fact versions with their valid time ranges.
    History {
        /// Start of the history range
        from: TemporalPoint,
        /// End of the history range
        to: TemporalPoint,
    },
}

impl TemporalQuery {
    /// Create an AsOf query for the current time
    pub fn current() -> Self {
        Self::AsOf(TemporalPoint::Now)
    }

    /// Create an AsOf query for a specific time
    pub fn as_of(point: TemporalPoint) -> Self {
        Self::AsOf(point)
    }

    /// Create a Since query
    pub fn since(point: TemporalPoint) -> Self {
        Self::Since(point)
    }

    /// Create a History query
    pub fn history(from: TemporalPoint, to: TemporalPoint) -> Self {
        Self::History { from, to }
    }
}

impl Default for TemporalQuery {
    fn default() -> Self {
        Self::current()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Finality Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for finality requirements in a scope.
///
/// This allows per-scope defaults with operation-level override.
/// The configuration cascades down the scope hierarchy unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeFinalityConfig {
    /// The scope this configuration applies to
    pub scope: ScopeId,
    /// Default finality for operations in this scope
    pub default_finality: Finality,
    /// Minimum finality (operations cannot request less than this)
    pub minimum_finality: Finality,
    /// Whether child scopes inherit this configuration
    pub cascade: bool,
    /// Operation-specific overrides by content type
    pub content_overrides: Vec<ContentFinalityOverride>,
}

impl ScopeFinalityConfig {
    /// Create a new scope finality configuration
    #[must_use]
    pub fn new(scope: ScopeId) -> Self {
        Self {
            scope,
            default_finality: Finality::Local,
            minimum_finality: Finality::Local,
            cascade: true,
            content_overrides: Vec::new(),
        }
    }

    /// Set the default finality level
    #[must_use]
    pub fn with_default(mut self, finality: Finality) -> Self {
        self.default_finality = finality;
        self
    }

    /// Set the minimum finality level
    #[must_use]
    pub fn with_minimum(mut self, finality: Finality) -> Self {
        self.minimum_finality = finality;
        self
    }

    /// Set whether to cascade to child scopes
    #[must_use]
    pub fn with_cascade(mut self, cascade: bool) -> Self {
        self.cascade = cascade;
        self
    }

    /// Add a content-type override
    #[must_use]
    pub fn with_override(mut self, override_: ContentFinalityOverride) -> Self {
        self.content_overrides.push(override_);
        self
    }

    /// Get the effective finality for a content type
    pub fn effective_finality(&self, content_type: &str) -> Finality {
        // Check for content-specific override
        for override_ in &self.content_overrides {
            if override_.content_type == content_type {
                return override_.required_finality.clone();
            }
        }
        // Fall back to default
        self.default_finality.clone()
    }

    /// Validate that a requested finality meets minimum requirements
    pub fn validate_finality(&self, requested: &Finality) -> Result<(), FinalityError> {
        if requested >= &self.minimum_finality {
            Ok(())
        } else {
            Err(FinalityError::BelowMinimum {
                requested: Box::new(requested.clone()),
                minimum: Box::new(self.minimum_finality.clone()),
            })
        }
    }
}

/// Override finality requirements for specific content types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFinalityOverride {
    /// Content type this override applies to
    pub content_type: String,
    /// Required finality for this content type
    pub required_finality: Finality,
}

impl ContentFinalityOverride {
    /// Create a new content finality override
    #[must_use]
    pub fn new(content_type: impl Into<String>, required_finality: Finality) -> Self {
        Self {
            content_type: content_type.into(),
            required_finality,
        }
    }
}

/// Errors related to finality requirements
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum FinalityError {
    #[error("Requested finality {requested} is below minimum {minimum}")]
    BelowMinimum {
        requested: Box<Finality>,
        minimum: Box<Finality>,
    },

    #[error("Finality timeout: could not achieve {target} within deadline")]
    Timeout { target: Box<Finality> },

    #[error("Finality level not supported: {reason}")]
    NotSupported { reason: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_id_parse() {
        let scope = ScopeId::parse("authority:abc123/chat/channel:xyz").unwrap();
        assert_eq!(scope.depth(), 3);

        let segments = scope.segments();
        assert!(
            matches!(&segments[0], ScopeSegment::Typed { kind, id } if kind == "authority" && id == "abc123")
        );
        assert!(matches!(&segments[1], ScopeSegment::Named(n) if n == "chat"));
        assert!(
            matches!(&segments[2], ScopeSegment::Typed { kind, id } if kind == "channel" && id == "xyz")
        );
    }

    #[test]
    fn test_scope_id_starts_with() {
        let full = ScopeId::parse("authority:abc/chat/channel:xyz").unwrap();
        let prefix = ScopeId::parse("authority:abc").unwrap();
        let other = ScopeId::parse("authority:def").unwrap();

        assert!(full.starts_with(&prefix));
        assert!(!full.starts_with(&other));
        assert!(prefix.starts_with(&ScopeId::root()));
    }

    #[test]
    fn test_scope_id_child() {
        let parent = ScopeId::authority("abc123");
        let child = parent.child(ScopeSegment::named("chat"));

        assert_eq!(child.depth(), 2);
        assert!(child.starts_with(&parent));
    }

    #[test]
    fn test_scope_id_display() {
        let scope = ScopeId::parse("authority:abc/chat/channel:xyz").unwrap();
        assert_eq!(scope.to_string(), "authority:abc/chat/channel:xyz");

        let root = ScopeId::root();
        assert_eq!(root.to_string(), "/");
    }

    #[test]
    fn test_finality_ordering() {
        assert!(Finality::Local < Finality::replicated(1));
        assert!(Finality::replicated(1) < Finality::Checkpointed);
        assert!(Finality::Checkpointed < Finality::consensus(ConsensusId([0; 32])));
    }

    #[test]
    fn test_finality_is_at_least() {
        let checkpoint = Finality::Checkpointed;
        assert!(checkpoint.is_at_least(&Finality::Local));
        assert!(checkpoint.is_at_least(&Finality::replicated(5)));
        assert!(checkpoint.is_at_least(&Finality::Checkpointed));
        assert!(!checkpoint.is_at_least(&Finality::consensus(ConsensusId([0; 32]))));
    }

    #[test]
    fn test_fact_op_is_monotonic() {
        let assert_op = FactOp::assert(FactContent::new("test", vec![]));
        assert!(assert_op.is_monotonic());

        let retract_op = FactOp::retract(FactId([0; 32]), RetractReason::UserDeleted);
        assert!(!retract_op.is_monotonic());

        let epoch_bump = FactOp::epoch_bump(ScopeId::root(), Epoch::new(1));
        assert!(!epoch_bump.is_monotonic());

        let checkpoint = FactOp::checkpoint(ScopeId::root(), Hash32([0; 32]), vec![]);
        assert!(checkpoint.is_monotonic());
    }

    #[test]
    fn test_transaction_builder() {
        let tx = Transaction::new(ScopeId::authority("abc"))
            .with_op(FactOp::assert(FactContent::new("msg", vec![1, 2, 3])))
            .with_op(FactOp::assert(FactContent::new("msg", vec![4, 5, 6])))
            .with_finality(Finality::Checkpointed);

        assert_eq!(tx.len(), 2);
        assert!(tx.is_monotonic());
        assert_eq!(tx.required_finality, Finality::Checkpointed);
    }

    #[test]
    fn test_scope_finality_config() {
        let config = ScopeFinalityConfig::new(ScopeId::authority("abc"))
            .with_default(Finality::replicated(2))
            .with_minimum(Finality::Local)
            .with_override(ContentFinalityOverride::new(
                "payment",
                Finality::consensus(ConsensusId([0; 32])),
            ));

        // Regular content uses default
        assert_eq!(
            config.effective_finality("message"),
            Finality::replicated(2)
        );

        // Payment uses override
        assert!(matches!(
            config.effective_finality("payment"),
            Finality::Consensus { .. }
        ));

        // Validate finality
        assert!(config.validate_finality(&Finality::Local).is_ok());
        assert!(config.validate_finality(&Finality::Checkpointed).is_ok());
    }

    #[test]
    fn test_temporal_point() {
        let now = TemporalPoint::now();
        assert!(matches!(now, TemporalPoint::Now));

        let epoch_point = TemporalPoint::at_epoch(ScopeId::authority("abc"), Epoch::new(5));
        assert!(matches!(
            epoch_point,
            TemporalPoint::AtEpoch { epoch, .. } if epoch.value() == 5
        ));
    }

    #[test]
    fn test_temporal_query() {
        let query = TemporalQuery::current();
        assert!(matches!(query, TemporalQuery::AsOf(TemporalPoint::Now)));

        let history = TemporalQuery::history(
            TemporalPoint::at_epoch(ScopeId::root(), Epoch::new(0)),
            TemporalPoint::Now,
        );
        assert!(matches!(history, TemporalQuery::History { .. }));
    }
}
