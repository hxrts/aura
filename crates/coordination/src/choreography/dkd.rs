//! DKD Protocol Choreography
//!
//! This module implements the P2P Deterministic Key Derivation
//! protocol using choreographic programming.

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, ProtocolContext,
    ProtocolContextExt, ProtocolError, ProtocolErrorType, SessionLifecycle,
};
use aura_crypto::{aggregate_dkd_points, DkdParticipant};
use aura_journal::{
    DeviceId, Event, EventType, FinalizeDkdSessionEvent, InitiateDkdSessionEvent,
    OperationType, ParticipantId as JournalParticipantId, ProtocolType,
    RecordDkdCommitmentEvent, RevealDkdPointEvent, Session,
};
use std::collections::BTreeSet;

/// DKD Protocol implementation using SessionLifecycle trait
pub struct DkdProtocol<'a> {
    ctx: &'a mut ProtocolContext,
    context_id: Vec<u8>,
}

impl<'a> DkdProtocol<'a> {
    pub fn new(ctx: &'a mut ProtocolContext, context_id: Vec<u8>) -> Self {
        Self { ctx, context_id }
    }
}

#[async_trait::async_trait]
impl<'a> SessionLifecycle for DkdProtocol<'a> {
    type Result = Vec<u8>; // The derived key

    fn operation_type(&self) -> OperationType {
        OperationType::Dkd
    }

    fn generate_context_id(&self) -> Vec<u8> {
        self.context_id.clone()
    }

    async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let ledger_context = self.ctx.fetch_ledger_context().await?;
        
        // Convert participants to session participants
        let session_participants: Vec<JournalParticipantId> = self.ctx.participants()
            .iter()
            .map(|&device_id| JournalParticipantId::Device(device_id))
            .collect();

