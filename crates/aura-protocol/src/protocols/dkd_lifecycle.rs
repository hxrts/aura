//! DKD protocol lifecycle with real cryptographic operations
//!
//! Implements deterministic key derivation using a multi-round protocol:
//! 1. Coordinator broadcasts context_id
//! 2. Participants compute commitments and send them
//! 3. After all commitments received, participants reveal points
//! 4. Coordinator aggregates points into derived key

use super::DkdLifecycleError;
use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects, ProtocolMessage},
    lifecycle::{
        transition_from_witness, ProtocolDescriptor, ProtocolInput, ProtocolLifecycle,
        ProtocolRehydration, ProtocolStep,
    },
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use crate::{protocol_results::DkdProtocolResult, ParticipantId, ThresholdSignature};
use aura_crypto::{dkd, Ed25519Signature};
use aura_journal::SessionId as JournalSessionId;
use aura_types::{AccountId, AuraError, DeviceId, SessionId};
use curve25519_dalek::traits::Identity;
use std::collections::HashMap;
use uuid::Uuid;

/// Typestate marker for the DKD lifecycle
#[derive(Debug, Clone)]
pub struct DkdLifecycleState;

impl SessionState for DkdLifecycleState {
    const NAME: &'static str = "DkdLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// DKD protocol state machine states
#[derive(Debug, Clone)]
enum DkdState {
    /// Initial state - waiting to start
    Init,
    /// Waiting for context_id from coordinator (non-coordinator only)
    AwaitingContext,
    /// Computing local commitment
    ComputingCommitment { context_id: Vec<u8> },
    /// Waiting for commitments from all participants
    AwaitingCommitments {
        context_id: Vec<u8>,
        own_commitment: [u8; 32],
        own_point: [u8; 32],
        received_commitments: HashMap<DeviceId, [u8; 32]>,
    },
    /// Revealing point after all commitments received
    RevealingPoint {
        context_id: Vec<u8>,
        own_point: [u8; 32],
        commitments: HashMap<DeviceId, [u8; 32]>,
    },
    /// Waiting for revealed points from all participants
    AwaitingReveals {
        context_id: Vec<u8>,
        commitments: HashMap<DeviceId, [u8; 32]>,
        revealed_points: HashMap<DeviceId, [u8; 32]>,
    },
    /// Aggregating points and deriving final key
    Aggregating {
        context_id: Vec<u8>,
        revealed_points: HashMap<DeviceId, [u8; 32]>,
    },
    /// Protocol completed successfully
    Complete(DkdProtocolResult),
    /// Protocol failed
    Failed(AuraError),
}

/// DKD protocol lifecycle implementation
pub struct DkdLifecycle {
    descriptor: ProtocolDescriptor,
    state: DkdState,
    /// Local key share (16 bytes from FROST share)
    key_share: Option<[u8; 16]>,
    participants: Vec<DeviceId>,
    /// Whether this device is the coordinator (first in participants list)
    is_coordinator: bool,
}

impl DkdLifecycle {
    /// Create a new DKD lifecycle instance
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        context_id: Vec<u8>,
        participants: Vec<DeviceId>,
    ) -> Self {
        let descriptor =
            ProtocolDescriptor::new(Uuid::new_v4(), session_id, device_id, ProtocolType::Dkd)
                .with_operation_type(OperationType::Dkd)
                .with_priority(ProtocolPriority::High)
                .with_mode(ProtocolMode::Interactive);

        let is_coordinator = participants.first() == Some(&device_id);

        let initial_state = if is_coordinator && !context_id.is_empty() {
            // Coordinator starts in ComputingCommitment if context provided
            DkdState::ComputingCommitment { context_id }
        } else if is_coordinator {
            // Coordinator waits for local signal to start
            DkdState::Init
        } else {
            // Non-coordinator waits for context from coordinator
            DkdState::AwaitingContext
        };

        Self {
            descriptor,
            state: initial_state,
            key_share: None,
            participants,
            is_coordinator,
        }
    }

    /// Convenience constructor for ephemeral sessions
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(
        device_id: DeviceId,
        context_id: Vec<u8>,
        participants: Vec<DeviceId>,
    ) -> Self {
        Self::new(device_id, SessionId::new(), context_id, participants)
    }

    /// Set the key share for this participant
    pub fn with_key_share(mut self, share: [u8; 16]) -> Self {
        self.key_share = Some(share);
        self
    }

    /// Get coordinator device ID
    fn coordinator(&self) -> DeviceId {
        self.participants
            .first()
            .copied()
            .unwrap_or(self.descriptor.device_id)
    }
}

