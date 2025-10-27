//! DKD Protocol: Complete Implementation
//!
//! This module contains the complete implementation of the P2P Deterministic Key Derivation
//! protocol, including both the session type definitions for compile-time safety and the
//! choreographic execution logic. This merger improves cohesion and maintainability by
//! keeping all protocol-related code in a single file.

// ========== Session Type Definitions ==========

use crate::session_types::wrapper::{SessionProtocol, SessionTypedProtocol};
use aura_journal::Event;
use aura_types::DeviceId;
use session_types::witnesses::RuntimeWitness;
use session_types::SessionState;
use uuid::Uuid;

/// Configuration for commitment phase
#[derive(Debug, Clone, Default)]
pub struct CommitmentConfig {
    pub threshold: usize,
}

/// Collected commitments from participants
#[derive(Debug, Clone, Default)]
pub struct CollectedCommitments {
    pub commitments: Vec<Vec<u8>>,
}

/// Verified reveals from participants
#[derive(Debug, Clone, Default)]
pub struct VerifiedReveals {
    pub reveals: Vec<Vec<u8>>,
}

/// Core data structure for DKD protocol
#[derive(Debug, Clone)]
pub struct DkdProtocolCore {
    /// Device ID for this protocol instance
    device_id: DeviceId,
    /// Protocol session ID
    session_id: Uuid,
    /// App ID for DKD
    app_id: String,
    /// Context label for DKD
    context: String,
}

impl DkdProtocolCore {
    #[allow(clippy::disallowed_methods)]
    pub fn new(device_id: DeviceId, app_id: String, context: String) -> Self {
        Self {
            device_id,
            session_id: Uuid::new_v4(),
            app_id,
            context,
        }
    }
}

/// Error type for DKD session protocols
#[derive(Debug, thiserror::Error)]
pub enum DkdSessionError {
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid state transition")]
    InvalidTransition,
    #[error("Insufficient participants")]
    InsufficientParticipants,
    #[error("Timeout")]
    Timeout,
    #[error("Commitment validation failed")]
    CommitmentValidationFailed,
    #[error("Reveal validation failed")]
    RevealValidationFailed,
}

// ========== State Definitions ==========

/// Initial state where participants are being gathered
#[derive(Debug, Clone)]
pub struct InitializationPhase;

impl SessionState for InitializationPhase {
    const NAME: &'static str = "InitializationPhase";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where participants are collecting commitments
#[derive(Debug, Clone)]
pub struct CommitmentPhase;

impl SessionState for CommitmentPhase {
    const NAME: &'static str = "CommitmentPhase";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where participants are revealing their values
#[derive(Debug, Clone)]
pub struct RevealPhase;

impl SessionState for RevealPhase {
    const NAME: &'static str = "RevealPhase";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where the final key is being derived
#[derive(Debug, Clone)]
pub struct FinalizationPhase;

impl SessionState for FinalizationPhase {
    const NAME: &'static str = "FinalizationPhase";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Final successful state with derived key
#[derive(Debug, Clone)]
pub struct CompletionPhase;

impl SessionState for CompletionPhase {
    const NAME: &'static str = "CompletionPhase";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Final failure state
#[derive(Debug, Clone)]
pub struct Failure;

impl SessionState for Failure {
    const NAME: &'static str = "Failure";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

// ========== Witnesses ==========

/// Witness for DKD completion
#[derive(Debug, Clone)]
pub struct DkdCompleted {
    pub derived_key: Vec<u8>,
    pub context: String,
    pub app_id: String,
}

impl RuntimeWitness for DkdCompleted {
    type Evidence = (Vec<u8>, String, String); // (derived_key, context, app_id)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (derived_key, context, app_id) = evidence;
        if !derived_key.is_empty() && !context.is_empty() && !app_id.is_empty() {
            Some(DkdCompleted {
                derived_key,
                context,
                app_id,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "DKD protocol completed successfully"
    }
}

impl DkdCompleted {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), DkdSessionError> {
        if self.derived_key.is_empty() {
            return Err(DkdSessionError::ProtocolError(
                "Empty derived key".to_string(),
            ));
        }
        if self.context.is_empty() {
            return Err(DkdSessionError::ProtocolError("Empty context".to_string()));
        }
        if self.app_id.is_empty() {
            return Err(DkdSessionError::ProtocolError("Empty app_id".to_string()));
        }
        Ok(())
    }
}

// ========== Protocol State Machine ==========

/// Union type representing all possible DKD session states
#[derive(Debug, Clone)]
pub enum DkdProtocolState {
    InitializationPhase(SessionTypedProtocol<DkdProtocolCore, InitializationPhase>),
    CommitmentPhase(SessionTypedProtocol<DkdProtocolCore, CommitmentPhase>),
    RevealPhase(SessionTypedProtocol<DkdProtocolCore, RevealPhase>),
    FinalizationPhase(SessionTypedProtocol<DkdProtocolCore, FinalizationPhase>),
    CompletionPhase(SessionTypedProtocol<DkdProtocolCore, CompletionPhase>),
    Failure(SessionTypedProtocol<DkdProtocolCore, Failure>),
}

// Marker type for union state
#[derive(Debug, Clone)]
pub struct DkdUnionState;

impl SessionState for DkdUnionState {
    const NAME: &'static str = "DkdUnion";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

impl SessionProtocol for DkdProtocolState {
    fn session_id(&self) -> Uuid {
        match self {
            DkdProtocolState::InitializationPhase(p) => p.core().session_id,
            DkdProtocolState::CommitmentPhase(p) => p.core().session_id,
            DkdProtocolState::RevealPhase(p) => p.core().session_id,
            DkdProtocolState::FinalizationPhase(p) => p.core().session_id,
            DkdProtocolState::CompletionPhase(p) => p.core().session_id,
            DkdProtocolState::Failure(p) => p.core().session_id,
        }
    }