        // Create DKD session
        Ok(Session::new(
            aura_journal::SessionId(self.ctx.session_id()),
            ProtocolType::Dkd,
            session_participants,
            ledger_context.epoch,
            50, // TTL in epochs - DKD is relatively quick
            self.ctx.effects().now().map_err(|e| ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to get timestamp: {:?}", e),
            })?,
        ))
    }

    async fn execute_protocol(&mut self, _session: &Session) -> Result<Vec<u8>, ProtocolError> {
        // Phase 0: Initiate Session
        let start_epoch = self.ctx.fetch_ledger_context().await?.epoch;
        let session_id = self.ctx.session_id();
        let context_id = self.context_id.clone();
        let threshold = self.ctx.threshold().unwrap() as u16;
        let participants = self.ctx.participants().clone();
        
        EventBuilder::new(self.ctx)
            .with_type(EventType::InitiateDkdSession(InitiateDkdSessionEvent {
                session_id,
                context_id,
                threshold,
                participants,
                start_epoch,
                ttl_in_epochs: 50,
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Phase 1: Commitment Phase
        let (our_commitment, mut dkd_participant) = self.generate_commitment();

        let session_id = self.ctx.session_id();
        let device_id = self.ctx.device_id();
        EventBuilder::new(self.ctx)
            .with_type(EventType::RecordDkdCommitment(RecordDkdCommitmentEvent {
                session_id,
                device_id: DeviceId(device_id),
                commitment: our_commitment,
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Wait for threshold commitments
        let session_id = self.ctx.session_id();
        let threshold = self.ctx.threshold().unwrap();
        let peer_commitments = EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(vec![EventTypePattern::DkdCommitment])
            .await_threshold(threshold, 10)
            .await?;

        // Phase 2: Reveal Phase
        let our_point = dkd_participant.revealed_point();

        let session_id = self.ctx.session_id();
        let device_id = self.ctx.device_id();
        EventBuilder::new(self.ctx)
            .with_type(EventType::RevealDkdPoint(RevealDkdPointEvent {
                session_id,
                device_id: DeviceId(device_id),
                point: our_point.to_vec(),
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Collect committed authors for reveal phase
        let committed_authors: BTreeSet<DeviceId> = peer_commitments
            .iter()
            .filter_map(|e| match &e.authorization {
                aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => {
                    Some(*device_id)
                }
                _ => None,
            })
            .collect();

        // Wait for reveals from all committed participants
        let session_id = self.ctx.session_id();
        let peer_reveals = EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(vec![EventTypePattern::DkdReveal])
            .from_authors(committed_authors.clone())
            .await_threshold(committed_authors.len(), 10)
            .await?;

        // Phase 3: Verification & Aggregation
        self.verify_reveals(&peer_reveals, &peer_commitments)?;
        let derived_key = self.aggregate_points(&peer_reveals, &our_point)?;

        // Phase 4: Finalize
        // Compute Merkle root of all commitments for protocol verification
        let commitment_hashes: Vec<[u8; 32]> = peer_commitments
            .iter()
            .filter_map(|event| {
                if let EventType::RecordDkdCommitment(commitment_event) = &event.event_type {
                    Some(commitment_event.commitment)
                } else {
                    None
                }
            })
            .collect();
        
        let commitment_root = if commitment_hashes.is_empty() {
            [0u8; 32] // Empty root for no commitments
        } else {
            let (root, _proofs) = aura_crypto::merkle::build_commitment_tree(&commitment_hashes)?;
            root
        };
        
        // Compute seed fingerprint from derived key
        let seed_fingerprint = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"aura-dkd-seed-v1:");
            hasher.update(&derived_key.to_bytes());
            *hasher.finalize().as_bytes()
        };

        let session_id = self.ctx.session_id();
        EventBuilder::new(self.ctx)
            .with_type(EventType::FinalizeDkdSession(FinalizeDkdSessionEvent {
                session_id,
                seed_fingerprint,
                commitment_root,
                derived_identity_pk: derived_key.to_bytes().to_vec(),
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        Ok(derived_key.to_bytes().to_vec())
    }

    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<Vec<u8>, ProtocolError> {
        let finalize_event = EventAwaiter::new(self.ctx)
            .for_session(winning_session.session_id.0)
            .for_event_types(vec![EventTypePattern::DkdFinalize])
            .await_single(100) // Default TTL epochs
            .await?;

        match &finalize_event.event_type {
            EventType::FinalizeDkdSession(finalize) => Ok(finalize.derived_identity_pk.clone()),
            _ => Err(ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected DKD finalize event".to_string(),
            }),
        }
    }
}

impl<'a> DkdProtocol<'a> {
    /// Generate commitment for DKD protocol
    fn generate_commitment(&self) -> ([u8; 32], DkdParticipant) {
        // Mix session ID with device ID for unique but deterministic shares
        let mut share_bytes = [0u8; 16];
        let session_id = self.ctx.session_id();
        let session_bytes = session_id.as_bytes();
        let device_id = self.ctx.device_id();
        let device_bytes = device_id.as_bytes();

        // XOR session ID with device ID
        for i in 0..16 {
            share_bytes[i] = session_bytes[i] ^ device_bytes[i];
        }

        let mut participant = DkdParticipant::new(share_bytes);
        let commitment = participant.commitment_hash();
        (commitment, participant)
    }

    /// Verify reveals match commitments
    fn verify_reveals(
        &self,
        peer_reveals: &[Event],
        peer_commitments: &[Event],
    ) -> Result<(), ProtocolError> {
        for reveal_event in peer_reveals {
            let reveal_author = match &reveal_event.authorization {
                aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => device_id,
                _ => continue,
            };

            // Find corresponding commitment
            let commitment = peer_commitments.iter().find(|e| match &e.authorization {
                aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => {
                    device_id == reveal_author
                }
                _ => false,
            });

            if commitment.is_none() {
                return Err(ProtocolError {
                    session_id: self.ctx.session_id(),
                    error_type: ProtocolErrorType::ByzantineBehavior,
                    message: format!("Reveal from {:?} without commitment", reveal_author.0),
                });
            }

            // Verify reveal hash matches commitment hash
            let commitment = commitment.unwrap();
            
            // Extract commitment and reveal data
            let commitment_hash = match &commitment.event_type {
                aura_journal::EventType::RecordDkdCommitment(event) => event.commitment,
                _ => {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::ByzantineBehavior,
                        message: format!("Invalid commitment event type from {:?}", reveal_author.0),
                    });
                }
            };
            
            let reveal_point = match &reveal_event.event_type {
                aura_journal::EventType::RevealDkdPoint(event) => &event.point,
                _ => {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::ByzantineBehavior,
                        message: format!("Invalid reveal event type from {:?}", reveal_author.0),
                    });
                }
            };
            
            // Verify that blake3(point) equals the commitment hash
            let calculated_hash = *blake3::hash(reveal_point).as_bytes();
            if calculated_hash != commitment_hash {
                return Err(ProtocolError {
                    session_id: self.ctx.session_id(),
                    error_type: ProtocolErrorType::ByzantineBehavior,
                    message: format!(
                        "Reveal from {:?} does not match commitment: expected {:?}, got {:?}",
                        reveal_author.0,
                        commitment_hash,
                        calculated_hash
                    ),
                });
            }
        }

        Ok(())
    }

    /// Aggregate revealed points to derive key
    fn aggregate_points(
        &self,
        peer_reveals: &[Event],
        our_point: &[u8; 32],
    ) -> Result<ed25519_dalek::VerifyingKey, ProtocolError> {
        // Extract points from peer reveals (excluding our own)
        let mut revealed_points: Vec<[u8; 32]> = peer_reveals
            .iter()
            .filter_map(|e| {
                // Skip our own reveal event
                if let aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } =
                    &e.authorization
                {
                    if device_id.0 == self.ctx.device_id() {
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

        // Add our own point
        revealed_points.push(*our_point);

        // Sort points deterministically
        revealed_points.sort();

        aggregate_dkd_points(&revealed_points).map_err(|e| ProtocolError {
            session_id: self.ctx.session_id(),
            error_type: ProtocolErrorType::Other,
            message: format!("Failed to aggregate points: {:?}", e),
        })
    }
}

/// DKD Protocol Choreography - Main entry point
pub async fn dkd_choreography(ctx: &mut ProtocolContext, context_id: Vec<u8>) -> Result<Vec<u8>, ProtocolError> {
    let mut protocol = DkdProtocol::new(ctx, context_id);
    protocol.execute().await
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::{AccountLedger, AccountState};
    use crate::execution::context::StubTransport;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_dkd_choreography_structure() {
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
            next_nonce: 1,
            used_nonces: std::collections::BTreeSet::new(),
        };

        let state = AccountState::new(
            aura_journal::AccountId(Uuid::from_bytes([6u8; 16])),
            ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2,
            3,
        );

        let ledger = Arc::new(RwLock::new(AccountLedger::new(state).unwrap()));

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
        assert_eq!(ctx.session_id(), session_id);
        assert_eq!(ctx.threshold(), Some(2));
    }
}