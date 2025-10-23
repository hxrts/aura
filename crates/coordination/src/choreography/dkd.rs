//! DKD Protocol Choreography - Choreographic Programming Implementation
//!
//! This module implements the P2P Deterministic Key Derivation protocol using
//! choreographic programming, a style where distributed protocols are
//! written as single programs from a global viewpoint.
//!
//! The choreography implicitly encodes session types:
//! ```text
//! Initiate . Commit* . Reveal* . Aggregate . Finalize
//! ```
//! Where * indicates threshold collection (M-of-N parties).
//!
//! Reference:
//! - 080_architecture_protocol_integration.md - Part 4: P2P DKD Protocol
//! - work/04_declarative_protocol_evolution.md - Phase 2
//! - Choreographic Programming: https://arxiv.org/abs/1303.0039

use crate::execution::{
    ProtocolContext, ProtocolError, ProtocolErrorType,
    Instruction, EventFilter, EventTypePattern, InstructionResult,
};
use aura_crypto::{DkdParticipant, aggregate_dkd_points};
use aura_journal::{
    Event, EventType, InitiateDkdSessionEvent, RecordDkdCommitmentEvent,
    RevealDkdPointEvent, FinalizeDkdSessionEvent, EventAuthorization, DeviceId,
    Session, ProtocolType, ParticipantId as JournalParticipantId,
};
use std::collections::BTreeSet;

/// DKD Protocol Choreography
///
/// This choreography defines the entire P2P DKD protocol from a **global viewpoint**.
/// Each device executes this same choreography, but the ProtocolContext automatically
/// handles local projection (determining which actions apply to this device).
///
/// ## Choreographic Session Type
///
/// ```text
/// Initiate(SessionId, Participants, Threshold) .
/// Commit{p ∈ Participants}(Commitment_p) .
/// Reveal{p ∈ Participants}(Point_p) .
/// Aggregate(DerivedKey) .
/// Finalize(DerivedKey)
/// ```
///
/// ## Protocol Flow (Choreographic View)
///
/// 1. **Initiate**: All parties observe session start
/// 2. **Commit**: Each party broadcasts commitment_i, waits for threshold
/// 3. **Reveal**: Each party broadcasts point_i, waits for threshold
/// 4. **Aggregate**: All parties compute derived_key = ∑ point_i
/// 5. **Finalize**: Coordinator writes finalization
///
/// ## Example Usage
///
/// ```rust,ignore
/// let mut ctx = ProtocolContext::new(...);
/// let derived_key = dkd_choreography(&mut ctx).await?;
/// ```
pub async fn dkd_choreography(
    ctx: &mut ProtocolContext,
) -> Result<Vec<u8>, ProtocolError> {
    // Step 1: Create DKD Session  
    let session = create_dkd_session(ctx).await?;
    
    // Step 2: Execute Protocol with Session Tracking
    match execute_dkd_protocol(ctx, &session).await {
        Ok(result) => {
            // Step 3: Mark Session as Completed
            complete_dkd_session(ctx, &session).await?;
            Ok(result)
        }
        Err(error) => {
            // Step 4: Mark Session as Aborted
            abort_dkd_session(ctx, &session, &error).await?;
            Err(error)
        }
    }
}