impl ProtocolLifecycle for DkdLifecycle {
    type State = DkdLifecycleState;
    type Output = DkdProtocolResult;
    type Error = DkdLifecycleError;

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match (&self.state, input) {
            // Coordinator: Start signal initiates protocol
            (DkdState::Init, ProtocolInput::LocalSignal { signal, data })
                if signal == "start" && self.is_coordinator =>
            {
                let context_id = if let Some(data) = data {
                    // Extract context_id from data
                    if let Ok(ctx) = serde_json::from_value::<Vec<u8>>(data.clone()) {
                        ctx
                    } else {
                        vec![]
                    }
                } else {
                    // Generate random context if none provided
                    caps.effects.random_bytes_vec(32)
                };

                // Broadcast context to all participants
                let mut effects = Vec::new();
                for participant in &self.participants {
                    if *participant != self.descriptor.device_id {
                        let message = ProtocolMessage {
                            from: self.descriptor.device_id,
                            to: *participant,
                            payload: serde_json::to_vec(&context_id).unwrap_or_default(),
                            session_id: Some(self.descriptor.session_id.uuid()),
                        };
                        effects.push(ProtocolEffects::Send { message });
                    }
                }

                self.state = DkdState::ComputingCommitment {
                    context_id: context_id.clone(),
                };

                ProtocolStep::progress(effects, None)
            }

            // Non-coordinator: Receive context from coordinator
            (DkdState::AwaitingContext, ProtocolInput::Message(msg))
                if msg.from == self.coordinator() =>
            {
                let context_id = serde_json::from_slice(&msg.payload).unwrap_or_default();
                self.state = DkdState::ComputingCommitment { context_id };
                ProtocolStep::progress(Vec::new(), None)
            }

            // Compute commitment using local key share
            (DkdState::ComputingCommitment { context_id }, _) => {
                let key_share = match self.key_share {
                    Some(share) => share,
                    None => {
                        self.state = DkdState::Failed(AuraError::agent_invalid_state(
                            "No key share provided",
                        ));
                        return ProtocolStep::completed(
                            Vec::new(),
                            None,
                            Err(AuraError::agent_invalid_state("No key share provided")),
                        );
                    }
                };

                // Compute commitment and point using DKD crypto
                let (point, commitment) = dkd::participant_dkd_phase(&key_share, context_id);
                let point_bytes = point.compress().to_bytes();

                // Send commitment to all participants
                let mut effects = Vec::new();
                for participant in &self.participants {
                    if *participant != self.descriptor.device_id {
                        let message = ProtocolMessage {
                            from: self.descriptor.device_id,
                            to: *participant,
                            payload: serde_json::to_vec(&commitment).unwrap_or_default(),
                            session_id: Some(self.descriptor.session_id.uuid()),
                        };
                        effects.push(ProtocolEffects::Send { message });
                    }
                }

                let mut received_commitments = HashMap::new();
                received_commitments.insert(self.descriptor.device_id, commitment);

                self.state = DkdState::AwaitingCommitments {
                    context_id: context_id.clone(),
                    own_commitment: commitment,
                    own_point: point_bytes,
                    received_commitments,
                };

                ProtocolStep::progress(effects, None)
            }

            // Receive commitments from other participants
            (
                DkdState::AwaitingCommitments {
                    context_id,
                    own_commitment,
                    own_point,
                    received_commitments,
                },
                ProtocolInput::Message(msg),
            ) if self.participants.contains(&msg.from) => {
                let commitment: [u8; 32] = match serde_json::from_slice(&msg.payload) {
                    Ok(c) => c,
                    Err(_) => return ProtocolStep::progress(Vec::new(), None),
                };

                let mut commitments = received_commitments.clone();
                commitments.insert(msg.from, commitment);

                // Check if we have all commitments
                if commitments.len() == self.participants.len() {
                    // All commitments received, move to revealing
                    self.state = DkdState::RevealingPoint {
                        context_id: context_id.clone(),
                        own_point: *own_point,
                        commitments: commitments.clone(),
                    };
                } else {
                    self.state = DkdState::AwaitingCommitments {
                        context_id: context_id.clone(),
                        own_commitment: *own_commitment,
                        own_point: *own_point,
                        received_commitments: commitments,
                    };
                }

                ProtocolStep::progress(Vec::new(), None)
            }

            // Reveal point after all commitments received
            (
                DkdState::RevealingPoint {
                    context_id,
                    own_point,
                    commitments,
                },
                _,
            ) => {
                // Send revealed point to all participants
                let mut effects = Vec::new();
                for participant in &self.participants {
                    if *participant != self.descriptor.device_id {
                        let message = ProtocolMessage {
                            from: self.descriptor.device_id,
                            to: *participant,
                            payload: serde_json::to_vec(own_point).unwrap_or_default(),
                            session_id: Some(self.descriptor.session_id.uuid()),
                        };
                        effects.push(ProtocolEffects::Send { message });
                    }
                }

                let mut revealed_points = HashMap::new();
                revealed_points.insert(self.descriptor.device_id, *own_point);

                self.state = DkdState::AwaitingReveals {
                    context_id: context_id.clone(),
                    commitments: commitments.clone(),
                    revealed_points,
                };

                ProtocolStep::progress(effects, None)
            }

            // Receive revealed points from other participants
            (
                DkdState::AwaitingReveals {
                    context_id,
                    commitments,
                    revealed_points,
                },
                ProtocolInput::Message(msg),
            ) if self.participants.contains(&msg.from) => {
                let point: [u8; 32] = match serde_json::from_slice(&msg.payload) {
                    Ok(p) => p,
                    Err(_) => return ProtocolStep::progress(Vec::new(), None),
                };

                // Verify point matches commitment
                let computed_commitment = dkd::compute_commitment(
                    &curve25519_dalek::edwards::CompressedEdwardsY::from_slice(&point)
                        .ok()
                        .and_then(|c| c.decompress())
                        .unwrap_or(curve25519_dalek::edwards::EdwardsPoint::identity()),
                );

                if commitments.get(&msg.from) != Some(&computed_commitment) {
                    // Commitment mismatch - Byzantine fault
                    self.state = DkdState::Failed(AuraError::coordination_failed(
                        "DKD commitment verification failed",
                    ));
                    return ProtocolStep::completed(
                        Vec::new(),
                        None,
                        Err(AuraError::coordination_failed(
                            "DKD commitment verification failed",
                        )),
                    );
                }

                let mut points = revealed_points.clone();
                points.insert(msg.from, point);

                // Check if we have all reveals
                if points.len() == self.participants.len() {
                    self.state = DkdState::Aggregating {
                        context_id: context_id.clone(),
                        revealed_points: points,
                    };
                } else {
                    self.state = DkdState::AwaitingReveals {
                        context_id: context_id.clone(),
                        commitments: commitments.clone(),
                        revealed_points: points,
                    };
                }

                ProtocolStep::progress(Vec::new(), None)
            }

            // Aggregate points into final derived key
            (DkdState::Aggregating { revealed_points, .. }, _) => {
                let points: Vec<[u8; 32]> = revealed_points.values().copied().collect();

                let derived_public_key = match dkd::aggregate_dkd_points(&points) {
                    Ok(key) => key,
                    Err(e) => {
                        self.state = DkdState::Failed(AuraError::crypto_operation_failed(
                            format!("DKD aggregation failed: {}", e),
                        ));
                        return ProtocolStep::completed(
                            Vec::new(),
                            None,
                            Err(AuraError::crypto_operation_failed(
                                "DKD aggregation failed".to_string(),
                            )),
                        );
                    }
                };

                // Create transcript hash from all points
                let mut transcript = Vec::new();
                for point in &points {
                    transcript.extend_from_slice(point);
                }
                let transcript_hash = *blake3::hash(&transcript).as_bytes();

                // Create result
                let result = DkdProtocolResult {
                    session_id: JournalSessionId::from_uuid(self.descriptor.session_id.uuid()),
                    derived_key: derived_public_key.to_bytes().to_vec(),
                    derived_public_key,
                    transcript_hash,
                    threshold_signature: ThresholdSignature {
                        signature: Ed25519Signature::default(), // TODO: Add real threshold signature
                        signers: self
                            .participants
                            .iter()
                            .enumerate()
                            .filter_map(|(i, _)| {
                                std::num::NonZeroU16::new((i + 1) as u16).map(ParticipantId::new)
                            })
                            .collect(),
                    },
                    ledger_events: Vec::new(),
                    participants: self.participants.clone(),
                    capability_proof: crate::protocols::CapabilityProofBuilder::new(
                        self.descriptor.device_id,
                        "dkd",
                    )
                    .create_proof("dkd_derived_keys", "dkd_key_derivation")
                    .unwrap_or_else(|_| {
                        crate::protocols::CapabilityProofBuilder::create_placeholder()
                    }),
                };

                self.state = DkdState::Complete(result.clone());

                ProtocolStep::completed(
                    Vec::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        DkdLifecycleState::NAME,
                        "DkdCompleted",
                        None,
                    )),
                    Ok(result),
                )
            }

            // Return completed result
            (DkdState::Complete(result), _) => ProtocolStep::completed(
                Vec::new(),
                None,
                Ok(result.clone()),
            ),

            // Return failed error
            (DkdState::Failed(error), _) => {
                ProtocolStep::completed(Vec::new(), None, Err(error.clone()))
            }

            // Abort signal
            (_, ProtocolInput::LocalSignal { signal, .. }) if signal == "abort" => {
                self.state = DkdState::Failed(AuraError::session_aborted("DKD aborted"));
                ProtocolStep::completed(
                    Vec::new(),
                    Some(transition_from_witness(
                        &self.descriptor,
                        DkdLifecycleState::NAME,
                        "DkdAborted",
                        None,
                    )),
                    Err(AuraError::session_aborted("DKD aborted")),
                )
            }

            // Unknown input - ignore
            _ => ProtocolStep::progress(Vec::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        matches!(self.state, DkdState::Complete(_) | DkdState::Failed(_))
    }
}

impl ProtocolRehydration for DkdLifecycle {
    type Evidence = ();

    fn validate_evidence(_evidence: &Self::Evidence) -> bool {
        true
    }

    fn rehydrate(
        device_id: DeviceId,
        account_id: AccountId,
        _evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        let _ = account_id;
        Ok(Self::new_ephemeral(device_id, Vec::new(), Vec::new()))
    }
}
