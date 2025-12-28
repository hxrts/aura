//! Protocol-level relational facts.
//!
//! These facts are owned by `aura-journal` because they participate directly in
//! reduction semantics and cross-domain invariants. Domain facts must use
//! `RelationalFact::Generic` + `FactRegistry` instead.

use crate::fact::{
    ChannelCheckpoint, ChannelPolicy, CommittedChannelEpochBump, LeakageFact,
    ProposedChannelEpochBump,
};
use aura_core::{AuthorityId, Hash32};
use serde::{Deserialize, Serialize};

/// Protocol-level relational facts that must remain in `aura-journal`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProtocolRelationalFact {
    /// Guardian binding established between two authorities
    GuardianBinding {
        /// Account being bound to a guardian
        account_id: AuthorityId,
        /// Guardian authority
        guardian_id: AuthorityId,
        /// Hash of the binding agreement
        binding_hash: Hash32,
    },
    /// Recovery grant issued by a guardian
    RecoveryGrant {
        /// Account that can be recovered
        account_id: AuthorityId,
        /// Guardian granting recovery capability
        guardian_id: AuthorityId,
        /// Hash of the grant details
        grant_hash: Hash32,
    },
    /// Consensus result from Aura Consensus
    Consensus {
        /// Consensus operation identifier (as Hash32 to avoid circular dependency)
        consensus_id: Hash32,
        /// Hash of the operation being consensus'd
        operation_hash: Hash32,
        /// Whether consensus threshold was met
        threshold_met: bool,
        /// Number of participants in the consensus
        participant_count: u16,
    },
    /// AMP channel checkpoint anchoring ratchet windows
    AmpChannelCheckpoint(ChannelCheckpoint),
    /// Proposed channel epoch bump (optimistic)
    AmpProposedChannelEpochBump(ProposedChannelEpochBump),
    /// Committed channel epoch bump (final)
    AmpCommittedChannelEpochBump(CommittedChannelEpochBump),
    /// Channel policy overrides
    AmpChannelPolicy(ChannelPolicy),
    /// Leakage tracking event (privacy budget accounting)
    LeakageEvent(LeakageFact),
}