/// Execute the original DKD choreography implementation
async fn execute_dkd_protocol(
    ctx: &mut ProtocolContext,
    _session: &Session,
) -> Result<Vec<u8>, ProtocolError> {
    // Get ledger state for proper event construction
    let (account_id, nonce, parent_hash, epoch, start_epoch) = {
        let ledger_result = ctx.execute(Instruction::GetLedgerState).await?;
        let ledger_state = match ledger_result {
            InstructionResult::LedgerState(state) => state,
            _ => return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected ledger state".to_string(),
            }),
        };
        
        let epoch_result = ctx.execute(Instruction::GetCurrentEpoch).await?;
        let epoch = match epoch_result {
            InstructionResult::CurrentEpoch(e) => e,
            _ => return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected current epoch".to_string(),
            }),
        };
        
        (
            ledger_state.account_id,
            ledger_state.next_nonce,
            ledger_state.last_event_hash,
            epoch,
            epoch,
        )
    };
    
    // ========== Phase 0: Initiate Session ==========
    
    let mut init_event = Event {
        version: 1,
        event_id: aura_journal::EventId::new(),
        account_id,
        timestamp: ctx.effects.now().map_err(|e| ProtocolError {
            session_id: ctx.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to get timestamp: {:?}", e),
        })?,
        nonce,
        parent_hash,
        epoch_at_write: epoch,
        event_type: EventType::InitiateDkdSession(InitiateDkdSessionEvent {
            session_id: ctx.session_id,
            context_id: vec![], // Context for key derivation
            threshold: ctx.threshold.unwrap() as u16,
            participants: ctx.participants.clone(),
            start_epoch,
            ttl_in_epochs: 100, // 100 epoch timeout
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id: DeviceId(ctx.device_id),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Temporary, will be replaced
        },
    };
    
    // Sign the event properly
    let signature = ctx.sign_event(&init_event)?;
    init_event.authorization = EventAuthorization::DeviceCertificate {
        device_id: DeviceId(ctx.device_id),
        signature,
    };
    
    // Get the hash of this event before writing it
    let init_event_hash = init_event.hash().map_err(|e| ProtocolError {
        session_id: ctx.session_id,
        error_type: ProtocolErrorType::Other,
        message: format!("Failed to hash init event: {:?}", e),
    })?;
    
    ctx.execute(Instruction::WriteToLedger(init_event)).await
        .map_err(|e| ProtocolError {
            session_id: ctx.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to initiate session: {:?}", e),
        })?;
    
    // Update parent hash for next event
    let _parent_hash = Some(init_event_hash);
    
    // ========== Phase 1: Commitment Phase ==========
    
    // Compute our local commitment
    // Each participant generates a unique point, but they all derive the same key
    // Mix the session ID with the device ID to get unique but deterministic shares
    let mut share_bytes = [0u8; 16];
    let session_bytes = ctx.session_id.as_bytes();
    let device_bytes = ctx.device_id.as_bytes();
    
    // XOR session ID with device ID to create unique but deterministic shares
    for i in 0..16 {
        share_bytes[i] = session_bytes[i] ^ device_bytes[i];
    }
    
    let mut dkd_participant = DkdParticipant::new(share_bytes);
    let our_commitment = dkd_participant.commitment_hash();
    
    // Get the latest parent hash and nonce before writing
    let (parent_hash, nonce) = {
        let ledger_result = ctx.execute(Instruction::GetLedgerState).await?;
        let ledger_state = match ledger_result {
            InstructionResult::LedgerState(state) => state,
            _ => return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected ledger state".to_string(),
            }),
        };
        (ledger_state.last_event_hash, ledger_state.next_nonce)
    };
    
    // Broadcast our commitment
    let mut commitment_event = Event {
        version: 1,
        event_id: aura_journal::EventId::new(),
        account_id,
        timestamp: ctx.effects.now().unwrap(),
        nonce,
        parent_hash,
        epoch_at_write: epoch,
        event_type: EventType::RecordDkdCommitment(RecordDkdCommitmentEvent {
            session_id: ctx.session_id,
            device_id: DeviceId(ctx.device_id),
            commitment: our_commitment, // Correct field name
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id: DeviceId(ctx.device_id),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Temporary
        },
    };
    
    // Sign the event properly
    let signature = ctx.sign_event(&commitment_event)?;
    commitment_event.authorization = EventAuthorization::DeviceCertificate {
        device_id: DeviceId(ctx.device_id),
        signature,
    };
    
    // Get the hash of this event before writing it
    let commitment_event_hash = commitment_event.hash().map_err(|e| ProtocolError {
        session_id: ctx.session_id,
        error_type: ProtocolErrorType::Other,
        message: format!("Failed to hash commitment event: {:?}", e),
    })?;
    
    ctx.execute(Instruction::WriteToLedger(commitment_event)).await?;
    
    // Update parent hash for next event
    let _parent_hash = Some(commitment_event_hash);
    
    // Wait for threshold commitments from peers
    let commitment_filter = EventFilter {
        session_id: Some(ctx.session_id),
        event_types: Some(vec![EventTypePattern::DkdCommitment]),
        authors: None,
        predicate: None,
    };
    
    let peer_commitments_result = ctx.execute(Instruction::AwaitThreshold {
        count: ctx.threshold.unwrap(),
        filter: commitment_filter,
        timeout_epochs: Some(10), // Much shorter timeout for simulation
    }).await?;
    
    let peer_commitments = match peer_commitments_result {
        InstructionResult::EventsReceived(events) => events,
        _ => return Err(ProtocolError {
            session_id: ctx.session_id,
            error_type: ProtocolErrorType::InvalidState,
            message: "Expected events from AwaitThreshold".to_string(),
        }),
    };
    
    // ========== Phase 2: Reveal Phase ==========
    
    // Get the latest parent hash and nonce before writing (in case other events were written)
    let (parent_hash, nonce) = {
        let ledger_result = ctx.execute(Instruction::GetLedgerState).await?;
        let ledger_state = match ledger_result {
            InstructionResult::LedgerState(state) => state,
            _ => return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected ledger state".to_string(),
            }),
        };
        (ledger_state.last_event_hash, ledger_state.next_nonce)
    };
    
    // Reveal our point
    let our_point = dkd_participant.revealed_point();
    let mut reveal_event = Event {
        version: 1,
        event_id: aura_journal::EventId::new(),
        account_id,
        timestamp: ctx.effects.now().unwrap(),
        nonce,
        parent_hash,
        epoch_at_write: epoch,
        event_type: EventType::RevealDkdPoint(RevealDkdPointEvent {
            session_id: ctx.session_id,
            device_id: DeviceId(ctx.device_id),
            point: our_point.to_vec(), // Correct field name
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id: DeviceId(ctx.device_id),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Temporary
        },
    };
    
    // Sign the event properly
    let signature = ctx.sign_event(&reveal_event)?;
    reveal_event.authorization = EventAuthorization::DeviceCertificate {
        device_id: DeviceId(ctx.device_id),
        signature,
    };
    
    ctx.execute(Instruction::WriteToLedger(reveal_event)).await?;
    
    // Wait for reveals from ALL committed participants (not just threshold)
    // This ensures all participants see the same set of reveals
    let committed_authors: BTreeSet<DeviceId> = peer_commitments
        .iter()
        .filter_map(|e| match &e.authorization {
            EventAuthorization::DeviceCertificate { device_id, .. } => Some(*device_id),
            _ => None,
        })
        .collect();
    
    let reveal_filter = EventFilter {
        session_id: Some(ctx.session_id),
        event_types: Some(vec![EventTypePattern::DkdReveal]),
        authors: Some(committed_authors.clone()),
        predicate: None,
    };
    
    // Wait for ALL committed participants to reveal, not just threshold
    let peer_reveals_result = ctx.execute(Instruction::AwaitThreshold {
        count: committed_authors.len(), // Wait for ALL, not just threshold
        filter: reveal_filter,
        timeout_epochs: Some(10), // Much shorter timeout for simulation
    }).await?;
    
    let peer_reveals = match peer_reveals_result {
        InstructionResult::EventsReceived(events) => events,
        _ => return Err(ProtocolError {
            session_id: ctx.session_id,
            error_type: ProtocolErrorType::InvalidState,
            message: "Expected events from AwaitThreshold".to_string(),
        }),
    };
    
    // ========== Phase 3: Verification & Aggregation ==========
    
    // Verify reveals match commitments
    for reveal_event in &peer_reveals {
        let reveal_author = match &reveal_event.authorization {
            EventAuthorization::DeviceCertificate { device_id, .. } => device_id,
            _ => continue,
        };
        
        // Find corresponding commitment
        let commitment = peer_commitments
            .iter()
            .find(|e| match &e.authorization {
                EventAuthorization::DeviceCertificate { device_id, .. } => {
                    device_id == reveal_author
                }
                _ => false,
            });
        
        if commitment.is_none() {
            return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::ByzantineBehavior,
                message: format!("Reveal from {:?} without commitment", reveal_author.0),
            });
        }
        
        // TODO: Verify reveal hash matches commitment hash
    }
    
    // Aggregate revealed points to derive key
    // Extract points from peer reveals (excluding our own to avoid double counting)
    let mut revealed_points: Vec<[u8; 32]> = peer_reveals
        .iter()
        .filter_map(|e| {
            // Skip our own reveal event to avoid double counting
            if let EventAuthorization::DeviceCertificate { device_id, .. } = &e.authorization {
                if *device_id == DeviceId(ctx.device_id) {
                    return None;
                }
            }
            
            match &e.event_type {
                EventType::RevealDkdPoint(reveal) => {
                    let mut arr = [0u8; 32];
                    let len = reveal.point.len().min(32);
                    arr[..len].copy_from_slice(&reveal.point[..len]);
                    Some(arr)
                }
                _ => None,
            }
        })
        .collect();
    
    // Add our own point to the aggregation
    revealed_points.push(*our_point);
    
    // Sort points deterministically to ensure all participants aggregate in same order
    revealed_points.sort();
    
    
    let derived_key = aggregate_dkd_points(&revealed_points)
        .map_err(|e| ProtocolError {
            session_id: ctx.session_id,
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to aggregate points: {:?}", e),
        })?;
    
    // ========== Phase 4: Finalize ==========
    
    // Compute commitment root (Merkle root of all commitments)
    let commitment_root = [0u8; 32]; // TODO: Actually compute Merkle root
    
    // Get the latest parent hash and nonce before writing
    let (parent_hash, nonce) = {
        let ledger_result = ctx.execute(Instruction::GetLedgerState).await?;
        let ledger_state = match ledger_result {
            InstructionResult::LedgerState(state) => state,
            _ => return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected ledger state".to_string(),
            }),
        };
        (ledger_state.last_event_hash, ledger_state.next_nonce)
    };
    
    let mut finalize_event = Event {
        version: 1,
        event_id: aura_journal::EventId::new(),
        account_id,
        timestamp: ctx.effects.now().unwrap(),
        nonce,
        parent_hash,
        epoch_at_write: epoch,
        event_type: EventType::FinalizeDkdSession(FinalizeDkdSessionEvent {
            session_id: ctx.session_id,
            seed_fingerprint: [0u8; 32], // TODO: Compute seed fingerprint
            commitment_root,
            derived_identity_pk: derived_key.as_bytes().to_vec(),
        }),
        authorization: EventAuthorization::DeviceCertificate {
            device_id: DeviceId(ctx.device_id),
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]), // Temporary
        },
    };
    
    // Sign the event properly
    let signature = ctx.sign_event(&finalize_event)?;
    finalize_event.authorization = EventAuthorization::DeviceCertificate {
        device_id: DeviceId(ctx.device_id),
        signature,
    };
    
    ctx.execute(Instruction::WriteToLedger(finalize_event)).await?;
    
    // Return the derived key
    Ok(derived_key.as_bytes().to_vec())
}

