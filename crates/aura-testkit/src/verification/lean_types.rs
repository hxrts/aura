//! Rust types matching the Lean verification model JSON schema.
//!
//! These types enable differential testing between the formally verified
//! Lean models and the Rust implementation. They serialize/deserialize
//! to match the exact JSON format expected by the Lean oracle.
//!
//! ## Type Correspondence
//!
//! | Lean Type              | Rust Type           |
//! |------------------------|---------------------|
//! | ByteArray32            | ByteArray32         |
//! | OrderTime              | OrderTime           |
//! | Hash32/AuthorityId/... | (aliases)           |
//! | TimeStamp              | LeanTimeStamp       |
//! | JournalNamespace       | LeanNamespace       |
//! | FactContent            | LeanFactContent     |
//! | Fact                   | LeanFact            |
//! | Journal                | LeanJournal         |

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ============================================================================
// ByteArray32 - Foundation type
// ============================================================================

/// 32-byte array serialized as 64-char hex string.
/// Matches Lean: `Aura.Types.ByteArray32`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ByteArray32(pub [u8; 32]);

impl ByteArray32 {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn zero() -> Self {
        Self([0u8; 32])
    }

    /// Create from hex string (must be exactly 64 chars)
    pub fn from_hex(s: &str) -> Result<Self, String> {
        if s.len() != 64 {
            return Err(format!("Expected 64 hex chars, got {}", s.len()));
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex_str = std::str::from_utf8(chunk).map_err(|e| e.to_string())?;
            bytes[i] = u8::from_str_radix(hex_str, 16).map_err(|e| e.to_string())?;
        }
        Ok(Self(bytes))
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl Serialize for ByteArray32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ByteArray32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

// Type aliases matching Lean
pub type Hash32 = ByteArray32;
pub type AuthorityId = ByteArray32;
pub type ContextId = ByteArray32;
pub type ChannelId = ByteArray32;

// ============================================================================
// OrderTime - Opaque ordering key
// ============================================================================

/// Opaque 32-byte ordering key for deterministic fact ordering.
/// Matches Lean: `Aura.Types.OrderTime.OrderTime`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderTime(pub ByteArray32);

impl OrderTime {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(ByteArray32(bytes))
    }

    pub fn zero() -> Self {
        Self(ByteArray32::zero())
    }

    /// Lexicographic comparison (matches Lean)
    pub fn compare(&self, other: &Self) -> std::cmp::Ordering {
        self.0 .0.cmp(&other.0 .0)
    }
}

impl PartialOrd for OrderTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.compare(other))
    }
}

impl Ord for OrderTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.compare(other)
    }
}

// ============================================================================
// TimeStamp - 4-variant time enum
// ============================================================================

/// Physical time structure matching Lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalTime {
    pub ts_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<u64>,
}

/// Vector clock entry matching Lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorEntry {
    pub device: ByteArray32,
    pub counter: u64,
}

/// Logical time structure matching Lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalTime {
    pub vector: Vec<VectorEntry>,
    pub lamport: u64,
}

/// Time confidence level matching Lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeConfidence {
    Low,
    Medium,
    High,
}

/// Range time structure matching Lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeTime {
    pub earliest_ms: u64,
    pub latest_ms: u64,
    pub confidence: TimeConfidence,
}

/// TimeStamp enum matching Lean's 4 variants.
/// Matches Lean: `Aura.Types.TimeStamp.TimeStamp`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "camelCase")]
pub enum LeanTimeStamp {
    /// Opaque ordering token (no temporal meaning)
    #[serde(rename = "orderClock")]
    OrderClock { value: OrderTime },

    /// Wall-clock time with optional uncertainty
    #[serde(rename = "physicalClock")]
    PhysicalClock { value: PhysicalTime },

    /// Logical clock for causality
    #[serde(rename = "logicalClock")]
    LogicalClock { value: LogicalTime },

    /// Time range constraint
    #[serde(rename = "range")]
    Range { value: RangeTime },
}

impl LeanTimeStamp {
    pub fn order_clock(order: OrderTime) -> Self {
        Self::OrderClock { value: order }
    }

    pub fn physical(ts_ms: u64, uncertainty: Option<u64>) -> Self {
        Self::PhysicalClock {
            value: PhysicalTime { ts_ms, uncertainty },
        }
    }

