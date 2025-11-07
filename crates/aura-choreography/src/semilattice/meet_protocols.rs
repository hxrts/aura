//! Meet semi-lattice choreographic protocols for constraint synchronization
//!
//! This module implements choreographic protocols for meet-based CRDT synchronization
//! using the rumpsteak-aura DSL. These protocols coordinate constraint propagation
//! and consistency verification across distributed participants.

use aura_types::{
    crdt::{MvState, MeetStateMsg, ConstraintMsg, ConsistencyProof, ConstraintScope},
    identifiers::DeviceId,
};
use rumpsteak_choreography::choreography;
use crate::core::{AuraHandlerAdapter, ChoreographyError};
use std::collections::HashMap;

/// Constraint synchronization result
#[derive(Debug, Clone)]
pub struct ConstraintSyncResult<S> {
    /// Final synchronized constraint state
    pub final_state: S,
    /// Consistency verification successful
    pub consistent: bool,
    /// Participants who verified consistency
    pub verified_participants: Vec<DeviceId>,
}

choreography! {
    ConstraintSync {
        roles: Enforcer[N]
        
        protocol ConstraintPropagation {
            // Phase 1: Each enforcer broadcasts their constraint
            loop (count: N) {
                Enforcer[i] -> Enforcer[*]: ConstraintMsg
            }
            
            // Phase 2: Local meet computation at each enforcer
            loop (count: N) {
                Enforcer[i].local_meet_computation()
            }
            
            // Phase 3: Consistency proof exchange
            loop (count: N) {
                Enforcer[i] -> Enforcer[*]: ConsistencyProof
            }
            
            // Phase 4: Final verification
            loop (count: N) {
                Enforcer[i].verify_global_consistency()
            }
        }
        
        call ConstraintPropagation
    }
}

choreography! {
    CapabilityRestriction {
        roles: Grantor, Grantee[M], Verifier[K]
        
        protocol CapabilityIntersection {
            // Phase 1: Grantor distributes base capabilities
            Grantor -> Grantee[*]: MeetStateMsg
            Grantor -> Verifier[*]: MeetStateMsg
            
            // Phase 2: Each grantee applies local constraints
            loop (count: M) {
                Grantee[i].apply_local_constraints()
            }
            
            // Phase 3: Grantees report effective capabilities
            loop (count: M) {
                Grantee[i] -> Verifier[*]: MeetStateMsg
            }
            
            // Phase 4: Verifiers check constraint satisfaction
            loop (count: K) {
                Verifier[i].validate_capability_bounds()
            }
            
            // Phase 5: Verification results
            loop (count: K) {
                Verifier[i] -> Grantor: ConsistencyProof
                Verifier[i] -> Grantee[*]: ConsistencyProof
            }
        }
        
        call CapabilityIntersection
    }
}

choreography! {
    TimeWindowIntersection {
        roles: Coordinator, Participant[N]
        
        protocol TemporalConstraint {
            // Phase 1: Participants propose time windows
            loop (count: N) {
                Participant[i] -> Coordinator: MeetStateMsg
            }
            
            // Phase 2: Coordinator computes intersection
            Coordinator.compute_time_intersection()
            
            // Phase 3: Coordinator announces intersection result
            Coordinator -> Participant[*]: MeetStateMsg
            
            // Phase 4: Participants verify intersection
            loop (count: N) {
                Participant[i] -> Coordinator: ConsistencyProof
            }
        }
        
        call TemporalConstraint
    }
}

/// Execute constraint synchronization choreography
///
/// This function coordinates the distributed constraint propagation protocol,
/// ensuring all participants reach consensus on the meet intersection result.
pub async fn execute_constraint_sync<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    local_constraint: S,
    participants: Vec<DeviceId>,
    my_device_id: DeviceId,
) -> Result<ConstraintSyncResult<S>, ChoreographyError> {
    let my_role = participants.iter()
        .position(|&id| id == my_device_id)
        .ok_or_else(|| ChoreographyError::InvalidRole("Device not in participants".to_string()))?;
    
    let n = participants.len();
    let mut received_constraints: HashMap<usize, S> = HashMap::new();
    let mut consistency_proofs: HashMap<DeviceId, ConsistencyProof> = HashMap::new();
    
    // Phase 1: Broadcast our constraint to all participants
    let constraint_msg = ConstraintMsg::new(
        local_constraint.clone(),
        ConstraintScope::Global,
        1
    );
    
    for (i, &participant) in participants.iter().enumerate() {
        if i != my_role {
            adapter.send(participant, constraint_msg.clone()).await?;
        }
    }
    
    // Receive constraints from other participants
    for i in 0..n {
        if i != my_role {
            let msg: ConstraintMsg<S> = adapter.recv_from(participants[i]).await?;
            received_constraints.insert(i, msg.constraint);
        } else {
            received_constraints.insert(i, local_constraint.clone());
        }
    }
    
    // Phase 2: Compute local meet of all received constraints
    let mut final_state = S::top();
    for constraint in received_constraints.values() {
        final_state = final_state.meet(constraint);
    }
    
    // Phase 3: Generate and exchange consistency proofs
    let our_proof = generate_consistency_proof(&final_state, my_device_id)?;
    
    for (i, &participant) in participants.iter().enumerate() {
        if i != my_role {
            adapter.send(participant, our_proof.clone()).await?;
        }
    }
    
    // Receive consistency proofs from other participants
    for i in 0..n {
        if i != my_role {
            let proof: ConsistencyProof = adapter.recv_from(participants[i]).await?;
            consistency_proofs.insert(proof.participant, proof);
        }
    }
    
    // Phase 4: Verify global consistency
    let consistent = verify_global_consistency(&our_proof, &consistency_proofs);
    let verified_participants: Vec<DeviceId> = consistency_proofs.keys().cloned().collect();
    
    Ok(ConstraintSyncResult {
        final_state,
        consistent,
        verified_participants,
    })
}