    fn device_id(&self) -> Uuid {
        match self {
            DkdProtocolState::InitializationPhase(p) => p.core().device_id.0,
            DkdProtocolState::CommitmentPhase(p) => p.core().device_id.0,
            DkdProtocolState::RevealPhase(p) => p.core().device_id.0,
            DkdProtocolState::FinalizationPhase(p) => p.core().device_id.0,
            DkdProtocolState::CompletionPhase(p) => p.core().device_id.0,
            DkdProtocolState::Failure(p) => p.core().device_id.0,
        }
    }

    fn state_name(&self) -> &'static str {
        match self {
            DkdProtocolState::InitializationPhase(_) => InitializationPhase::NAME,
            DkdProtocolState::CommitmentPhase(_) => CommitmentPhase::NAME,
            DkdProtocolState::RevealPhase(_) => RevealPhase::NAME,
            DkdProtocolState::FinalizationPhase(_) => FinalizationPhase::NAME,
            DkdProtocolState::CompletionPhase(_) => CompletionPhase::NAME,
            DkdProtocolState::Failure(_) => Failure::NAME,
        }
    }

    fn can_terminate(&self) -> bool {
        match self {
            DkdProtocolState::InitializationPhase(_) => InitializationPhase::CAN_TERMINATE,
            DkdProtocolState::CommitmentPhase(_) => CommitmentPhase::CAN_TERMINATE,
            DkdProtocolState::RevealPhase(_) => RevealPhase::CAN_TERMINATE,
            DkdProtocolState::FinalizationPhase(_) => FinalizationPhase::CAN_TERMINATE,
            DkdProtocolState::CompletionPhase(_) => CompletionPhase::CAN_TERMINATE,
            DkdProtocolState::Failure(_) => Failure::CAN_TERMINATE,
        }
    }

    fn protocol_id(&self) -> Uuid {
        // For union types, protocol_id is the same as session_id
        self.session_id()
    }

    fn is_final(&self) -> bool {
        matches!(
            self,
            DkdProtocolState::CompletionPhase(_) | DkdProtocolState::Failure(_)
        )
    }
}

// ========== State Transition Methods ==========

impl DkdProtocolState {
    /// Check if protocol is in a final state
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            DkdProtocolState::CompletionPhase(_) | DkdProtocolState::Failure(_)
        )
    }

    /// Transition from InitializationPhase to CommitmentPhase
    pub fn begin_commitment_phase(
        self,
        _config: CommitmentConfig,
    ) -> Result<DkdProtocolState, DkdSessionError> {
        match self {
            DkdProtocolState::InitializationPhase(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(DkdProtocolState::CommitmentPhase(new_protocol))
            }
            _ => Err(DkdSessionError::InvalidTransition),
        }
    }

    /// Transition from CommitmentPhase to RevealPhase
    pub fn begin_reveal_phase(
        self,
        _commitments: CollectedCommitments,
    ) -> Result<DkdProtocolState, DkdSessionError> {
        match self {
            DkdProtocolState::CommitmentPhase(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(DkdProtocolState::RevealPhase(new_protocol))
            }
            _ => Err(DkdSessionError::InvalidTransition),
        }
    }

    /// Transition from RevealPhase to FinalizationPhase
    pub fn begin_finalization_phase(
        self,
        _reveals: VerifiedReveals,
    ) -> Result<DkdProtocolState, DkdSessionError> {
        match self {
            DkdProtocolState::RevealPhase(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(DkdProtocolState::FinalizationPhase(new_protocol))
            }
            _ => Err(DkdSessionError::InvalidTransition),
        }
    }

