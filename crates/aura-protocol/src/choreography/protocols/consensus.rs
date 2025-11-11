//! Consensus Choreography
//!
//! This module implements choreographic protocols for Byzantine fault-tolerant consensus
//! using rumpsteak-aura DSL following the protocol guide design principles.
//!
//! ## Protocol Flow
//!
//! 1. Leader → Voter[N]: Broadcast proposal
//! 2. Each Voter[i] → Leader: Send vote (approve/reject)
//! 3. Leader → Voter[N]: Broadcast decision

use crate::effects::ChoreographyError;
use crate::effects::{ConsoleEffects, CryptoEffects, RandomEffects};
use aura_core::{DeviceId, SessionId};
use rumpsteak_aura_choreography::choreography;
use serde::{Deserialize, Serialize};

/// Consensus choreography configuration
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    pub participants: Vec<DeviceId>,
    pub proposal: Vec<u8>,
    pub threshold: usize, // Minimum votes needed for consensus
}

/// Consensus choreography result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusResult {
    pub consensus_reached: bool,
    pub votes_for: usize,
    pub votes_against: usize,
    pub decided_value: Option<Vec<u8>>,
    pub success: bool, // For ProtocolResult compatibility
}

/// Consensus error types
#[derive(Debug, thiserror::Error)]
pub enum ConsensusError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Consensus failed: {0}")]
    ConsensusFailed(String),
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
}