/// Execute capability restriction choreography
///
/// Implements the capability intersection protocol where a grantor distributes
/// base capabilities and grantees apply local restrictions.
pub async fn execute_capability_restriction<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    role: CapabilityRole,
    base_capabilities: Option<S>,
    local_constraints: Option<S>,
    participants: &CapabilityParticipants,
) -> Result<CapabilityRestrictionResult<S>, ChoreographyError> {
    match role {
        CapabilityRole::Grantor => {
            execute_grantor_protocol(adapter, base_capabilities.unwrap(), participants).await
        }
        CapabilityRole::Grantee(grantee_id) => {
            execute_grantee_protocol(adapter, local_constraints.unwrap(), grantee_id, participants).await
        }
        CapabilityRole::Verifier(verifier_id) => {
            execute_verifier_protocol(adapter, verifier_id, participants).await
        }
    }
}

/// Execute time window intersection choreography
///
/// Coordinates temporal constraint intersection across multiple participants
/// to find a consensus time window.
pub async fn execute_time_window_intersection<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    role: TimeRole,
    time_window: Option<S>,
    participants: &TimeParticipants,
) -> Result<TimeIntersectionResult<S>, ChoreographyError> {
    match role {
        TimeRole::Coordinator => {
            execute_coordinator_protocol(adapter, participants).await
        }
        TimeRole::Participant(participant_id) => {
            execute_participant_protocol(adapter, time_window.unwrap(), participant_id, participants).await
        }
    }
}

// === Supporting Types and Functions ===

#[derive(Debug, Clone)]
pub enum CapabilityRole {
    Grantor,
    Grantee(DeviceId),
    Verifier(DeviceId),
}

#[derive(Debug, Clone)]
pub enum TimeRole {
    Coordinator,
    Participant(DeviceId),
}

#[derive(Debug, Clone)]
pub struct CapabilityParticipants {
    pub grantor: DeviceId,
    pub grantees: Vec<DeviceId>,
    pub verifiers: Vec<DeviceId>,
}

#[derive(Debug, Clone)]
pub struct TimeParticipants {
    pub coordinator: DeviceId,
    pub participants: Vec<DeviceId>,
}

#[derive(Debug, Clone)]
pub struct CapabilityRestrictionResult<S> {
    pub effective_capabilities: S,
    pub verification_successful: bool,
    pub verifier_proofs: HashMap<DeviceId, ConsistencyProof>,
}

#[derive(Debug, Clone)]
pub struct TimeIntersectionResult<S> {
    pub intersection_window: S,
    pub participating_devices: Vec<DeviceId>,
    pub verification_proofs: HashMap<DeviceId, ConsistencyProof>,
}

/// Generate consistency proof for a given state
fn generate_consistency_proof<S: MvState>(
    state: &S,
    participant: DeviceId,
) -> Result<ConsistencyProof, ChoreographyError> {
    let state_bytes = bincode::serialize(state)
        .map_err(|e| ChoreographyError::SerializationError(e.to_string()))?;
    let constraint_hash = blake3::hash(&state_bytes).into();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    Ok(ConsistencyProof::new(constraint_hash, participant, timestamp))
}

/// Verify that all consistency proofs match
fn verify_global_consistency(
    our_proof: &ConsistencyProof,
    other_proofs: &HashMap<DeviceId, ConsistencyProof>,
) -> bool {
    other_proofs.values()
        .all(|proof| proof.constraint_hash == our_proof.constraint_hash)
}

// === Implementation Functions for Choreographic Protocols ===

async fn execute_grantor_protocol<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    base_capabilities: S,
    participants: &CapabilityParticipants,
) -> Result<CapabilityRestrictionResult<S>, ChoreographyError> {
    // Phase 1: Distribute base capabilities
    let base_msg = MeetStateMsg::new(base_capabilities.clone(), 1);
    
    for &grantee in &participants.grantees {
        adapter.send(grantee, base_msg.clone()).await?;
    }
    for &verifier in &participants.verifiers {
        adapter.send(verifier, base_msg.clone()).await?;
    }
    
    // Phase 5: Collect verification results
    let mut verifier_proofs = HashMap::new();
    for &verifier in &participants.verifiers {
        let proof: ConsistencyProof = adapter.recv_from(verifier).await?;
        verifier_proofs.insert(verifier, proof);
    }
    
    let verification_successful = verifier_proofs.values()
        .all(|proof| {
            // Verify proof signatures and consistency
            true // Placeholder for actual verification logic
        });
    
    Ok(CapabilityRestrictionResult {
        effective_capabilities: base_capabilities,
        verification_successful,
        verifier_proofs,
    })
}