/// Create a new DKD session in the CRDT
async fn create_dkd_session(
    ctx: &mut ProtocolContext,
) -> Result<Session, ProtocolError> {
    // Get current state for session configuration
    let (current_epoch, participants) = {
        let epoch_result = ctx.execute(Instruction::GetCurrentEpoch).await?;
        let epoch = match epoch_result {
            InstructionResult::CurrentEpoch(e) => e,
            _ => return Err(ProtocolError {
                session_id: ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Failed to get current epoch".to_string(),
            }),
        };
        
        (epoch, ctx.participants.clone())
    };
    
    // Convert participants to session participants
    let session_participants: Vec<JournalParticipantId> = participants.into_iter()
        .map(JournalParticipantId::Device)
        .collect();
    
    // Create DKD session
    let session = Session::new(
        ctx.session_id,
        ProtocolType::Dkd,
        session_participants,
        current_epoch,
        50, // TTL in epochs - DKD is relatively quick
        current_timestamp(),
    );
    
    // Add session to CRDT (would be done via event application)
    // For now, we just return the session for protocol tracking
    Ok(session)
}

/// Mark the DKD session as completed
async fn complete_dkd_session(
    ctx: &mut ProtocolContext,
    session: &Session,
) -> Result<(), ProtocolError> {
    // In production, this would emit a CompleteSessionEvent
    // For MVP, we just log the completion
    let _ = (ctx, session);
    Ok(())
}