    /// Transition from FinalizationPhase to CompletionPhase
    pub fn complete(self, completion: DkdCompleted) -> Result<DkdProtocolState, DkdSessionError> {
        completion.check()?;
        match self {
            DkdProtocolState::FinalizationPhase(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(DkdProtocolState::CompletionPhase(new_protocol))
            }
            _ => Err(DkdSessionError::InvalidTransition),
        }
    }

    /// Transition to failure state from any non-final state
    pub fn fail(self, _reason: String) -> Result<DkdProtocolState, DkdSessionError> {
        if self.is_final() {
            return Err(DkdSessionError::InvalidTransition);
        }

        let core = match self {
            DkdProtocolState::InitializationPhase(p) => p.into_core(),
            DkdProtocolState::CommitmentPhase(p) => p.into_core(),
            DkdProtocolState::RevealPhase(p) => p.into_core(),
            DkdProtocolState::FinalizationPhase(p) => p.into_core(),
            _ => return Err(DkdSessionError::InvalidTransition),
        };

        let new_protocol = SessionTypedProtocol::new(core);
        Ok(DkdProtocolState::Failure(new_protocol))
    }
}

// ========== Constructor Functions ==========

/// Create a new DKD protocol instance in the initial state
pub fn new_dkd_protocol(
    device_id: DeviceId,
    app_id: String,
    context: String,
) -> Result<DkdProtocolState, DkdSessionError> {
    let core = DkdProtocolCore::new(device_id, app_id, context);
    let protocol = SessionTypedProtocol::new(core);
    Ok(DkdProtocolState::InitializationPhase(protocol))
}

/// Rehydrate a DKD protocol from crash recovery evidence
pub fn rehydrate_dkd_protocol(
    device_id: DeviceId,
    app_id: String,
    context: String,
    _evidence: Vec<Event>,
) -> Result<DkdProtocolState, DkdSessionError> {
    // For now, just create a new protocol
    // In a full implementation, this would analyze the evidence
    // to determine the correct state to resume from
    new_dkd_protocol(device_id, app_id, context)
}

// ========== Choreographic Execution Logic ==========

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, ProtocolContext, ProtocolContextExt,
    ProtocolError, ProtocolErrorType, SessionLifecycle,
};
use crate::protocol_results::DkdProtocolResult;
use aura_crypto::{aggregate_dkd_points, DkdParticipant};
use aura_journal::{
    EventType, FinalizeDkdSessionEvent, InitiateDkdSessionEvent, OperationType,
    ParticipantId as JournalParticipantId, ProtocolType, RecordDkdCommitmentEvent,
    RevealDkdPointEvent, Session,
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
    type Result = DkdProtocolResult; // Complete protocol result with ledger mutations

    fn operation_type(&self) -> OperationType {
        OperationType::Dkd
    }

    fn generate_context_id(&self) -> Vec<u8> {
        self.context_id.clone()
    }

    async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let ledger_context = self.ctx.fetch_ledger_context().await?;

        // Convert participants to session participants
        let session_participants: Vec<JournalParticipantId> = self
            .ctx
            .participants()
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

    async fn execute_protocol(
        &mut self,
        _session: &Session,
    ) -> Result<DkdProtocolResult, ProtocolError> {
        // Phase 0: Initiate Session
        let start_epoch = self.ctx.fetch_ledger_context().await?.epoch;
        let session_id = self.ctx.session_id();
        let context_id = self.context_id.clone();
        let threshold = self.ctx.threshold().unwrap() as u16;
        let participants = self.ctx.participants().clone();

        let _initiate_event = EventBuilder::new(self.ctx)
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
        let _commitment_event = EventBuilder::new(self.ctx)
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
        let _reveal_event = EventBuilder::new(self.ctx)
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
        let _finalize_event = EventBuilder::new(self.ctx)
            .with_type(EventType::FinalizeDkdSession(FinalizeDkdSessionEvent {
                session_id,
                seed_fingerprint,
                commitment_root,
                derived_identity_pk: derived_key.to_bytes().to_vec(),
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Collect all events from this protocol execution
        let ledger_events = self.ctx.collected_events().to_vec();

        // Create threshold signature
        // In production, this would be collected from M-of-N participants
        let threshold_signature = crate::ThresholdSignature {
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            signers: vec![], // Would be populated with actual ParticipantIds from signing participants
        };

        // Return complete protocol result
        Ok(DkdProtocolResult {
            session_id: aura_journal::SessionId(session_id),
            derived_key: derived_key.to_bytes().to_vec(),
            derived_public_key: derived_key,
            transcript_hash: commitment_root,
            threshold_signature,
            ledger_events,
            participants: self.ctx.participants().clone(),
        })
    }

    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<DkdProtocolResult, ProtocolError> {
        let finalize_event = EventAwaiter::new(self.ctx)
            .for_session(winning_session.session_id.0)
            .for_event_types(vec![EventTypePattern::DkdFinalize])
            .await_single(100) // Default TTL epochs
            .await?;

        match &finalize_event.event_type {
            EventType::FinalizeDkdSession(finalize) => {
                // Reconstruct protocol result from finalize event
                let derived_public_key = ed25519_dalek::VerifyingKey::from_bytes(
                    &finalize.derived_identity_pk[..32]
                        .try_into()
                        .map_err(|_| ProtocolError {
                            session_id: self.ctx.session_id(),
                            error_type: ProtocolErrorType::InvalidState,
                            message: "Invalid key length in finalize event".to_string(),
                        })?,
                )
                .map_err(|e| ProtocolError {
                    session_id: self.ctx.session_id(),
                    error_type: ProtocolErrorType::Other,
                    message: format!("Invalid public key: {:?}", e),
                })?;

                // Create a placeholder signature - in production this would be collected from signers
                let signature = ed25519_dalek::Signature::from_bytes(&[0u8; 64]);

                Ok(DkdProtocolResult {
                    session_id: winning_session.session_id,
                    derived_key: finalize.derived_identity_pk.clone(),
                    derived_public_key,
                    transcript_hash: finalize.commitment_root,
                    threshold_signature: crate::ThresholdSignature {
                        signature,
                        signers: vec![], // Would be populated with actual ParticipantIds from signing participants
                    },
                    ledger_events: vec![finalize_event],
                    participants: winning_session
                        .participants
                        .iter()
                        .filter_map(|p| match p {
                            JournalParticipantId::Device(d) => Some(*d),
                            _ => None,
                        })
                        .collect(),
                })
            }
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
                        message: format!(
                            "Invalid commitment event type from {:?}",
                            reveal_author.0
                        ),
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
                        reveal_author.0, commitment_hash, calculated_hash
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
pub async fn dkd_choreography(
    ctx: &mut ProtocolContext,
    context_id: Vec<u8>,
) -> Result<DkdProtocolResult, ProtocolError> {
    let mut protocol = DkdProtocol::new(ctx, context_id);
    protocol.execute().await
}

// ========== Tests ==========

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use crate::execution::context::StubTransport;
    use aura_crypto::Effects;
    use aura_journal::{AccountLedger, AccountState};
    use aura_types::{AccountId, DeviceId};
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
            AccountId(Uuid::from_bytes([6u8; 16])),
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

    #[test]
    fn test_dkd_session_state_transitions() {
        let device_id = DeviceId(Uuid::new_v4());
        let app_id = "test-app".to_string();
        let context = "test-context".to_string();

        // Test protocol creation
        let protocol = new_dkd_protocol(device_id, app_id.clone(), context.clone()).unwrap();
        assert!(!protocol.is_final());
        assert_eq!(protocol.state_name(), "InitializationPhase");

        // Test state transition to commitment phase
        let config = CommitmentConfig::default();
        let protocol = protocol.begin_commitment_phase(config).unwrap();
        assert_eq!(protocol.state_name(), "CommitmentPhase");

        // Test state transition to reveal phase
        let commitments = CollectedCommitments::default();
        let protocol = protocol.begin_reveal_phase(commitments).unwrap();
        assert_eq!(protocol.state_name(), "RevealPhase");

        // Test state transition to finalization phase
        let reveals = VerifiedReveals::default();
        let protocol = protocol.begin_finalization_phase(reveals).unwrap();
        assert_eq!(protocol.state_name(), "FinalizationPhase");

        // Test completion
        let completion = DkdCompleted {
            derived_key: vec![1, 2, 3, 4],
            context: context.clone(),
            app_id: app_id.clone(),
        };
        let protocol = protocol.complete(completion).unwrap();
        assert_eq!(protocol.state_name(), "CompletionPhase");
        assert!(protocol.is_final());
    }

    #[test]
    fn test_dkd_failure_transition() {
        let device_id = DeviceId(Uuid::new_v4());
        let app_id = "test-app".to_string();
        let context = "test-context".to_string();

        let protocol = new_dkd_protocol(device_id, app_id, context).unwrap();

        // Test failure transition from any non-final state
        let failed_protocol = protocol.fail("Test failure".to_string()).unwrap();
        assert_eq!(failed_protocol.state_name(), "Failure");
        assert!(failed_protocol.is_final());
    }
}