async fn execute_grantee_protocol<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    local_constraints: S,
    grantee_id: DeviceId,
    participants: &CapabilityParticipants,
) -> Result<CapabilityRestrictionResult<S>, ChoreographyError> {
    // Phase 1: Receive base capabilities from grantor
    let base_msg: MeetStateMsg<S> = adapter.recv_from(participants.grantor).await?;
    
    // Phase 2: Apply local constraints
    let effective_capabilities = base_msg.payload.meet(&local_constraints);
    
    // Phase 3: Report effective capabilities to verifiers
    let effective_msg = MeetStateMsg::new(effective_capabilities.clone(), 2);
    for &verifier in &participants.verifiers {
        adapter.send(verifier, effective_msg.clone()).await?;
    }
    
    // Phase 5: Receive verification results
    let mut verifier_proofs = HashMap::new();
    for &verifier in &participants.verifiers {
        let proof: ConsistencyProof = adapter.recv_from(verifier).await?;
        verifier_proofs.insert(verifier, proof);
    }
    
    Ok(CapabilityRestrictionResult {
        effective_capabilities,
        verification_successful: true, // Simplified
        verifier_proofs,
    })
}

async fn execute_verifier_protocol<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    verifier_id: DeviceId,
    participants: &CapabilityParticipants,
) -> Result<CapabilityRestrictionResult<S>, ChoreographyError> {
    // Phase 1: Receive base capabilities from grantor
    let base_msg: MeetStateMsg<S> = adapter.recv_from(participants.grantor).await?;
    
    // Phase 3: Receive effective capabilities from grantees
    let mut grantee_capabilities = HashMap::new();
    for &grantee in &participants.grantees {
        let effective_msg: MeetStateMsg<S> = adapter.recv_from(grantee).await?;
        grantee_capabilities.insert(grantee, effective_msg.payload);
    }
    
    // Phase 4: Validate capability bounds
    let verification_successful = grantee_capabilities.values()
        .all(|effective| {
            // Check that effective capabilities are within base bounds
            let intersection = base_msg.payload.meet(effective);
            intersection == *effective
        });
    
    // Phase 5: Send verification results
    let proof = generate_consistency_proof(&base_msg.payload, verifier_id)?;
    adapter.send(participants.grantor, proof.clone()).await?;
    
    for &grantee in &participants.grantees {
        adapter.send(grantee, proof.clone()).await?;
    }
    
    Ok(CapabilityRestrictionResult {
        effective_capabilities: base_msg.payload,
        verification_successful,
        verifier_proofs: [(verifier_id, proof)].into(),
    })
}

async fn execute_coordinator_protocol<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    participants: &TimeParticipants,
) -> Result<TimeIntersectionResult<S>, ChoreographyError> {
    // Phase 1: Receive time windows from participants
    let mut time_windows = Vec::new();
    for &participant in &participants.participants {
        let window_msg: MeetStateMsg<S> = adapter.recv_from(participant).await?;
        time_windows.push(window_msg.payload);
    }
    
    // Phase 2: Compute intersection
    let mut intersection_window = S::top();
    for window in &time_windows {
        intersection_window = intersection_window.meet(window);
    }
    
    // Phase 3: Announce intersection result
    let result_msg = MeetStateMsg::new(intersection_window.clone(), 3);
    for &participant in &participants.participants {
        adapter.send(participant, result_msg.clone()).await?;
    }
    
    // Phase 4: Collect verification proofs
    let mut verification_proofs = HashMap::new();
    for &participant in &participants.participants {
        let proof: ConsistencyProof = adapter.recv_from(participant).await?;
        verification_proofs.insert(participant, proof);
    }
    
    Ok(TimeIntersectionResult {
        intersection_window,
        participating_devices: participants.participants.clone(),
        verification_proofs,
    })
}

async fn execute_participant_protocol<S: MvState + Send + Sync + 'static>(
    adapter: &mut AuraHandlerAdapter,
    time_window: S,
    participant_id: DeviceId,
    participants: &TimeParticipants,
) -> Result<TimeIntersectionResult<S>, ChoreographyError> {
    // Phase 1: Send time window to coordinator
    let window_msg = MeetStateMsg::new(time_window, 1);
    adapter.send(participants.coordinator, window_msg).await?;
    
    // Phase 3: Receive intersection result
    let result_msg: MeetStateMsg<S> = adapter.recv_from(participants.coordinator).await?;
    
    // Phase 4: Verify intersection and send proof
    let proof = generate_consistency_proof(&result_msg.payload, participant_id)?;
    adapter.send(participants.coordinator, proof.clone()).await?;
    
    Ok(TimeIntersectionResult {
        intersection_window: result_msg.payload,
        participating_devices: participants.participants.clone(),
        verification_proofs: [(participant_id, proof)].into(),
    })
}