//! CRDT choreographic protocols using rumpsteak-aura DSL
//!
//! This module implements the session-type protocols specified in docs/402_crdt_types.md
//! using the `choreography!` macro. These protocols bridge between session types and
//! CRDT semantic laws enforced by effect handlers.

use rumpsteak_choreography::choreography;
use crate::types::ChoreographicRole;
use crate::runtime::AuraHandlerAdapter;

// Re-import message types from foundation
use aura_types::semilattice::{StateMsg, DeltaMsg, OpWithCtx};

/// State-based CRDT anti-entropy choreography
///
/// Implements the session type: `CvSync<S> := μX . (A → B : StateMsg<S> . X) ∥ (B → A : StateMsg<S> . X)`
/// 
/// Each replica periodically exchanges its full state with others.
/// The choreography ensures eventual consistency through anti-entropy.
choreography! {
    CvSync {
        roles: Replica[N]
        
        /// Anti-entropy round where all replicas exchange states
        protocol AntiEntropyRound {
            loop (count: N) {
                // Each replica broadcasts its state to all others
                Replica[i] -> Replica[*]: StateMsg
            }
        }
        
        call AntiEntropyRound
    }
}

/// Delta-based CRDT gossip choreography
///
/// Implements bandwidth-optimized synchronization by exchanging
/// deltas rather than full states.
choreography! {
    DeltaSync {
        roles: Replica[N]
        
        /// Gossip round for delta exchange
        protocol GossipRound {
            loop (count: N) {
                // Each replica shares its recent deltas
                Replica[i] -> Replica[*]: DeltaMsg
            }
        }
        
        call GossipRound
    }
}

/// Operation-based CRDT broadcast choreography
///
/// Implements the session type: `OpBroadcast<Op, Ctx> := μX . r ⊳ { issue : r → * : OpWithCtx . X, idle : end }`
///
/// Replicas can choose to issue operations which are broadcast to all others,
/// or remain idle. Operations carry causal context for proper ordering.
choreography! {
    OpBroadcast {
        roles: Issuer, Replica[N]
        
        /// Operation broadcast phase with issuer choice
        protocol OperationPhase {
            choice Issuer {
                issue: {
                    // Issuer broadcasts operation to all replicas
                    Issuer -> Replica[*]: OpWithCtx
                }
                idle: {
                    // No operation this round
                }
            }
        }
        
        call OperationPhase
    }
}

/// Multi-replica operation broadcast choreography
///
/// Extends OpBroadcast to allow any replica to issue operations.
/// This provides a more symmetric protocol for peer-to-peer networks.
choreography! {
    MultiOpBroadcast {
        roles: Replica[N]
        
        /// Multi-issuer operation phase
        protocol MultiOperationPhase {
            loop (count: N) {
                choice Replica[i] {
                    issue: {
                        // This replica issues an operation
                        Replica[i] -> Replica[*]: OpWithCtx
                    }
                    idle: {
                        // This replica has no operation to issue
                    }
                }
            }
        }
        
        call MultiOperationPhase
    }
}

/// Repair protocol for missing operations
///
/// Implements the session type: `OpRepair<Id, Op> := μX . A ⊳ { pull : A → B : Digest<Id> . B → A : Missing<Op> . X, idle : end }`
///
/// Replicas can request missing operations by exchanging digests.
choreography! {
    OpRepair {
        roles: Requester, Provider
        
        /// Repair request phase
        protocol RepairPhase {
            choice Requester {
                pull: {
                    // Request operations by sending digest
                    Requester -> Provider: Digest
                    // Provider responds with missing operations  
                    Provider -> Requester: Missing
                }
                idle: {
                    // No repair needed
                }
            }
        }
        
        call RepairPhase
    }
}

/// Hierarchical CRDT synchronization choreography
///
/// Implements a two-tier synchronization pattern where replicas
/// first synchronize within clusters, then cluster leaders synchronize.
choreography! {
    HierarchicalSync {
        roles: Replica[N], Leader[M]
        
        /// Intra-cluster synchronization
        protocol ClusterSync {
            loop (count: N) {
                Replica[i] -> Replica[*]: StateMsg
            }
        }
        
        /// Inter-cluster leader synchronization  
        protocol LeaderSync {
            loop (count: M) {
                Leader[i] -> Leader[*]: StateMsg
            }
        }
        
        /// Combined hierarchical protocol
        protocol FullSync {
            call ClusterSync
            call LeaderSync
            call ClusterSync  // Final propagation to all replicas
        }
        
        call FullSync
    }
}

/// Epidemic-style gossip choreography
///
/// Implements probabilistic gossip where replicas randomly select
/// peers for state exchange, providing good scalability properties.
choreography! {
    EpidemicGossip {
        roles: Replica[N]
        
        /// Random pairwise gossip round
        protocol GossipRound {
            // Note: In practice, pair selection would be randomized
            // This shows the communication pattern for selected pairs
            loop (count: N/2) {
                // Pairwise state exchange
                Replica[2*i] -> Replica[2*i + 1]: StateMsg
                Replica[2*i + 1] -> Replica[2*i]: StateMsg
            }
        }
        
        /// Multiple gossip rounds for epidemic spreading
        protocol EpidemicRounds {
            call GossipRound
            call GossipRound
            call GossipRound
        }
        
        call EpidemicRounds
    }
}

/// Consensus-based CRDT choreography
///
/// Implements a hybrid approach where critical operations go through
/// consensus while regular operations use standard CRDT broadcast.
choreography! {
    ConsensusCRDT {
        roles: Participant[N], Leader
        
        /// Consensus phase for critical operations
        protocol ConsensusPhase {
            choice Leader {
                propose: {
                    // Leader proposes a critical operation
                    Leader -> Participant[*]: OpWithCtx
                    
                    // Participants respond with votes
                    loop (count: N) {
                        Participant[i] -> Leader: Vote
                    }
                    
                    // Leader announces decision
                    Leader -> Participant[*]: Decision
                }
                skip: {
                    // No consensus needed this round
                }
            }
        }
        
        /// Regular CRDT operations
        protocol RegularOps {
            loop (count: N) {
                choice Participant[i] {
                    issue: {
                        Participant[i] -> Participant[*]: OpWithCtx
                    }
                    idle: {
                        // No operation
                    }
                }
            }
        }
        
        /// Combined protocol
        protocol FullProtocol {
            call ConsensusPhase
            call RegularOps
        }
        
        call FullProtocol
    }
}

// Protocol-specific message types for consensus
use serde::{Serialize, Deserialize};

/// Vote message for consensus phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    /// Participant casting the vote
    pub participant: ChoreographicRole,
    /// Vote decision (approve/reject)
    pub approve: bool,
    /// Optional justification
    pub reason: Option<String>,
}

/// Decision message from consensus leader
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Whether the proposed operation was accepted
    pub accepted: bool,
    /// Vote tally
    pub votes_for: u32,
    pub votes_against: u32,
}

// Type aliases for common digest and missing operation types
/// Operation ID digest for repair protocols
pub type Digest<Id> = Vec<Id>;

/// Missing operations response
pub type Missing<Op> = Vec<Op>;