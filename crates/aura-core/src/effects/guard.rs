//! Pure guard evaluation with effect commands
//!
//! This module implements ADR-014's pure guard evaluation model where:
//! - Guards are pure functions that return effect commands as data
//! - Effect interpreters execute the commands asynchronously
//! - No blocking operations or sync/async bridges
//!
//! This enables algebraic effects, WASM compatibility, and deterministic simulation
//! while maintaining clean separation between business logic and I/O.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-protocol::guards` (Layer 4)
//! - **Usage**: Guard chain evaluation, effect command generation per ADR-014
//!
//! This is an application effect implementing Aura's guard chain model (CapGuard →
//! FlowGuard → JournalCoupler → LeakageTracker). The pure guard evaluation and
//! effect interpreter pattern is core to Aura's architecture. Handlers implement
//! the guard chain pipeline in `aura-protocol`.

use super::NetworkAddress;
use crate::{
    domain::journal::{Cap, Fact},
    time::TimeStamp,
    types::identifiers::{AuthorityId, ContextId},
    AuraResult as Result, Receipt,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Immutable snapshot of state for pure guard evaluation
///
/// This contains all data that guards are allowed to inspect during evaluation.
/// It's prepared asynchronously before guard evaluation and remains immutable
/// during the synchronous guard chain execution.
#[derive(Debug, Clone)]
pub struct GuardSnapshot {
    /// Current timestamp
    pub now: TimeStamp,
    /// Derived capability set for the current context
    pub caps: Cap,
    /// Current flow budget headroom (context, authority) -> remaining budget
    pub budgets: FlowBudgetView,
    /// Key-value metadata for guard decisions
    pub metadata: MetadataView,
    /// Pre-allocated randomness for deterministic nonce generation
    pub rng_seed: [u8; 32],
}

/// Read-only view of flow budgets
#[derive(Debug, Clone, Default)]
pub struct FlowBudgetView {
    budgets: HashMap<(ContextId, AuthorityId), u32>,
}

impl FlowBudgetView {
    /// Create a new flow budget view
    pub fn new(budgets: HashMap<(ContextId, AuthorityId), u32>) -> Self {
        Self { budgets }
    }

    /// Get remaining budget for a context/authority
    pub fn get(&self, context: &ContextId, authority: &AuthorityId) -> Option<u32> {
        self.budgets.get(&(*context, *authority)).copied()
    }

    /// Check if authority has at least the specified budget in a context
    pub fn has_budget(&self, context: &ContextId, authority: &AuthorityId, amount: u32) -> bool {
        self.get(context, authority)
            .is_some_and(|budget| budget >= amount)
    }
}

/// Read-only view of metadata
#[derive(Debug, Clone, Default)]
pub struct MetadataView {
    metadata: HashMap<String, String>,
}

impl MetadataView {
    /// Create a new metadata view
    pub fn new(metadata: HashMap<String, String>) -> Self {
        Self { metadata }
    }

    /// Get metadata value by key
    pub fn get(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// Check if metadata key exists
    pub fn contains(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }
}

/// Decision from guard evaluation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    /// Request is authorized
    Authorized,
    /// Request is denied with reason
    Denied(String),
}

impl Decision {
    /// Check if decision is authorized
    pub fn is_authorized(&self) -> bool {
        matches!(self, Self::Authorized)
    }

    /// Get denial reason if denied
    pub fn denial_reason(&self) -> Option<&str> {
        match self {
            Self::Denied(reason) => Some(reason),
            Self::Authorized => None,
        }
    }
}

/// Outcome of pure guard evaluation
#[derive(Debug, Clone)]
pub struct GuardOutcome {
    /// Authorization decision
    pub decision: Decision,
    /// Effect commands to execute if authorized
    pub effects: Vec<EffectCommand>,
}

impl GuardOutcome {
    /// Create an authorized outcome with effects
    pub fn authorized(effects: Vec<EffectCommand>) -> Self {
        Self {
            decision: Decision::Authorized,
            effects,
        }
    }

    /// Create a denied outcome with reason
    pub fn denied(reason: impl Into<String>) -> Self {
        Self {
            decision: Decision::Denied(reason.into()),
            effects: Vec::new(),
        }
    }

    /// Check if outcome is authorized
    pub fn is_authorized(&self) -> bool {
        self.decision.is_authorized()
    }
}

/// Minimal, domain-agnostic effect commands
///
/// These are the primitive algebraic operations that guards can request.
/// They represent what should happen, not how it happens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectCommand {
    /// Charge flow budget for spam/DoS protection
    ChargeBudget {
        /// Context being charged
        context: ContextId,
        /// Authority to charge
        authority: AuthorityId,
        /// Peer being communicated with (if applicable)
        peer: AuthorityId,
        /// Amount to charge
        amount: u32,
    },
    /// Append entry to the journal
    AppendJournal {
        /// Journal entry to append
        entry: JournalEntry,
    },
    /// Record metadata leakage for privacy analysis
    RecordLeakage {
        /// Number of metadata bits revealed
        bits: u32,
    },
    /// Store metadata key-value pair
    StoreMetadata {
        /// Storage key
        key: String,
        /// Storage value
        value: String,
    },
    /// Send envelope to network address
    SendEnvelope {
        /// Target address
        to: NetworkAddress,
        /// Envelope payload
        envelope: Vec<u8>,
    },
    /// Generate cryptographic nonce
    GenerateNonce {
        /// Number of random bytes needed
        bytes: usize,
    },
}

