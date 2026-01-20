//! Protocol instance state management
//!
//! This module contains the per-instance state for consensus protocol execution.

use crate::{
    core::{self, ConsensusPhase as CorePhase, ConsensusState as CoreState},
    messages::ConsensusPhase,
    witness::WitnessTracker,
    ConsensusId,
};
use aura_core::{crypto::tree_signing::NonceToken, frost::Share, AuthorityId, Hash32};

/// State for a single protocol instance
pub(crate) struct ProtocolInstance {
    pub consensus_id: ConsensusId,
    pub prestate_hash: Hash32,
    pub operation_hash: Hash32,
    pub operation_bytes: Vec<u8>,
    pub role: ProtocolRole,
    pub tracker: WitnessTracker,
    pub phase: ConsensusPhase,
    pub start_time_ms: u64,
    /// Cached nonce token for signing (slow path)
    pub nonce_token: Option<NonceToken>,
    /// Pure core state for invariant validation
    /// Quint: protocol_consensus.qnt / Lean: Aura.Consensus.Types
    pub core_state: CoreState,
}

impl ProtocolInstance {
    /// Convert effectful phase to pure core phase
    /// Quint: ConsensusPhase / Lean: Aura.Consensus.Types.ConsensusPhase
    pub fn to_core_phase(&self) -> CorePhase {
        match self.phase {
            ConsensusPhase::Execute => CorePhase::FastPathActive,
            ConsensusPhase::NonceCommit => CorePhase::FastPathActive,
            ConsensusPhase::Sign => CorePhase::FastPathActive,
            ConsensusPhase::Result => CorePhase::Committed,
        }
    }

    /// Synchronize pure core state with effectful state
    pub fn sync_core_state(&mut self) {
        self.core_state.phase = self.to_core_phase();
        // Sync proposals from tracker
        let participants = self.tracker.get_participants();
        let signatures = self.tracker.get_signatures();

        self.core_state.proposals = participants
            .iter()
            .zip(signatures.iter())
            .map(|(witness, sig)| core::ShareProposal {
                witness: *witness,
                result_id: self.operation_hash,
                share: core::ShareData {
                    share_value: hex::encode(&sig.signature),
                    nonce_binding: format!("nonce:{}", sig.signer),
                    data_binding: format!(
                        "{}:{}:{}",
                        self.consensus_id, self.operation_hash, self.prestate_hash
                    ),
                },
            })
            .collect();
    }

    /// Check invariants after state transitions (debug mode only)
    pub fn assert_invariants(&self) {
        debug_assert!(
            core::check_invariants(&self.core_state).is_ok(),
            "Consensus invariant violation: {:?}",
            core::check_invariants(&self.core_state).err()
        );
    }
}

/// Role in the protocol (coordinator or witness)
pub(crate) enum ProtocolRole {
    Coordinator {
        witness_set: crate::witness::WitnessSet,
    },
    Witness {
        coordinator: AuthorityId,
        my_share: Share,
    },
}