    pub fn logical(lamport: u64) -> Self {
        Self::LogicalClock {
            value: LogicalTime {
                vector: vec![],
                lamport,
            },
        }
    }

    pub fn range(start: u64, end: u64) -> Self {
        Self::Range {
            value: RangeTime {
                earliest_ms: start,
                latest_ms: end,
                confidence: TimeConfidence::Medium,
            },
        }
    }
}

// ============================================================================
// JournalNamespace
// ============================================================================

/// Namespace for journal scoping.
/// Matches Lean: `Aura.Types.Namespace.JournalNamespace`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "camelCase")]
pub enum LeanNamespace {
    /// Authority-scoped journal
    #[serde(rename = "authority")]
    Authority { id: AuthorityId },

    /// Context-scoped journal (shared between authorities)
    #[serde(rename = "context")]
    Context { id: ContextId },
}

// ============================================================================
// TreeOp types
// ============================================================================

/// Leaf role in commitment tree.
/// Matches Lean: `Aura.Types.TreeOp.LeafRole`
/// Note: Lean only defines Device and Guardian variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeafRole {
    Device,
    Guardian,
}

/// Tree operation kind.
/// Matches Lean: `Aura.Types.TreeOp.TreeOpKind`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "camelCase")]
pub enum TreeOpKind {
    #[serde(rename = "addLeaf")]
    AddLeaf {
        public_key: Vec<u8>,
        role: LeafRole,
    },
    #[serde(rename = "removeLeaf")]
    RemoveLeaf { leaf_index: u64 },
    #[serde(rename = "updatePolicy")]
    UpdatePolicy { threshold: u64 },
    #[serde(rename = "rotateEpoch")]
    RotateEpoch,
}

/// Attested operation on commitment tree.
/// Matches Lean: `Aura.Types.AttestedOp.AttestedOp`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestedOp {
    pub tree_op: TreeOpKind,
    pub parent_commitment: Hash32,
    pub new_commitment: Hash32,
    pub witness_threshold: u64,
    pub signature: Vec<u8>,
}

// ============================================================================
// Protocol Relational Facts
// ============================================================================

/// Channel checkpoint for AMP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelCheckpoint {
    pub channel_id: ChannelId,
    pub sequence: u64,
    pub state_hash: Hash32,
}

/// Proposed channel epoch bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedChannelEpochBump {
    pub channel_id: ChannelId,
    pub old_epoch: u64,
    pub new_epoch: u64,
    pub proposal_hash: Hash32,
}

/// Committed channel epoch bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedChannelEpochBump {
    pub channel_id: ChannelId,
    pub old_epoch: u64,
    pub new_epoch: u64,
    pub commitment_hash: Hash32,
}

/// Channel policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPolicy {
    pub channel_id: ChannelId,
    pub policy_hash: Hash32,
    pub effective_epoch: u64,
}

/// Leakage tracking fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeakageFact {
    pub source_authority: AuthorityId,
    pub target_authority: AuthorityId,
    pub leakage_type: String,
    pub amount: u64,
}

/// DKG transcript commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DkgTranscriptCommit {
    pub ceremony_id: Hash32,
    pub transcript_hash: Hash32,
    pub participant_count: u64,
}

/// Convergence certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergenceCert {
    pub context_id: ContextId,
    pub sequence: u64,
    pub cert_hash: Hash32,
}

/// Reversion fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReversionFact {
    pub authority_id: AuthorityId,
    pub reverted_facts: Vec<OrderTime>,
    pub reason: String,
}

/// Rotation fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotateFact {
    pub authority_id: AuthorityId,
    pub old_key_hash: Hash32,
    pub new_key_hash: Hash32,
}