/// Journal entry for effect commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Fact to record in journal
    pub fact: Fact,
    /// Authority making the entry
    pub authority: AuthorityId,
    /// Timestamp of entry
    pub timestamp: TimeStamp,
}

/// Result of effect execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectResult {
    /// Command executed successfully
    Success,
    /// Command failed with error
    Failure(String),
    /// Budget charge produced a receipt
    Receipt(Receipt),
    /// Generated nonce bytes
    Nonce(Vec<u8>),
    /// Remaining flow budget after charge
    RemainingBudget(u32),
}

/// Asynchronous effect interpreter trait
///
/// Implementations execute effect commands according to their environment:
/// - Production: Real I/O operations
/// - Simulation: Deterministic event recording
/// - Testing: Mock responses
#[async_trait]
pub trait EffectInterpreter: Send + Sync {
    /// Execute an effect command asynchronously
    async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult>;

    /// Get interpreter type for debugging
    fn interpreter_type(&self) -> &'static str;
}

/// Simulation events for deterministic replay
///
/// These events capture all observable side effects from effect execution,
/// enabling replay, analysis, and property checking in simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimulationEvent {
    /// Flow budget was charged
    BudgetCharged {
        /// Event timestamp
        time: TimeStamp,
        /// Authority that was charged
        authority: AuthorityId,
        /// Amount charged
        amount: u32,
        /// Remaining budget after charge
        remaining: u32,
    },
    /// Journal entry was appended
    JournalAppended {
        /// Event timestamp
        time: TimeStamp,
        /// Entry that was appended
        entry: JournalEntry,
    },
    /// Metadata leakage was recorded
    LeakageRecorded {
        /// Event timestamp
        time: TimeStamp,
        /// Bits of metadata leaked
        bits: u32,
    },
    /// Metadata was stored
    MetadataStored {
        /// Event timestamp
        time: TimeStamp,
        /// Storage key
        key: String,
        /// Storage value
        value: String,
    },
    /// Network envelope was queued
    EnvelopeQueued {
        /// Event timestamp
        time: TimeStamp,
        /// Source address
        from: NetworkAddress,
        /// Target address
        to: NetworkAddress,
        /// Envelope content
        envelope: Vec<u8>,
    },
    /// Nonce was generated
    NonceGenerated {
        /// Event timestamp
        time: TimeStamp,
        /// Generated bytes
        nonce: Vec<u8>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_budget_view() {
        let mut budgets = HashMap::new();
        let authority = AuthorityId::new();
        let context = ContextId::default();
        budgets.insert((context, authority), 1000);

        let view = FlowBudgetView::new(budgets);

        assert_eq!(view.get(&context, &authority), Some(1000));
        assert!(view.has_budget(&context, &authority, 500));
        assert!(!view.has_budget(&context, &authority, 2000));

        let unknown = AuthorityId::new();
        assert_eq!(view.get(&context, &unknown), None);
    }

    #[test]
    fn test_metadata_view() {
        let mut metadata = HashMap::new();
        metadata.insert("key1".to_string(), "value1".to_string());

        let view = MetadataView::new(metadata);

        assert_eq!(view.get("key1"), Some("value1"));
        assert_eq!(view.get("key2"), None);
        assert!(view.contains("key1"));
        assert!(!view.contains("key2"));
    }

    #[test]
    fn test_decision() {
        let auth = Decision::Authorized;
        assert!(auth.is_authorized());
        assert_eq!(auth.denial_reason(), None);

        let denied = Decision::Denied("Insufficient budget".to_string());
        assert!(!denied.is_authorized());
        assert_eq!(denied.denial_reason(), Some("Insufficient budget"));
    }

    #[test]
    fn test_guard_outcome() {
        let effects = vec![EffectCommand::ChargeBudget {
            context: ContextId::new(),
            authority: AuthorityId::new(),
            peer: AuthorityId::new(),
            amount: 100,
        }];

        let authorized = GuardOutcome::authorized(effects);
        assert!(authorized.is_authorized());
        assert_eq!(authorized.effects.len(), 1);

        let denied = GuardOutcome::denied("Not allowed");
        assert!(!denied.is_authorized());
        assert!(denied.effects.is_empty());
    }

    #[test]
    fn test_effect_command_serialization() {
        let cmd = EffectCommand::ChargeBudget {
            context: ContextId::new(),
            authority: AuthorityId::new(),
            peer: AuthorityId::new(),
            amount: 100,
        };

        let serialized = bincode::serialize(&cmd).unwrap();
        let deserialized: EffectCommand = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            EffectCommand::ChargeBudget { amount, .. } => assert_eq!(amount, 100),
            _ => panic!("Wrong command type"),
        }
    }
}
