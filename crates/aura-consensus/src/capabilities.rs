#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "consensus")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsensusCapability {
    #[capability("initiate")]
    Initiate,
    #[capability("witness_nonce")]
    WitnessNonce,
    #[capability("aggregate_nonces")]
    AggregateNonces,
    #[capability("witness_sign")]
    WitnessSign,
    #[capability("finalize")]
    Finalize,
}

pub fn evaluation_candidates_for_consensus_protocol() -> &'static [ConsensusCapability] {
    ConsensusCapability::declared_names()
}