/// Mark the DKD session as aborted
async fn abort_dkd_session(
    ctx: &mut ProtocolContext,
    session: &Session,
    error: &ProtocolError,
) -> Result<(), ProtocolError> {
    // In production, this would emit an AbortSessionEvent
    // For MVP, we just log the failure
    let _ = (ctx, session, error);
    Ok(())
}

/// Get current timestamp for session creation
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::{AccountLedger, AccountState};
    use aura_transport::StubTransport;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use uuid::Uuid;
    
    #[tokio::test]
    async fn test_dkd_choreography_structure() {
        // This test verifies the choreography compiles and has the right structure
        // Full integration tests would require a mock CRDT
        
        // Use deterministic UUIDs for testing
        let session_id = Uuid::from_bytes([1u8; 16]);
        let device_id = Uuid::from_bytes([2u8; 16]);
        
        let participants = vec![
            DeviceId(Uuid::from_bytes([3u8; 16])),
            DeviceId(Uuid::from_bytes([4u8; 16])),
            DeviceId(Uuid::from_bytes([5u8; 16])),
        ];
        
        // Create minimal context (won't actually execute)
        let device_metadata = aura_journal::DeviceMetadata {
            device_id: DeviceId(device_id),
            device_name: "test-device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };
        
        let state = AccountState::new(
            aura_journal::AccountId(Uuid::from_bytes([6u8; 16])),
            ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2,
            3,
        );
        
        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(state).unwrap(),
        ));
        
        let device_key = ed25519_dalek::SigningKey::from_bytes(&[0u8; 32]);
        
        let ctx = ProtocolContext::new(
            session_id,
            device_id,
            participants,
            Some(2),
            ledger,
            Arc::new(StubTransport::default()),
            Effects::test(),
            device_key,
            Box::new(crate::ProductionTimeSource::new()),
        );
        
        // Verify context is set up correctly
        assert_eq!(ctx.session_id, session_id);
        assert_eq!(ctx.threshold, Some(2));
    }
}