/// Message types for consensus choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusProposal {
    pub session_id: SessionId,
    pub leader_id: DeviceId,
    pub proposal: Vec<u8>,
    pub round: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusVote {
    pub session_id: SessionId,
    pub voter_id: DeviceId,
    pub approve: bool,
    pub round: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusDecision {
    pub session_id: SessionId,
    pub consensus_reached: bool,
    pub decided_value: Option<Vec<u8>>,
    pub votes_for: usize,
    pub votes_against: usize,
}

/// Consensus choreography for Byzantine fault-tolerant consensus
///
/// N-party protocol:
/// - Leader proposes value to all N voters
/// - Each voter sends vote (approve/reject)
/// - Leader decides based on threshold and broadcasts decision
// TEMPORARILY DISABLED DUE TO MACRO CONFLICTS - needs investigation
/*
choreography! {
    protocol Consensus {
        roles: Leader, Voter1, Voter2, Voter3;

        // Round 1: Leader broadcasts proposal to all voters
        Leader -> Voter1: ConsensusSendProposal(ConsensusProposal);
        Leader -> Voter2: ConsensusSendProposal(ConsensusProposal);
        Leader -> Voter3: ConsensusSendProposal(ConsensusProposal);

        // Round 2: Voters send votes to leader
        Voter1 -> Leader: ConsensusSendVote(ConsensusVote);
        Voter2 -> Leader: ConsensusSendVote(ConsensusVote);
        Voter3 -> Leader: ConsensusSendVote(ConsensusVote);

        // Round 3: Leader broadcasts decision to all voters
        Leader -> Voter1: ConsensusSendDecision(ConsensusDecision);
        Leader -> Voter2: ConsensusSendDecision(ConsensusDecision);
        Leader -> Voter3: ConsensusSendDecision(ConsensusDecision);
    }
}
*/

/// Execute consensus protocol
pub async fn execute_consensus(
    device_id: DeviceId,
    config: ConsensusConfig,
    is_leader: bool,
    voter_index: Option<usize>,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<ConsensusResult, ConsensusError> {
    // Validate configuration
    let n = config.participants.len();

    if n < 2 {
        return Err(ConsensusError::InvalidConfig(format!(
            "Consensus requires at least 2 participants, got {}",
            n
        )));
    }

    if config.threshold == 0 || config.threshold > n {
        return Err(ConsensusError::InvalidConfig(format!(
            "Invalid threshold: {} (must be 1..={})",
            config.threshold, n
        )));
    }

    // Create handler adapter
    let mut adapter = crate::choreography::AuraHandlerAdapter::new(
        device_id,
        effect_system.execution_mode(),
    );

    // Execute appropriate role
    if is_leader {
        let voters: Vec<DeviceId> = config
            .participants
            .iter()
            .filter(|&&id| id != device_id)
            .copied()
            .collect();

        leader_session(&mut adapter, &voters, &config).await
    } else {
        let leader_id = config.participants[0];
        let voter_idx = voter_index
            .ok_or_else(|| ConsensusError::InvalidConfig("Voter must have index".to_string()))?;

        voter_session(&mut adapter, leader_id, voter_idx, &config).await
    }
}

/// Leader's role in consensus protocol
async fn leader_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    voters: &[DeviceId],
    config: &ConsensusConfig,
) -> Result<ConsensusResult, ConsensusError> {
    let session_id = SessionId::new();
    let round = 1;

    // Round 1: Broadcast proposal to all voters
    let proposal = ConsensusProposal {
        session_id: session_id.clone(),
        leader_id: adapter.device_id(),
        proposal: config.proposal.clone(),
        round,
    };

    for voter_id in voters {
        adapter
            .send(*voter_id, proposal.clone())
            .await
            .map_err(|e| {
                ConsensusError::Communication(format!("Failed to send proposal: {}", e))
            })?;
    }

    // Round 2: Collect votes from all voters
    let mut votes_for = 0;
    let mut votes_against = 0;

    for voter_id in voters {
        let vote: ConsensusVote = adapter
            .recv_from(*voter_id)
            .await
            .map_err(|e| ConsensusError::Communication(format!("Failed to receive vote: {}", e)))?;

        if vote.session_id != session_id || vote.round != round {
            continue; // Ignore invalid votes
        }

        if vote.approve {
            votes_for += 1;
        } else {
            votes_against += 1;
        }
    }

    // Round 3: Decide and broadcast decision
    let consensus_reached = votes_for >= config.threshold;
    let decided_value = if consensus_reached {
        Some(config.proposal.clone())
    } else {
        None
    };

    let decision = ConsensusDecision {
        session_id,
        consensus_reached,
        decided_value: decided_value.clone(),
        votes_for,
        votes_against,
    };

    for voter_id in voters {
        adapter
            .send(*voter_id, decision.clone())
            .await
            .map_err(|e| {
                ConsensusError::Communication(format!("Failed to send decision: {}", e))
            })?;
    }

    Ok(ConsensusResult {
        consensus_reached,
        votes_for,
        votes_against,
        decided_value,
        success: consensus_reached,
    })
}

/// Voter's role in consensus protocol
async fn voter_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    leader_id: DeviceId,
    _voter_index: usize,
    config: &ConsensusConfig,
) -> Result<ConsensusResult, ConsensusError> {
    // Round 1: Receive proposal from leader
    let proposal: ConsensusProposal = adapter
        .recv_from(leader_id)
        .await
        .map_err(|e| ConsensusError::Communication(format!("Failed to receive proposal: {}", e)))?;

    // Round 2: Validate proposal and send vote
    // TODO fix - Simplified validation: check if proposal matches expected value
    let approve = proposal.proposal == config.proposal;

    let vote = ConsensusVote {
        session_id: proposal.session_id.clone(),
        voter_id: adapter.device_id(),
        approve,
        round: proposal.round,
    };

    adapter
        .send(leader_id, vote)
        .await
        .map_err(|e| ConsensusError::Communication(format!("Failed to send vote: {}", e)))?;

    // Round 3: Receive decision from leader
    let decision: ConsensusDecision = adapter
        .recv_from(leader_id)
        .await
        .map_err(|e| ConsensusError::Communication(format!("Failed to receive decision: {}", e)))?;

    Ok(ConsensusResult {
        consensus_reached: decision.consensus_reached,
        votes_for: decision.votes_for,
        votes_against: decision.votes_against,
        decided_value: decision.decided_value,
        success: decision.consensus_reached,
    })
}