/// Protocol-level relational facts (12 variants).
/// Matches Lean: `Aura.Types.ProtocolFacts.ProtocolRelationalFact`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "camelCase")]
pub enum ProtocolRelationalFact {
    #[serde(rename = "guardianBinding")]
    GuardianBinding {
        account_id: AuthorityId,
        guardian_id: AuthorityId,
        binding_hash: Hash32,
    },
    #[serde(rename = "recoveryGrant")]
    RecoveryGrant {
        account_id: AuthorityId,
        guardian_id: AuthorityId,
        grant_hash: Hash32,
    },
    #[serde(rename = "consensus")]
    Consensus {
        consensus_id: Hash32,
        operation_hash: Hash32,
        threshold_met: bool,
        participant_count: u64,
    },
    #[serde(rename = "ampChannelCheckpoint")]
    AmpChannelCheckpoint(ChannelCheckpoint),
    #[serde(rename = "ampProposedChannelEpochBump")]
    AmpProposedChannelEpochBump(ProposedChannelEpochBump),
    #[serde(rename = "ampCommittedChannelEpochBump")]
    AmpCommittedChannelEpochBump(CommittedChannelEpochBump),
    #[serde(rename = "ampChannelPolicy")]
    AmpChannelPolicy(ChannelPolicy),
    #[serde(rename = "leakageEvent")]
    LeakageEvent(LeakageFact),
    #[serde(rename = "dkgTranscriptCommit")]
    DkgTranscriptCommit(DkgTranscriptCommit),
    #[serde(rename = "convergenceCert")]
    ConvergenceCert(ConvergenceCert),
    #[serde(rename = "reversionFact")]
    ReversionFact(ReversionFact),
    #[serde(rename = "rotateFact")]
    RotateFact(RotateFact),
}

// ============================================================================
// RelationalFact and SnapshotFact
// ============================================================================

/// Relational fact (Protocol or Generic).
/// Matches Lean: `Aura.Types.FactContent.RelationalFact`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "camelCase")]
pub enum RelationalFact {
    #[serde(rename = "protocol")]
    Protocol { data: ProtocolRelationalFact },
    #[serde(rename = "generic")]
    Generic {
        context_id: ContextId,
        binding_type: String,
        binding_data: Vec<u8>,
    },
}

/// Snapshot fact for garbage collection.
/// Matches Lean: `Aura.Types.FactContent.SnapshotFact`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFact {
    pub state_hash: Hash32,
    pub superseded_facts: Vec<OrderTime>,
    pub sequence: u64,
}

// ============================================================================
// FactContent - Main fact payload
// ============================================================================

/// Fact content with 4 variants.
/// Matches Lean: `Aura.Types.FactContent.FactContent`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "variant", rename_all = "camelCase")]
pub enum LeanFactContent {
    #[serde(rename = "attestedOp")]
    AttestedOp { data: AttestedOp },
    #[serde(rename = "relational")]
    Relational { data: RelationalFact },
    #[serde(rename = "snapshot")]
    Snapshot { data: SnapshotFact },
    #[serde(rename = "rendezvousReceipt")]
    RendezvousReceipt {
        envelope_id: ByteArray32,
        authority_id: AuthorityId,
        timestamp: LeanTimeStamp,
        signature: Vec<u8>,
    },
}

// ============================================================================
// Fact and Journal - Top-level types
// ============================================================================

/// Full structured fact.
/// Matches Lean: `Aura.Journal.Fact`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanFact {
    /// Opaque total order for deterministic merges
    pub order: OrderTime,
    /// Semantic timestamp (not for ordering)
    pub timestamp: LeanTimeStamp,
    /// Content payload
    pub content: LeanFactContent,
}

impl LeanFact {
    pub fn new(order: OrderTime, timestamp: LeanTimeStamp, content: LeanFactContent) -> Self {
        Self {
            order,
            timestamp,
            content,
        }
    }
}

/// Full structured journal.
/// Matches Lean: `Aura.Journal.Journal`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanJournal {
    /// Namespace this journal belongs to (field name matches JSON key)
    pub namespace: LeanNamespace,
    /// Facts in this journal
    pub facts: Vec<LeanFact>,
}

impl LeanJournal {
    pub fn new(namespace: LeanNamespace, facts: Vec<LeanFact>) -> Self {
        Self { namespace, facts }
    }

    pub fn empty(namespace: LeanNamespace) -> Self {
        Self {
            namespace,
            facts: vec![],
        }
    }
}

// ============================================================================
// Journal merge result types
// ============================================================================

/// Result of journal merge operation.
#[derive(Debug, Clone, Deserialize)]
pub struct LeanJournalMergeResult {
    pub result: LeanJournal,
    pub count: usize,
}

