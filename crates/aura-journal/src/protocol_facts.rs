//! Protocol-level relational facts.
//!
//! These facts are owned by `aura-journal` because they participate directly in
//! reduction semantics and cross-domain invariants. Domain facts must use
//! `RelationalFact::Generic` + `FactRegistry` instead.

use crate::fact::{
    ChannelBootstrap, ChannelCheckpoint, ChannelPolicy, CommittedChannelEpochBump, ConvergenceCert,
    DkgTranscriptCommit, LeakageFact, ProposedChannelEpochBump, ProtocolFactKey, ReversionFact,
    RotateFact,
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
    /// AMP channel bootstrap metadata (dealer key)
    AmpChannelBootstrap(ChannelBootstrap),
    /// Leakage tracking event (privacy budget accounting)
    LeakageEvent(LeakageFact),
    /// Finalized DKG transcript commit
    DkgTranscriptCommit(DkgTranscriptCommit),
    /// Coordinator convergence certificate (soft-safe)
    ConvergenceCert(ConvergenceCert),
    /// Explicit reversion fact (soft-safe)
    ReversionFact(ReversionFact),
    /// Rotation/upgrade marker for lifecycle transitions
    RotateFact(RotateFact),
    /// Equivocation proof showing malicious consensus behavior
    EquivocationProof(crate::fact::EquivocationProof),
}

impl ProtocolRelationalFact {
    /// Stable reducer key for this protocol fact.
    pub fn binding_key(&self) -> ProtocolFactKey {
        match self {
            ProtocolRelationalFact::GuardianBinding {
                account_id,
                guardian_id,
                binding_hash,
            } => ProtocolFactKey::GuardianBinding {
                account_id: *account_id,
                guardian_id: *guardian_id,
                binding_hash: *binding_hash,
            },
            ProtocolRelationalFact::RecoveryGrant {
                account_id,
                guardian_id,
                grant_hash,
            } => ProtocolFactKey::RecoveryGrant {
                account_id: *account_id,
                guardian_id: *guardian_id,
                grant_hash: *grant_hash,
            },
            ProtocolRelationalFact::Consensus {
                consensus_id,
                operation_hash,
                ..
            } => ProtocolFactKey::Consensus {
                consensus_id: *consensus_id,
                operation_hash: *operation_hash,
            },
            ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint) => {
                ProtocolFactKey::AmpChannelCheckpoint {
                    channel: checkpoint.channel,
                    chan_epoch: checkpoint.chan_epoch,
                    ck_commitment: checkpoint.ck_commitment,
                }
            }
            ProtocolRelationalFact::AmpProposedChannelEpochBump(bump) => {
                ProtocolFactKey::AmpProposedChannelEpochBump {
                    channel: bump.channel,
                    parent_epoch: bump.parent_epoch,
                    new_epoch: bump.new_epoch,
                    bump_id: bump.bump_id,
                }
            }
            ProtocolRelationalFact::AmpCommittedChannelEpochBump(bump) => {
                ProtocolFactKey::AmpCommittedChannelEpochBump {
                    channel: bump.channel,
                    parent_epoch: bump.parent_epoch,
                    new_epoch: bump.new_epoch,
                    chosen_bump_id: bump.chosen_bump_id,
                }
            }
            ProtocolRelationalFact::AmpChannelPolicy(policy) => ProtocolFactKey::AmpChannelPolicy {
                channel: policy.channel,
            },
            ProtocolRelationalFact::AmpChannelBootstrap(bootstrap) => {
                ProtocolFactKey::AmpChannelBootstrap {
                    channel: bootstrap.channel,
                    bootstrap_id: bootstrap.bootstrap_id,
                }
            }
            ProtocolRelationalFact::LeakageEvent(event) => ProtocolFactKey::LeakageEvent {
                source: event.source,
                destination: event.destination,
                timestamp: event.timestamp.clone(),
            },
            ProtocolRelationalFact::DkgTranscriptCommit(commit) => {
                ProtocolFactKey::DkgTranscriptCommit {
                    transcript_hash: commit.transcript_hash,
                }
            }
            ProtocolRelationalFact::ConvergenceCert(cert) => {
                ProtocolFactKey::ConvergenceCert { op_id: cert.op_id }
            }
            ProtocolRelationalFact::ReversionFact(reversion) => ProtocolFactKey::ReversionFact {
                op_id: reversion.op_id,
            },
            ProtocolRelationalFact::RotateFact(rotate) => ProtocolFactKey::RotateFact {
                to_state: rotate.to_state,
            },
            ProtocolRelationalFact::EquivocationProof(proof) => {
                ProtocolFactKey::EquivocationProof {
                    witness: proof.witness,
                    consensus_id: proof.consensus_id,
                    first_result_id: proof.first_result_id,
                    second_result_id: proof.second_result_id,
                }
            }
        }
    }
}