/// Error result when namespace mismatch occurs.
#[derive(Debug, Clone, Deserialize)]
pub struct LeanNamespaceMismatchError {
    pub error: String,
    pub j1_namespace: LeanNamespace,
    pub j2_namespace: LeanNamespace,
}

/// Result of journal reduce operation.
#[derive(Debug, Clone, Deserialize)]
pub struct LeanJournalReduceResult {
    pub result: LeanJournal,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_array32_hex_roundtrip() {
        let bytes = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
                     0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
                     0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
                     0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let arr = ByteArray32::new(bytes);
        let hex = arr.to_hex();
        let parsed = ByteArray32::from_hex(&hex).unwrap();
        assert_eq!(arr, parsed);
    }

    #[test]
    fn test_order_time_ordering() {
        let a = OrderTime::new([0u8; 32]);
        let mut b_bytes = [0u8; 32];
        b_bytes[31] = 1;
        let b = OrderTime::new(b_bytes);

        assert!(a < b);
        assert!(b > a);
        assert_eq!(a, a.clone());
    }

    #[test]
    fn test_timestamp_json() {
        let ts = LeanTimeStamp::physical(1234567890, Some(100));
        let json = serde_json::to_string(&ts).unwrap();
        assert!(json.contains("physical"));
        assert!(json.contains("1234567890"));

        let parsed: LeanTimeStamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, parsed);
    }

    #[test]
    fn test_namespace_json() {
        let ns = LeanNamespace::Authority { id: ByteArray32::zero() };
        let json = serde_json::to_string(&ns).unwrap();
        assert!(json.contains("authority"));

        let parsed: LeanNamespace = serde_json::from_str(&json).unwrap();
        assert_eq!(ns, parsed);
    }

    #[test]
    fn test_fact_json() {
        let fact = LeanFact {
            order: OrderTime::zero(),
            timestamp: LeanTimeStamp::logical(42),
            content: LeanFactContent::Snapshot {
                data: SnapshotFact {
                    state_hash: ByteArray32::zero(),
                    superseded_facts: vec![],
                    sequence: 1,
                },
            },
        };

        let json = serde_json::to_string(&fact).unwrap();
        let parsed: LeanFact = serde_json::from_str(&json).unwrap();
        assert_eq!(fact, parsed);
    }

    #[test]
    fn test_journal_json() {
        let journal = LeanJournal {
            namespace: LeanNamespace::Context { id: ByteArray32::zero() },
            facts: vec![],
        };

        let json = serde_json::to_string(&journal).unwrap();
        assert!(json.contains("namespace"));
        assert!(json.contains("context"));

        let parsed: LeanJournal = serde_json::from_str(&json).unwrap();
        assert_eq!(journal, parsed);
    }
}

// ============================================================================
// Conversions: OrderTime only (Lean <-> Rust)
// ============================================================================
// Full type conversions between Lean and Rust are not possible because:
// - Rust uses UUID-based identifiers (AuthorityId, ContextId, DeviceId)
// - Lean uses 32-byte arrays for all identifiers
//
// For differential testing, we verify that BOTH systems satisfy the same
// CRDT semilattice laws (commutativity, associativity, idempotence).
// OrderTime is the key invariant - both systems must preserve OrderTime sets.

/// Convert Lean OrderTime to Rust OrderTime
impl From<OrderTime> for aura_core::time::OrderTime {
    fn from(ot: OrderTime) -> Self {
        aura_core::time::OrderTime(ot.0 .0)
    }
}

/// Convert Rust OrderTime to Lean OrderTime
impl From<aura_core::time::OrderTime> for OrderTime {
    fn from(ot: aura_core::time::OrderTime) -> Self {
        OrderTime(ByteArray32(ot.0))
    }
}

// Note: Full Journal/Fact/Namespace conversions are intentionally NOT provided.
// The Lean specification uses abstract 32-byte identifiers, while Rust uses
// UUID-based identifiers (AuthorityId, ContextId, DeviceId).
//
// For differential testing, we:
// 1. Test Lean CRDT laws using the Lean oracle
// 2. Test Rust CRDT laws using Rust types directly
// 3. Verify both systems satisfy the same mathematical properties
//
// The OrderTime conversion above is sufficient to compare the KEY INVARIANT:
// both systems must produce the same set of OrderTimes after merge operations.
